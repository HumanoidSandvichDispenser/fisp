use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use fisp_evaluator::{
    eval::EvalError,
    expr::Value,
    parser::ParseError,
    runtime::{Runtime, RuntimeError},
};
use fuser::{
    self, Errno, FileHandle, FileType, Filesystem, FopenFlags, INodeNo, OpenAccMode, OpenFlags,
};

use crate::evaluator::Evaluator;

#[derive(Debug, Clone)]
enum Kind {
    Dir,
    File(String),
    Symlink(String),
    Session(Vec<u8>),
}

struct Node {
    path: Vec<String>,
    kind: Kind,
    generation: u64,
}

struct Table {
    nodes: Vec<Node>,
    inos: HashMap<Vec<String>, INodeNo>,
}

impl Table {
    fn new() -> Self {
        let root = Node {
            path: Vec::new(),
            kind: Kind::Dir,
            generation: 0,
        };

        // index 0 is ino 1 (root)
        Table {
            nodes: vec![root],
            inos: HashMap::from([(Vec::new(), INodeNo::ROOT)]),
        }
    }

    fn get(&self, ino: INodeNo) -> Option<&Node> {
        let index = u64::from(ino).checked_sub(1)?;
        self.nodes.get(index as usize)
    }

    fn get_mut(&mut self, ino: INodeNo) -> Option<&mut Node> {
        let index = u64::from(ino).checked_sub(1)?;
        self.nodes.get_mut(index as usize)
    }

    fn insert(&mut self, path: Vec<String>, kind: Kind, generation: u64) -> INodeNo {
        if let Some(&ino) = self.inos.get(&path) {
            if let Some(node) = self.get_mut(ino) {
                node.kind = kind;
                node.generation = generation;
            }

            return ino;
        }

        let ino = INodeNo(self.nodes.len() as u64 + 1);
        self.inos.insert(path.clone(), ino);
        self.nodes.push(Node {
            path,
            kind,
            generation,
        });

        ino
    }
}

enum SessionState {
    /// A plain file that was evaluated and is ready to be read.
    Done { output: Vec<u8> },
    /// An `in` session that is currently collecting input to be evaluated.
    Collecting { path: Vec<String>, input: Vec<u8> },
    /// An `in` session whose output is being read.
    Collected { output: Vec<u8>, pos: usize },
}

pub struct FispFilesystem {
    evaluator: Evaluator,
    table: Mutex<Table>,
    revision: AtomicU64,
    next_fh: AtomicU64,
    sessions: Mutex<HashMap<FileHandle, SessionState>>,
    uid: u32,
    gid: u32,
}

