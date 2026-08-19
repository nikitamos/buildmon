use std::{error::Error, io::stdout, thread::sleep, time::Duration};

use buildmon::CompilationProcess;
use bumpalo::Bump;
use crossterm::{ExecutableCommand, cursor::MoveTo, terminal::{Clear, ClearType::{self, All}}};
use sysinfo::{ProcessRefreshKind, RefreshKind};

#[inline(always)]
fn print_depth(d: u32) {
    print!("{d}");
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

fn print_tree(depth: u32, root: &CompilationProcess, alloc: &mut AllocInfo) {
    if depth > 2 {
        return;
    }
    print_depth(depth);
    println!("-{} ({})", root.executable(), root.pid());
    let Ok(children) = root
        .children_pids(&alloc.arena, &mut alloc.str_buf) else {return;};

    for c in children.collect::<Vec<_>>() {
        if let Ok(p) = CompilationProcess::from_pid(c) {
            print_tree(depth + 1, &p, alloc);
        } else {
            print_depth(depth + 1);
            println!("<died of cringe>");
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();
    let s = sysinfo::System::new_with_specifics(
        RefreshKind::default().with_processes(ProcessRefreshKind::everything()),
    );
    let pid = args.nth(1).expect("No PID given").parse()?;
    println!("Process {pid}");
    // let proc = s.process(pid).expect("No process with such PID");
    let cproc = CompilationProcess::from_pid(pid)?;

    let mut alloc = AllocInfo::new();
    let mut stdout = stdout();
    stdout.execute(Clear(ClearType::All))?;
    loop {
        stdout.execute(MoveTo(0, 0))?;
        print_tree(0, &cproc, &mut alloc);
        sleep(Duration::from_millis(20));
        stdout.execute(Clear(ClearType::FromCursorUp))?;
    }
    Ok(())
}
