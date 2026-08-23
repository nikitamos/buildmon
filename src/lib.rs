#![feature(trait_alias)]
#![allow(dead_code)]

use std::{
    borrow::Cow, collections::HashMap, ffi::OsStr, fs::File, io::Read, num::IntErrorKind,
    path::Path,
};

use bumpalo::Bump;

mod bump_ext;

#[inline(always)]
pub(crate) fn minmax<T: Ord>(a: T, b: T) -> (T, T) {
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => (b, a),
        _ => (a, b),
    }
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

pub type Result<T, E = ProcessError> = std::result::Result<T, E>;

pub mod classifier;
pub mod tree;

#[derive(Debug)]
pub struct CompilationProcess<'a> {
    cmdline: &'a str,
    pid: u32,
}

impl std::error::Error for ProcessError {}

impl<'a> CompilationProcess<'a> {
    const BUF_CAPACITY: usize = 512;
    pub fn from_pid_in(pid: u32, arena: &'a Bump) -> Result<&'a Self> {
        let mut cmdline = String::new();
        let mut cmdline_file =
            File::open(format!("/proc/{pid}/cmdline")).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ProcessError::Dead,
                _ => ProcessError::Io(e),
            })?;
        // FIXME: If `CompilationProcess` for child process is created right after
        // the child spawned, for some reason it fails to read the cmdline at the
        // first attempt, so we try several times as a workaround
        for _ in 0..200 {
            cmdline_file.read_to_string(&mut cmdline)?;
            if !cmdline.is_empty() {
                break;
            }
        }
        let cmdline = arena.alloc_str(&cmdline);
        Ok(arena.alloc(Self { cmdline, pid }))
    }

    pub fn is_alive(&self, buf: &mut String) -> bool {
        let Ok(mut f) = File::open(format!("/proc/{}/status", self.pid)) else {
            return false;
        };
        if f.read_to_string(buf).is_err() {
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
    pub fn children_pids<'b>(
        &self,
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
    /// Full path to the executable
    pub fn executable(&self) -> &Path {
        Path::new(
            self.cmdline
                .split_once('\0')
                .map(|s| s.0)
                .unwrap_or(&self.cmdline),
        )
    }

    pub fn args(&self) -> impl Iterator<Item = &str> {
        self.cmdline().split('\0')
    }

    /// Filename of the executable
    pub fn executable_basename(&self) -> Option<Cow<'_, str>> {
        self.executable().file_name().map(OsStr::to_string_lossy)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn cmdline(&self) -> &str {
        &self.cmdline
    }
}
