#![allow(dead_code)]

use std::{
    collections::HashMap, ffi::OsStr, fmt::format, fs::File, io::Read, num::IntErrorKind,
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

#[derive(Debug)]
pub enum ProcessError {
    Dead,
    UnknownClass,
    Io(std::io::Error),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for ProcessError {
    fn from(value: std::io::Error) -> Self {
        ProcessError::Io(value)
    }
}

pub struct ProcessClassification {
    kind: ProcessKind,
}

impl ProcessClassification {
    pub const fn unknown() -> Self {
        todo!()
    }
}

pub type Result<T, E = ProcessError> = std::result::Result<T, E>;

pub trait ProcessClassifier {
    fn classify(&self, proc: &CompilationProcess) -> Result<ProcessClassification, ProcessError>;
}

#[derive(Debug)]
pub struct CompilationProcess {
    cmdline: String,
    pid: u32,
}

impl std::error::Error for ProcessError {}

impl CompilationProcess {
    const BUF_CAPACITY: usize = 512;

    pub fn from_pid(pid: u32) -> Result<Self> {
        let mut cmdline_file =
            File::open(&format!("/proc/{pid}/cmdline")).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ProcessError::Dead,
                _ => ProcessError::Io(e),
            })?;
        let mut cmdline = String::new();
        cmdline_file.read_to_string(&mut cmdline)?;
        Ok(Self { cmdline, pid })
    }

    pub fn is_alive(&self, buf: &mut String) -> bool {
        let Ok(mut f) = File::open(&format!("/proc/{}/status", self.pid)) else {
            eprintln!("no file?");
            return false;
        };
        if let Err(_) = f.read_to_string(buf) {
            return false;
        }
        let h: HashMap<&str, &str> = buf.lines().filter_map(|l| l.split_once(":\t")).collect();
        let state = h["State"].chars().next();
        state != Some('Z')
    }

    pub fn is_finished(&self, buf: &mut String) -> bool {
        !self.is_alive(buf)
    }
    // Is it actually better to have copies or a lot of heap allocations?
    // Large of the data stored in arenas literally gets copied.
    pub fn children_pids<'s, 'b>(
        &'s self,
        arena: &'b Bump,
        stbuf: &'b mut String,
    ) -> std::io::Result<impl Iterator<Item = u32> + use<'b>> {
        let pid_str = self.pid.to_string();
        let task_dir = Path::new("/proc").join(pid_str).join("task");
        Ok(std::fs::read_dir(task_dir)?
            .map(move |task| {
                let task = task?;
                let mut children_file = File::open(task.path().join("children"))?;
                stbuf.clear();
                children_file.read_to_string(stbuf)?;
                let buf = arena.alloc_str(stbuf);
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
            .flatten())
    }
    pub fn executable(&self) -> &Path {
        Path::new(
            self
                .cmdline
                .split_once('\0')
                .map(|s| s.0)
                .unwrap_or(&self.cmdline),
        )
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
}
