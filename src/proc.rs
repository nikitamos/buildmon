use bumpalo::Bump;
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    fmt::Write,
    fs::File,
    io::{BufRead, BufReader, Read},
    num::IntErrorKind,
    path::Path,
    sync::LazyLock,
    time::Instant,
};

use super::{ProcessError, Result};

#[derive(Debug, Clone)]
pub struct SystemInfo {
    page_size: usize,
    pages_avail: usize,
    clock_tick: u64,
    info: libc::sysinfo,
}

impl SystemInfo {
    pub fn get() -> &'static Self {
        &SYS_INFO
    }

    pub fn new() -> Self {
        unsafe {
            let mut info = std::mem::zeroed();
            if libc::sysinfo(&mut info) != 0 {
                panic!("sysinfo failed: {}", std::io::Error::last_os_error());
            }
            let page_size = libc::sysconf(libc::_SC_PAGESIZE)
                .try_into()
                .map_err(|_| std::io::Error::last_os_error())
                .expect("sysconf failed");
            let pages_avail = libc::sysconf(libc::_SC_AVPHYS_PAGES)
                .try_into()
                .map_err(|_| std::io::Error::last_os_error())
                .expect("sysconf failed");
            let clock_tick = libc::sysconf(libc::_SC_CLK_TCK)
                .try_into()
                .map_err(|_| std::io::Error::last_os_error())
                .expect("sysconf failed");
            SystemInfo {
                page_size,
                pages_avail,
                info,
                clock_tick,
            }
        }
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// Total amount of memory in bytes
    pub fn total_ram(&self) -> usize {
        self.info.mem_unit as usize * self.info.totalram as usize
    }
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self::new()
    }
}

static SYS_INFO: LazyLock<SystemInfo> = LazyLock::new(SystemInfo::new);

#[derive(Debug, Clone)]
pub struct ProcStat {
    /// Resident memory (in pages)
    mem: usize,
    /// The time the process started after boot, expressed in clock ticks
    start_time: u64,
}

impl ProcStat {
    /// Size of resident memory in bytes
    pub fn rss_bytes(&self) -> usize {
        self.mem * SYS_INFO.page_size
    }
    /// The time the process started after boot, in milliseconds
    pub fn start_time(&self) -> u64 {
        self.start_time * 1000 / SYS_INFO.clock_tick
    }
}

#[derive(Debug)]
pub struct CompilationProcess<'a> {
    cmdline: &'a str,
    pid: u32,
}

fn read_meminfo_like(path: impl AsRef<Path>, buf: &mut String) -> Result<HashMap<&str, &str>> {
    buf.clear();
    let mut f = File::open(path.as_ref())?;
    f.read_to_string(buf)?;
    Ok(buf
        .lines()
        .filter_map(|l| l.split_once(":"))
        .map(|(k, v)| (k, v.trim()))
        .collect())
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
        let Ok(h) = read_meminfo_like(format!("/proc/{}/status", self.pid), buf) else {
            return false;
        };
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
        const START_TIME: usize = 21;
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
            start_time: stat[START_TIME]
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

#[derive(Debug, Clone)]
pub struct SystemLoad {
    prev_load: u64,
    load_percent: u64,
    mem_avail: usize,
    // Time elapsed since system boot, in milliseconds
    uptime: u64,
    timestamp: Instant,
    buffer: String,
}

impl SystemLoad {
    const CPU_UPDATE_COOLDOWN_MS: u64 = 200;
    pub fn new() -> Self {
        let mut seed = Self {
            prev_load: 0,
            load_percent: 0,
            mem_avail: 0,
            uptime: 0,
            timestamp: Instant::now(),
            buffer: String::new(),
        };
        seed.update();
        seed
    }
    pub fn update(&mut self) {
        let meminfo = read_meminfo_like("/proc/meminfo", &mut self.buffer)
            .expect("failed to read /proc/meminfo");
        self.mem_avail = Self::parse_mem(meminfo["MemAvailable"]) * 1024;
        self.uptime = self.get_uptime();
        let load_ms = self.get_load_time() * 1000 / SYS_INFO.clock_tick;
        if load_ms != self.prev_load {
            let now = Instant::now();
            let passed = now.duration_since(self.timestamp).as_millis() as u64;
            if passed >= Self::CPU_UPDATE_COOLDOWN_MS && passed > 0 {
                self.load_percent = (load_ms - self.prev_load) * 100 / passed;
                self.prev_load = load_ms;
                self.timestamp = now;
            }
        }
    }
    pub fn load_percent(&self) -> u64 {
        self.load_percent
    }
    pub fn avail_ram(&self) -> usize {
        self.mem_avail
    }
    pub fn uptime(&self) -> u64 {
        self.uptime
    }
    fn parse_mem(s: &str) -> usize {
        s.strip_suffix(" kB")
            .expect("/proc/meminfo reported invalid value")
            .parse()
            .expect("/proc/meminfo reported invalid value")
    }
    fn get_load_time(&mut self) -> u64 {
        self.buffer.clear();
        let mut f = BufReader::new(File::open("/proc/stat").expect("failed to open /proc/stat"));
        f.read_line(&mut self.buffer)
            .expect("failed to read /proc/stat");

        self.buffer
            .split(' ')
            .filter(|s| !s.is_empty())
            .skip(1)
            .take(3) // utime, nice, stime,
            .flat_map(str::parse::<u64>)
            .sum()
    }
    /// Returns time elapsed since system boot, in milliseconds
    fn get_uptime(&mut self) -> u64 {
        self.buffer.clear();
        let mut f = File::open("/proc/uptime").expect("failed to open /proc/uptime");
        f.read_to_string(&mut self.buffer)
            .expect("failed to read /proc/stat");
        (
            self.buffer
                .split_once(' ')
                .unwrap()
                .0
                .parse::<f64>()
                .unwrap()
                * 1000.0
        ) as u64
    }
}

impl Default for SystemLoad {
    fn default() -> Self {
        Self::new()
    }
}
