use core::fmt::NumBuffer;
use std::cell::RefCell;
use std::fmt::{Display, Write};
use std::io;
use std::ops::{Deref, DerefMut};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use buildmon::ProcessError;
use buildmon::proc::SystemInfo;
use buildmon::tree::{LazyRow, Tree, TreeNode};
use buildmon::{
    CompilationProcess,
    classifier::{ClassifierImpl, ProcessClassifier},
};
use bumpalo::Bump;
use crossterm::event::{self, Event, KeyCode};
use ratatui::widgets;
use ratatui::{DefaultTerminal, Frame, prelude::*, widgets::Widget};

#[derive(Debug)]
pub struct AllocInfo {
    pub arena: Bump,
    pub procs: Bump,
    pub str_buf: RefCell<String>,
}

impl AllocInfo {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            procs: Bump::new(),
            str_buf: RefCell::new(String::with_capacity(1024)),
        }
    }
    #[track_caller]
    pub fn str_buf(&self) -> impl DerefMut<Target = String> {
        self.str_buf.borrow_mut()
    }
}

impl Default for AllocInfo {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProcessNode<'a> {
    proc: &'a CompilationProcess<'a>,
    alloc: &'a AllocInfo,
}

#[derive(Copy, Clone, Debug)]
pub enum MemUnit {
    B(f64),
    KiB(f64),
    MiB(f64),
    GiB(f64),
    TiB(f64),
}

impl Display for MemUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (val, unit) = match self {
            MemUnit::B(b) => (b, "b"),
            MemUnit::KiB(k) => (k, "K"),
            MemUnit::MiB(m) => (m, "M"),
            MemUnit::GiB(g) => (g, "G"),
            MemUnit::TiB(t) => (t, "T"),
        };
        write!(f, "{val:.2}{unit}",)
    }
}

pub fn convert_mem_units(bytes: usize) -> MemUnit {
    use MemUnit::*;
    const KIB: usize = 1024;
    const MIB: usize = KIB * 1024;
    const GIB: usize = MIB * 1024;
    const TIB: usize = GIB * 1024;

    let b = bytes as f64;
    match bytes {
        0..KIB => B(b),
        KIB..MIB => KiB(b / KIB as f64),
        MIB..GIB => MiB(b / MIB as f64),
        GIB..TIB => GiB(b / GIB as f64),
        _ => TiB(b / TIB as f64),
    }
}

impl<'a> ProcessNode<'a> {
    pub fn from_pid_in(pid: u32, alloc: &'a AllocInfo) -> buildmon::Result<Self> {
        let proc = CompilationProcess::from_pid_in(pid, &alloc.procs)?;
        Ok(Self { proc, alloc })
    }
}

impl<'a> TreeNode for ProcessNode<'a> {
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>> {
        let class = ClassifierImpl.classify(self.proc);
        let buf = &mut self.alloc.str_buf();
        let mem = match self.proc.stat(&self.alloc.procs, buf) {
            Ok(stat) => {
                buf.clear();
                let _ = write!(buf, "{}", convert_mem_units(stat.rss_bytes()));
                self.alloc.procs.alloc_str(buf)
            }
            Err(_) => "ERR",
        };
        LazyRow::new([
            class.tool.short_repr().green().bold(),
            Span::from(self.proc.executable_basename().unwrap_or_default()),
            Span::from(class.artifact.unwrap_or("UNKNOWN")),
            Span::from(mem),
            Span::from("TODO"),
        ])
    }

    fn children(self) -> impl Iterator<Item = Self> {
        use bumpalo::collections::Vec;
        let mut stbuf = self.alloc.str_buf();
        let children = self
            .proc
            .children_pids(&self.alloc.arena, &mut stbuf)
            .map(|pids| Vec::from_iter_in(pids, &self.alloc.arena))
            .unwrap_or_else(|_| Vec::new_in(&self.alloc.arena));

        children.into_iter().filter_map(move |c| {
            CompilationProcess::from_pid_in(c, &self.alloc.procs)
                .ok()
                .map(|proc| Self {
                    alloc: self.alloc,
                    proc,
                })
        })
    }
}

#[derive(Debug)]
struct WaitChild(Child);

impl std::ops::DerefMut for WaitChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl std::ops::Deref for WaitChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for WaitChild {
    fn drop(&mut self) {
        _ = self.0.wait();
    }
}

#[derive(Debug)]
struct App {
    exit: bool,
    child: WaitChild,
    alloc: AllocInfo,
    info: &'static SystemInfo,
}

impl App {
    pub fn new(child: WaitChild, alloc: AllocInfo) -> Result<Self, ProcessError> {
        Ok(Self {
            exit: false,
            child,
            alloc,
            info: SystemInfo::get(),
        })
    }
    pub fn run(&mut self, term: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            self.alloc.procs.reset();
            self.alloc.arena.reset();
            term.draw(|f| self.draw(f))?;
            self.handle_events()?;
        }
        self.child.kill()?;
        Ok(())
    }

    pub fn draw(&self, frame: &mut Frame) {
        let tree_block = widgets::Block::bordered()
            .title("Compilation Processes".italic().blue())
            .border_set(symbols::border::LIGHT_DOUBLE_DASHED)
            .title_alignment(HorizontalAlignment::Left)
            .title_bottom(
                Line::from_iter(["<Q>".green().bold(), "uit".cyan()])
                    .alignment(HorizontalAlignment::Left),
            )
            .title_bottom(
                Line::from(format!("{} RAM", convert_mem_units(self.info.total_ram())))
                    .alignment(HorizontalAlignment::Right)
                    .red(),
            );

        let self_area = tree_block.inner(frame.area());
        frame.render_widget(tree_block, frame.area());
        frame.render_widget(self, self_area);
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        if let Event::Key(crossterm::event::KeyEvent {
            code: KeyCode::Char('q'),
            ..
        }) = event::read()?
        {
            self.exit = true
        }
        Ok(())
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let Ok(root) = ProcessNode::from_pid_in(self.child.id(), &self.alloc) else {
            panic!("died?");
        };
        let tree = Tree::new(root, vec![0, 11, 21, 36, 50, 60]).max_depth(16);
        tree.render(area, buf);
    }
}

fn main() -> Result<(), std::io::Error> {
    let mut args = std::env::args();
    let c = WaitChild(
        Command::new(args.nth(1).expect("Program to start expected"))
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    ratatui::run(move |terminal| App::new(c, AllocInfo::new()).unwrap().run(terminal))
}
