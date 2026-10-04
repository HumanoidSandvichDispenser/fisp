use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use fisp_evaluator::{
    eval::EvalError,
    expr::Value,
    parser::ParseError,
    runtime::{Runtime, RuntimeError},
};
use fuser::{self, Errno, FileType, Filesystem, INodeNo};

use crate::evaluator::Evaluator;

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
        Table {
            nodes: Vec::new(),
            inos: HashMap::new(),
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
            table: Mutex::new(Table {
                nodes: Vec::new(),
                inos: HashMap::new(),
            }),
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
    }
}