impl FispFilesystem {
    pub fn new() -> Self {
        let uid = nix::unistd::getuid().as_raw();
        let gid = nix::unistd::getgid().as_raw();

        FispFilesystem {
            evaluator: Evaluator::spawn().unwrap(),
            table: Mutex::new(Table::new()),
            revision: AtomicU64::new(0),
            next_fh: AtomicU64::new(0),
            uid,
            gid,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn attr(&self, ino: INodeNo, kind: &Kind) -> fuser::FileAttr {
        let now = SystemTime::now();

        let (size, file_type, perm, nlink) = match kind {
            Kind::Dir => (0, FileType::Directory, 0o755, 2),
            Kind::File(content) => (content.len() as u64, FileType::RegularFile, 0o444, 1),
            Kind::Symlink(target) => (target.len() as u64, FileType::Symlink, 0o777, 1),
            Kind::Session(content) => (0, FileType::RegularFile, 0o600, 1),
        };

        // get uid and gid from current user (the user running the process)
        fuser::FileAttr {
            ino,
            size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: file_type,
            perm,
            nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    fn path(&self, ino: INodeNo) -> Result<Vec<String>, Errno> {
        let table = self.table.lock().unwrap();
        let node = table.get(ino).ok_or(Errno::ENOENT)?;
        Ok(node.path.clone())
    }

    fn kind(&self, ino: INodeNo) -> Result<Kind, Errno> {
        let path = self.path(ino)?;
        self.evaluator
            .run(move |rt| FispFilesystem::resolve(rt, &path))
    }

    fn resolve(rt: &Runtime, path: &Vec<String>) -> Result<Kind, Errno> {
        // if path is a defined name in root -> symlink
        // incomplete path -> dir
        // function -> dir
        // any other value -> file
        // bad name -> ENOENT
        // eval error -> EIO

        if let [name] = path.as_slice()
            && let Some(source) = rt.source(name)
        {
            Ok(Kind::Symlink(source))
        } else {
            let ast = rt.parse(&path.join("/"));

            let ast = match ast {
                Ok(ast) => ast,
                Err(ParseError::BadName) => return Err(Errno::ENOENT),
                Err(ParseError::StringNotTerminated) => return Err(Errno::ENOENT),
                Err(ParseError::StringDecodeError) => return Err(Errno::EILSEQ),
                Err(ParseError::Incomplete) => return Ok(Kind::Dir),
            };

            if ast.uses("in") {
                return Ok(Kind::Session(Vec::new()));
            }

            match rt.evaluate_ast(&ast) {
                Ok(Value::Closure { .. }) => Ok(Kind::Dir),
                Ok(Value::String(s)) => Ok(Kind::File(s)),
                Ok(value) => Ok(Kind::File(format!("{}\n", value.path_expr()))),
                Err(RuntimeError::ParseError(_)) => unreachable!(),
                Err(RuntimeError::EvalError(EvalError::UnresolvedSymbol(_))) => Err(Errno::ENOENT),
                Err(RuntimeError::EvalError(_)) => Err(Errno::EIO),
                Err(RuntimeError::DefinitionError(_)) => Err(Errno::EEXIST),
            }
        }
    }

    fn next_chunk<'a>(output: &'a [u8], pos: &mut usize, size: u32) -> &'a [u8] {
        let start = *pos;
        let end = (start + size as usize).min(output.len());
        *pos = end;
        &output[start..end]
    }

    fn evaluate_with_input(
        rt: &Runtime,
        path: &[String],
        input: Vec<u8>,
    ) -> Result<Vec<u8>, Errno> {
        let input = String::from_utf8(input).map_err(|_| Errno::EILSEQ)?;
        let ast = rt.parse(&path.join("/")).map_err(|_| Errno::ENOENT)?;

        let input_value = Value::String(input);
        let bindings = vec![("in".to_string(), input_value)];

        match rt.evaluate_ast_with(&ast, bindings) {
            Ok(Value::Closure { .. }) => Err(Errno::EISDIR),
            Ok(Value::String(s)) => Ok(s.into_bytes()),
            Ok(value) => Ok(format!("{}\n", value.path_expr()).into_bytes()),
            Err(RuntimeError::EvalError(EvalError::UnresolvedSymbol(_))) => Err(Errno::ENOENT),
            Err(_) => Err(Errno::EIO),
        }
    }

    fn is_session(&self, ino: INodeNo) -> bool {
        let table = self.table.lock().unwrap();
        if let Some(node) = table.get(ino) {
            matches!(node.kind, Kind::Session(_))
        } else {
            false
        }
    }
}

impl Filesystem for FispFilesystem {
    fn getattr(
        &self,
        _req: &fuser::Request,
        ino: fuser::INodeNo,
        fh: Option<fuser::FileHandle>,
        reply: fuser::ReplyAttr,
    ) {
        match self.kind(ino) {
            Ok(kind) => {
                let attr = self.attr(ino, &kind);
                reply.attr(&Duration::ZERO, &attr);
            }
            Err(errno) => {
                reply.error(errno);
            }
        }
    }

    fn lookup(
        &self,
        _req: &fuser::Request,
        parent: INodeNo,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        let parent_path = match self.path(parent) {
            Ok(path) => path,
            Err(errno) => {
                reply.error(errno);
                return;
            }
        };

        let path = {
            let mut path = parent_path.clone();
            path.push(name.to_string_lossy().to_string());
            path
        };

        let eval = self.evaluator.run({
            let path = path.clone();
            move |rt| FispFilesystem::resolve(rt, &path)
        });

        match &eval {
            Ok(kind) => {
                let ino = self.table.lock().unwrap().insert(
                    path,
                    kind.clone(),
                    self.revision.load(Ordering::Acquire),
                );

                let attr = self.attr(ino, &kind);

                reply.entry(&Duration::ZERO, &attr, fuser::Generation(0));
            }
            Err(errno) => {
                reply.error(*errno);
            }
        }
    }

    fn read(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        fh: fuser::FileHandle,
        offset: u64,
        size: u32,
        flags: fuser::OpenFlags,
        lock_owner: Option<fuser::LockOwner>,
        reply: fuser::ReplyData,
    ) {
        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(&fh) {
            Some(SessionState::Done { output }) => {
                let start = (offset as usize).min(output.len());
                let end = (start + size as usize).min(output.len());
                let slice = &output[start..end];
                reply.data(slice);
            }
            Some(SessionState::Collecting { path, input }) => {
                let path = path.clone();
                let input = std::mem::take(input);
                drop(sessions);

                // drop the lock before running the evaluator
                let result = self
                    .evaluator
                    .run(move |rt| FispFilesystem::evaluate_with_input(rt, &path, input));

                // reacquire
                let mut sessions = self.sessions.lock().unwrap();
                match result {
                    Ok(output) => {
                        let mut pos = 0;
                        reply.data(FispFilesystem::next_chunk(&output, &mut pos, size));
                        sessions.insert(fh, SessionState::Collected { output, pos });
                    }
                    Err(errno) => {
                        let output = Vec::new();
                        sessions.insert(fh, SessionState::Collected { output, pos: 0 });
                        reply.error(errno);
                    }
                }
            }
            Some(SessionState::Collected { output, pos }) => {
                let chunk = FispFilesystem::next_chunk(output, pos, size);
                reply.data(chunk);
            }
            None => reply.error(Errno::EBADF),
        }
    }

    fn readlink(&self, _req: &fuser::Request, ino: INodeNo, reply: fuser::ReplyData) {
        match self.kind(ino) {
            Ok(Kind::Symlink(target)) => {
                reply.data(target.as_bytes());
            }
            Ok(_) => {
                reply.error(Errno::EINVAL);
            }
            Err(errno) => {
                reply.error(errno);
            }
        }
    }

    fn readdir(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        fh: fuser::FileHandle,
        offset: u64,
        mut reply: fuser::ReplyDirectory,
    ) {
        // append "." and ".." entries and any global definitions
        let mut entries = vec![
            (ino, FileType::Directory, ".".to_owned()),
            (ino, FileType::Directory, "..".to_owned()),
        ];

        if ino == INodeNo::ROOT {
            let names = self.evaluator.run(|rt| {
                rt.definitions()
                    .map(|(name, _)| name.to_owned())
                    .collect::<Vec<_>>()
            });

            let mut table = self.table.lock().unwrap();
            let revision = self.revision.load(Ordering::Acquire);

            entries.extend(names.into_iter().map(|name| {
                let ino = table.insert(vec![name.clone()], Kind::Symlink(name.clone()), revision);
                (ino, FileType::Symlink, name)
            }))
        }

        for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(ino, (i + 1) as u64, kind, name) {
                break;
            }
        }

        reply.ok();
    }

    fn symlink(
        &self,
        _req: &fuser::Request,
        parent: INodeNo,
        link_name: &std::ffi::OsStr,
        target: &std::path::Path,
        reply: fuser::ReplyEntry,
    ) {
        if parent != INodeNo::ROOT {
            return reply.error(Errno::EPERM);
        }

        let (Some(name), Some(source)) = (link_name.to_str(), target.to_str()) else {
            return reply.error(Errno::EINVAL);
        };

        if target.is_absolute() {
            return reply.error(Errno::EINVAL);
        }

        let (name, source) = (name.to_owned(), source.to_owned());

        let result = self.evaluator.run({
            let (name, source) = (name.clone(), source.clone());
            move |rt| {
                rt.define(&name, &source).map_err(|err| match err {
                    RuntimeError::DefinitionError(_) => Errno::EEXIST,
                    err => {
                        eprintln!("{name}: {err:?}");
                        Errno::EINVAL
                    }
                })
            }
        });

        match result {
            Ok(()) => {
                let generation = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                let kind = Kind::Symlink(source.clone());
                let ino =
                    self.table
                        .lock()
                        .unwrap()
                        .insert(vec![name.clone()], kind.clone(), generation);

                reply.entry(
                    &Duration::ZERO,
                    &self.attr(ino, &kind),
                    fuser::Generation(0),
                );
            }
            Err(errno) => {
                reply.error(errno);
            }
        }
    }

    fn unlink(
        &self,
        _req: &fuser::Request,
        parent: INodeNo,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        if parent != INodeNo::ROOT {
            return reply.error(Errno::EPERM);
        }

        let Some(name) = name.to_str() else {
            return reply.error(Errno::EINVAL);
        };

        let result = self.evaluator.run({
            let name = name.to_owned();
            move |rt| rt.undefine(&name).map_err(|_| Errno::ENOENT)
        });

        match result {
            Ok(()) => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                self.table
                    .lock()
                    .unwrap()
                    .inos
                    .remove(&vec![name.to_owned()]);
                reply.ok();
            }
            Err(errno) => {
                reply.error(errno);
            }
        }
    }

