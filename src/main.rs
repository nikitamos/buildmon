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
use buildmon::{
    CompilationProcess,
    classifier::{ClassifierImpl, ProcessClassifier},
};
use bumpalo::Bump;
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

pub fn get_proc_row<'a>(proc: &'a CompilationProcess, _alloc: &AllocInfo) -> Row<'a> {
    let class = ClassifierImpl.classify(proc);
    Row::new([
        Cell::new(class.tool.short_repr().green().bold()),
        Cell::new(proc.executable().to_str().unwrap_or_default()),
        Cell::new(class.artifact.unwrap_or("UNKNOWN")),
        Cell::new("TODO"),
        Cell::new("TODO"),
    ])
}

fn retrieve_tree<'a>(
    depth: u32,
    root: &'a CompilationProcess,
    alloc: &'a AllocInfo,
    out: &mut Vec<Row<'a>>,
) {
    out.push(get_proc_row(root, alloc));
    eprintln!("adding proc!");
    let mut buf = String::new();
    let Ok(children) = root.children_pids(&alloc.arena, &mut buf) else {
        return;
    };

    for c in children.collect::<Vec<_>>() {
        if let Ok(p) = CompilationProcess::from_pid_in(c, &alloc.procs) {
            retrieve_tree(depth + 1, &p, alloc, out);
        }
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
struct App<'a> {
    exit: bool,
    child: WaitChild,
    alloc: &'a AllocInfo,
    root: &'a CompilationProcess<'a>,
}

impl<'a> App<'a> {
    pub fn new(child: WaitChild, alloc: &'a AllocInfo) -> Result<Self, ProcessError> {
        let root = CompilationProcess::from_pid_in(child.id(), &alloc.procs)?;
        Ok(Self {
            exit: false,
            child,
            alloc,
            root,
        })
    }
    pub fn run(&mut self, term: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            term.draw(|f| self.draw(f))?;
            self.handle_events()?;
        }
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

impl<'a> Widget for &App<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let mut rows = Vec::new();
        retrieve_tree(0, self.root, self.alloc, &mut rows);
        let tbl = widgets::Table::new(rows, [11, 10, 15, 4, 4]);
        Widget::render(tbl, area, buf);
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

    ratatui::run(move |terminal| App::new(c, &AllocInfo::new()).unwrap().run(terminal))
}
