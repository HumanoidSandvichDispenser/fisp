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
    self, Errno, FileHandle, FileType, Filesystem, FopenFlags, INodeNo,
    OpenAccMode::{self, O_WRONLY},
    OpenFlags,
};

use crate::evaluator::Evaluator;

#[derive(Debug, Clone)]
enum Kind {
    Dir,
    File(String),
    Symlink(String),
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

pub struct FispFilesystem {
    evaluator: Evaluator,
    table: Mutex<Table>,
    revision: AtomicU64,
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
            uid,
            gid,
        }
    }

    fn attr(&self, ino: INodeNo, kind: &Kind) -> fuser::FileAttr {
        let now = SystemTime::now();

        let (size, file_type, perm, nlink) = match kind {
            Kind::Dir => (0, FileType::Directory, 0o755, 2),
            Kind::File(content) => (content.len() as u64, FileType::RegularFile, 0o444, 1),
            Kind::Symlink(target) => (target.len() as u64, FileType::Symlink, 0o777, 1),
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

    fn kind(&self, ino: INodeNo) -> Result<Kind, Errno> {
        let revision = self.revision.load(Ordering::Acquire);

        let path: Vec<String> = {
            let table = self.table.lock().unwrap();
            let node = table.get(ino).ok_or(Errno::ENOENT)?;
            node.path.clone()
        };

        let kind = self.evaluator.run({
            let path = path.clone();
            move |rt| FispFilesystem::resolve(rt, &path)
        })?;

        Ok(kind)
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
            match rt.evaluate(&path.join("/")) {
                Ok(Value::Closure { .. }) => Ok(Kind::Dir),
                Ok(value) => Ok(Kind::File(format!("{}\n", value))),
                Err(RuntimeError::ParseError(ParseError::BadName)) => Err(Errno::ENOENT),
                Err(RuntimeError::ParseError(ParseError::StringNotTerminated)) => Err(Errno::ENOENT),
                Err(RuntimeError::ParseError(ParseError::StringDecodeError)) => Err(Errno::EILSEQ),
                Err(RuntimeError::ParseError(ParseError::Incomplete)) => Ok(Kind::Dir),
                Err(RuntimeError::EvalError(EvalError::UnresolvedSymbol(_))) => Err(Errno::ENOENT),
                Err(RuntimeError::EvalError(_)) => Err(Errno::EIO),
                Err(RuntimeError::DefinitionError(_)) => Err(Errno::EEXIST),
            }
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
        let parent_path: Vec<String> = {
            let table = self.table.lock().unwrap();
            let node = table.get(parent).ok_or(Errno::ENOENT);
            match node {
                Ok(node) => node.path.clone(),
                Err(errno) => {
                    reply.error(errno);
                    return;
                }
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
        match self.kind(ino) {
            Ok(Kind::File(content)) => {
                let data = content.as_bytes();
                let end = std::cmp::min(offset as usize + size as usize, data.len());
                let slice = &data[offset as usize..end];
                reply.data(slice);
            }
            Ok(_) => {
                reply.error(Errno::EISDIR);
            }
            Err(errno) => {
                reply.error(errno);
            }
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
        _ino: INodeNo,
        flags: fuser::OpenFlags,
        reply: fuser::ReplyOpen,
    ) {
        match flags.acc_mode() {
            OpenAccMode::O_RDONLY => reply.opened(FileHandle(0), FopenFlags::empty()),
            OpenAccMode::O_WRONLY | OpenAccMode::O_RDWR => reply.error(Errno::EACCES),
        }
    }
}