    fn open(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        flags: fuser::OpenFlags,
        reply: fuser::ReplyOpen,
    ) {
        let read_only = matches!(flags.acc_mode(), OpenAccMode::O_RDONLY);
        if !read_only && !self.is_session(ino) {
            return reply.error(Errno::EACCES);
        }

        let path = match self.path(ino) {
            Ok(path) => path,
            Err(errno) => return reply.error(errno),
        };

        // TODO: refactor into kind(path)
        let kind = self.evaluator.run({
            let path = path.clone();
            move |rt| FispFilesystem::resolve(rt, &path)
        });

        let session = match kind {
            Ok(Kind::File(content)) => SessionState::Done {
                output: content.into_bytes(),
            },
            Ok(Kind::Dir) => return reply.error(Errno::EISDIR),
            Ok(Kind::Symlink(_)) => return reply.error(Errno::ELOOP),
            Ok(Kind::Session(_)) => SessionState::Collecting {
                path,
                input: Vec::new(),
            },
            Err(errno) => return reply.error(errno),
        };

        let open_flags = match session {
            SessionState::Collecting { .. } => {
                FopenFlags::FOPEN_DIRECT_IO | FopenFlags::FOPEN_STREAM
            }
            _ => FopenFlags::FOPEN_DIRECT_IO,
        };

        let fh = FileHandle(self.next_fh.fetch_add(1, Ordering::Relaxed));

        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(fh, session);
        }

        reply.opened(fh, open_flags);
    }

    fn release(
        &self,
        _req: &fuser::Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        // remove the session for the file handle
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(&_fh);
        }
        reply.ok();
    }

    fn write(
        &self,
        _req: &fuser::Request,
        _ino: INodeNo,
        fh: FileHandle,
        _offset: u64,
        data: &[u8],
        _write_flags: fuser::WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: fuser::ReplyWrite,
    ) {
        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(&fh) {
            Some(SessionState::Collecting { path: _, input }) => {
                input.extend_from_slice(data);
                reply.written(data.len() as u32);
            }
            Some(SessionState::Done { output: _ }) => {
                reply.error(Errno::EBADF);
            }
            Some(SessionState::Collected { output: _, pos: _ }) => {
                reply.error(Errno::EPIPE);
            }
            None => {
                reply.error(Errno::EBADF);
            }
        }
    }
}
