use bumpalo::{Bump, collections::CollectIn};
use libc::perror;
use std::{
    borrow::Cow,
    cell::{LazyCell, OnceCell},
    collections::HashMap,
    ffi::{CStr, OsStr},
    fmt::Write,
    fs::File,
    io::Read,
    num::IntErrorKind,
    path::Path,
    sync::{LazyLock, OnceLock},
};

use super::{ProcessError, Result};

static PAGE_SIZE: LazyLock<usize> = LazyLock::new(|| unsafe {
    libc::sysconf(libc::_SC_PAGESIZE)
        .try_into()
        .map_err(|_| std::io::Error::last_os_error())
        .expect("sysconf failed")
});

#[derive(Debug, Clone)]
pub struct ProcStat {
    /// Resident memory (in pages)
    mem: usize,
}

impl ProcStat {
    /// Size of resident memory in bytes
    pub fn rss_bytes(&self) -> usize {
        self.mem * *PAGE_SIZE
    }
}

#[derive(Debug)]
pub struct CompilationProcess<'a> {
    cmdline: &'a str,
    pid: u32,
}

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

    pub fn stat(&self, alloc: &Bump, stbuf: &mut String) -> Result<ProcStat> {
        use bumpalo::collections::Vec;
        const UTIME: usize = 13;
        const STIMR: usize = 14;
        const RSS: usize = 23;

        stbuf.clear();
        write!(stbuf, "/proc/{}/stat", self.pid()).unwrap();
        let mut f = File::open(&stbuf)?;
        f.read_to_string(stbuf)?;
        let stat = Vec::from_iter_in(stbuf.split(' '), alloc);

        Ok(ProcStat {
            mem: stat[RSS]
                .parse()
                .expect("/proc/<PID>/stat reported invalid number"),
        })
    }

    /// Full path to the executable
    pub fn executable(&self) -> &Path {
        Path::new(
            self.cmdline
                .split_once('\0')
                .map(|s| s.0)
                .unwrap_or(self.cmdline),
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
        self.cmdline
    }
}
