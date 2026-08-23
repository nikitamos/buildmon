use std::cell::RefCell;
use std::io;
use std::ops::DerefMut;
use std::time::Duration;
use std::{
    error::Error,
    io::stdout,
    process::{Child, Command, Stdio},
};

use buildmon::ProcessError;
use buildmon::tree::{LazyRow, Tree, TreeNode};
use buildmon::{
    CompilationProcess,
    classifier::{ClassifierImpl, ProcessClassifier},
};
use bumpalo::Bump;
use bumpalo::collections::CollectIn;
use crossterm::event::{self, Event, KeyCode};
use ratatui::widgets::{self, Cell, Row};
use ratatui::{
    DefaultTerminal, Frame,
    prelude::*,
    symbols::border,
    widgets::{Block, Paragraph, Widget},
};

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

impl<'a> ProcessNode<'a> {
    pub fn from_pid_in(pid: u32, alloc: &'a AllocInfo) -> buildmon::Result<Self> {
        let proc = CompilationProcess::from_pid_in(pid, &alloc.procs)?;
        Ok(Self { proc, alloc })
    }
}

impl<'a> TreeNode for ProcessNode<'a> {
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>> {
        let class = ClassifierImpl.classify(&self.proc);
        LazyRow::new([
            Span::from(class.tool.short_repr().green().bold()),
            Span::from(self.proc.executable().to_str().unwrap_or_default()),
            Span::from(class.artifact.unwrap_or("UNKNOWN")),
            Span::from("TODO"),
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
                    alloc: &self.alloc,
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
}

impl App {
    pub fn new(child: WaitChild, alloc: AllocInfo) -> Result<Self, ProcessError> {
        Ok(Self {
            exit: false,
            child,
            alloc,
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
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(crossterm::event::KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => self.exit = true,
            _ => {}
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
            // Process probably died
            return;
        };
        let tree = Tree::new(root, vec![0, 11, 21, 36, 40, 44]);
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
