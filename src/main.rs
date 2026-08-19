use std::{error::Error, io::Read, num::IntErrorKind, os::unix::raw::dev_t, path::Path};

use buildmon::CompilationProcess;
use bumpalo::Bump;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};

#[inline(always)]
fn print_depth(d: u32) {
    for _ in 0..d {
        print!(" ");
    }
}

fn print_tree(depth: u32, root: CompilationProcess) {
    print_depth(depth);
    println!("-{} ({})", root.executable(), root.pid());
    for c in root.children_pids(arena)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();
    let s = sysinfo::System::new_with_specifics(
        RefreshKind::default().with_processes(ProcessRefreshKind::everything()),
    );
    let pid = args.nth(1).expect("No PID given").parse()?;
    println!("Process {pid}");
    // let proc = s.process(pid).expect("No process with such PID");
    let cproc = CompilationProcess::from_pid(pid);
    let temp_pids = Bump::with_capacity(8192);

    println!("Children processes:");
    for child in cproc.children_pids(&temp_pids)? {
        println!("{child}");
    }
    Ok(())
}
