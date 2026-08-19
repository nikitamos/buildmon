use std::{
    error::Error,
    ffi::OsStr,
    io::{Write, stdout},
    ops::Sub,
    process::{Command, Stdio},
    thread::{self, sleep},
    time::{Duration, SystemTime},
};

use buildmon::{
    CompilationProcess,
    classifier::{ClassifierImpl, ProcessClassifier},
};
use bumpalo::Bump;
use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::MoveTo,
    style::{self, Print, PrintStyledContent, StyledContent, Stylize},
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

fn print_proc_info(stdout: &mut std::io::Stdout, proc: &CompilationProcess) {
    if let Ok(class) = ClassifierImpl.classify(proc) {
        let _ = stdout.queue(PrintStyledContent(class.tool.green().bold()));
    }
    print!(" ");
    let _ = stdout.queue(
        PrintStyledContent(
            proc.executable_basename()
                .as_deref()
                .map(|ex| ex.underline_cyan())
                .unwrap_or_else(|| proc.cmdline().underline_black())
        ),
    );
    println!(" {}", proc.pid());
}

fn print_tree(depth: u32, root: &CompilationProcess, alloc: &mut AllocInfo) {
    let mut stdout = stdout();
    print_depth(&mut stdout, depth);
    print_proc_info(&mut stdout, root);
    let Ok(children) = root.children_pids(&alloc.arena, &mut alloc.str_buf) else {
        return;
    };

    for c in children.collect::<Vec<_>>() {
        if let Ok(p) = CompilationProcess::from_pid(c) {
            print_tree(depth + 1, &p, alloc);
        } else {
            print_depth(&mut stdout, depth + 1);
            let _ = stdout.queue(style::PrintStyledContent("<died quietly>".dark_red()));
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
    let cproc = CompilationProcess::from_pid(pid)?;

    let mut alloc = AllocInfo::new();
    let mut stdout = stdout();
    stdout.execute(Clear(ClearType::All))?;
    while cproc.is_alive(&mut alloc.str_buf) {
        alloc.arena.reset();
        stdout
            .queue(Clear(ClearType::FromCursorUp))?
            .queue(MoveTo(0, 0))?;
        print_tree(0, &cproc, &mut alloc);
        stdout.flush()?;
        sleep(Duration::from_millis(20));
    }
    let status = c.wait().unwrap();
    println!("\nExited with status {:?}", status.code());
    Ok(())
}
