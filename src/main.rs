use std::{
    error::Error, ffi::OsStr, io::{Write, stdout}, ops::Sub, process::{Command, Stdio}, thread::{self, sleep}, time::{Duration, SystemTime},
};

use buildmon::CompilationProcess;
use bumpalo::Bump;
use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::MoveTo,
    style::{self, Print, PrintStyledContent, Stylize},
    terminal::{
        Clear,
        ClearType::{self},
    },
};

#[inline(always)]
fn print_depth(stdout: &mut std::io::Stdout, d: u32) {
    for _ in 0..d {
        print!(" ");
    }
}

pub struct AllocInfo {
    pub arena: Bump,
    pub str_buf: String,
}

impl AllocInfo {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            str_buf: String::with_capacity(2048),
        }
    }
}

#[allow(unused_must_use)]
fn print_tree(depth: u32, root: &CompilationProcess, alloc: &mut AllocInfo) {
    let mut stdout = stdout();
    print_depth(&mut stdout, depth);
    println!(
        "-{} ({})",
        root.executable().file_name().unwrap_or(OsStr::new("")).display(),
        root.pid()
    );
    let Ok(children) = root.children_pids(&alloc.arena, &mut alloc.str_buf) else {
        return;
    };

    for c in children.collect::<Vec<_>>() {
        if let Ok(p) = CompilationProcess::from_pid(c) {
            print_tree(depth + 1, &p, alloc);
        } else {
            print_depth(&mut stdout, depth + 1);
            stdout.queue(style::PrintStyledContent("<died of cringe>".dark_red()));
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();
    let mut c = Command::new(args.nth(1).expect("Program to start expected"))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let pid = c.id();
    // let status = thread::spawn(move || c.wait());
    println!("Process {pid}");
    // let proc = s.process(pid).expect("No process with such PID");
    let cproc = CompilationProcess::from_pid(pid)?;

    let mut alloc = AllocInfo::new();
    let mut stdout = stdout();
    stdout.execute(Clear(ClearType::All))?;
    while cproc.is_alive(&mut alloc.str_buf) {
        stdout
            .queue(Clear(ClearType::FromCursorUp))?
            .queue(MoveTo(0, 0))?;
        print_tree(0, &cproc, &mut alloc);
        stdout.flush()?;
        sleep(Duration::from_millis(20));
    }
    println!("Exited with status {:?}", c.wait().unwrap().code());
    Ok(())
}
