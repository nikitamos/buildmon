#![allow(dead_code)]

use std::{
    io::{Read, Seek},
    num::IntErrorKind,
    path::Path,
};

use bumpalo::Bump;

mod bump_ext;

pub enum ProcessKind {
    Compiler,
    Linker,
    Tool,
    Build(BuildSystem),
}

pub enum Language {
    C,
    CXX,
    Rust,
    Zig,
}

pub enum BuildSystem {
    CMake,
    Ninja,
    Make,
    Cargo,
    ZigBuild,
    Heretical(String),
}

pub enum ProcessError {
    Dead,
    UnknownClass,
}

pub struct ProcessClassification {
    kind: ProcessKind,
}

impl ProcessClassification {
    pub const fn unknown() -> Self {
        todo!()
    }
}

pub trait ProcessClassifier {
    fn classify(&self, proc: &CompilationProcess) -> Result<ProcessClassification, ProcessError>;
}

#[derive(Debug)]
pub struct CompilationProcess {
    cmdline: String,
    pid: u32,
}

impl CompilationProcess {
    pub fn from_pid(pid: u32) -> Self {
        Self {
            cmdline: String::new(),
            pid,
        }
    }
    const BUF_CAPACITY: usize = 512;
    pub fn is_dead() {}
    // pub fn from_pid
    pub fn children_pids<'s, 'b>(
        &'s self,
        arena: &'b Bump,
    ) -> std::io::Result<impl Iterator<Item = u32> + use<'b>> {
        let pid_str = self.pid.to_string();
        let task_dir = Path::new("/proc").join(pid_str).join("task");
        Ok(std::fs::read_dir(task_dir)?
            .map(|task| {
                let task = task?;
                let mut children_file = std::fs::File::open(task.path().join("children"))?;
                children_file.seek(std::io::SeekFrom::Start(0))?;
                let buf = arena.alloc_slice_fill_default(Self::BUF_CAPACITY);
                let buf_len = children_file.read(buf)?;
                if buf_len == Self::BUF_CAPACITY {
                    panic!(
                        "Buffer is not sufficient to read all children in {}",
                        task.file_name().display()
                    );
                }
                let buf = str::from_utf8(&buf[0..buf_len]).unwrap();
                Ok::<_, std::io::Error>(buf.split(' ').filter_map(|c| {
                    c.parse::<u32>()
                        .inspect_err(|e| {
                            if *e.kind() != IntErrorKind::Empty {
                                eprintln!("failed to parse child pid `{c}`: {e}")
                            }
                        })
                        .ok()
                }))
            })
            .filter_map(|r| r.inspect_err(|e| eprint!("{}", e)).ok())
            .flat_map(|f| f))
    }
    pub fn executable(&self) -> &str {
        self.cmdline
            .split_once(' ')
            .map(|s| s.0)
            .unwrap_or_default()
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
}
