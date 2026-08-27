use std::cell::RefCell;
use std::fmt::{Display, Write};
use std::io::{self};
use std::ops::DerefMut;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use buildmon::ProcessError;
use buildmon::component::{Component, ComponentState};
use buildmon::proc::SystemInfo;
use buildmon::std_streams::{ActiveStream, StdStreamsWidget, StdStreamsWidgetState};
use buildmon::tree::{EagerTreeNode, LazyRow, Tree, TreeNode, TreeState};
use buildmon::{
    CompilationProcess,
    classifier::{ClassifierImpl, ProcessClassifier},
};
use bumpalo::Bump;
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEvent, MouseEventKind,
};
use ratatui::widgets::{self, Scrollbar};
use ratatui::{DefaultTerminal, Frame, prelude::*};

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

// TODO: consider migrating usize to u16
#[derive(Debug, Clone)]
pub struct ProcessNode<'a> {
    proc: &'a CompilationProcess<'a>,
    alloc: &'a AllocInfo,
    children: &'a [Option<ProcessNode<'a>>],
    depth: usize,
    height: usize,
    children_count: usize,
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
        use bumpalo::collections::Vec;
        let proc = CompilationProcess::from_pid_in(pid, &alloc.procs)?;
        let children = Vec::from_iter_in(
            proc.children_pids(&alloc.arena, &mut alloc.str_buf())?,
            &alloc.arena,
        );
        let children = alloc.procs.alloc_slice_fill_with(children.len(), |i| {
            ProcessNode::from_pid_in(children[i] as u32, alloc).ok()
        });
        let (depth, height, count) = children.iter().fold((0, 0, 0), |(d, h, count), c| {
            if let Some(c) = c {
                (d.max(c.depth), h + c.height, count + 1)
            } else {
                (d, h, count)
            }
        });
        Ok(Self {
            proc,
            alloc,
            children,
            depth: depth + 1, // TODO: is 1 here necessary?
            height: height + 1,
            children_count: count,
        })
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
        self.children.iter().flatten().cloned()
    }
}

impl<'a> EagerTreeNode for ProcessNode<'a> {
    fn subtree_height(&self) -> usize {
        self.height
    }

    fn subtree_depth(&self) -> u16 {
        self.depth as u16
    }

    fn children(&self) -> impl Iterator<Item = &Self> {
        self.children.iter().flatten()
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

// #[derive(Debug)]
struct App {
    exit: bool,
    child: WaitChild,
    alloc: AllocInfo,
    info: &'static SystemInfo,
    output: StdStreamsWidget,
    output_state: ComponentState<StdStreamsWidgetState>,
    tree_state: ComponentState<TreeState>,
}

impl App {
    pub fn new(mut child: WaitChild, alloc: AllocInfo) -> Result<Self, ProcessError> {
        Ok(Self {
            output: StdStreamsWidget::new(&mut child),
            output_state: Default::default(),
            exit: false,
            child,
            alloc,
            info: SystemInfo::get(),
            tree_state: ComponentState::<TreeState>::default().focus(true),
        })
    }

    pub fn run(&mut self, term: &mut DefaultTerminal) -> io::Result<()> {
        term.backend_mut().execute(EnableMouseCapture)?;
        while !self.exit {
            self.alloc.procs.reset();
            self.alloc.arena.reset();
            self.handle_events()?;
            self.update();
            term.draw(|f| self.draw(f))?;
        }
        self.child.kill()?;
        term.backend_mut().execute(DisableMouseCapture)?;
        Ok(())
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let root_block = widgets::Block::bordered()
            .title("Build status".italic().yellow().bold())
            .border_set(symbols::border::DOUBLE)
            .title_alignment(HorizontalAlignment::Left)
            .title_bottom(
                Line::from_iter(["Q".green().bold(), "uit".cyan()])
                    .alignment(HorizontalAlignment::Left),
            )
            .title_bottom(
                Line::from(format!("{} RAM", convert_mem_units(self.info.total_ram())))
                    .alignment(HorizontalAlignment::Right)
                    .red(),
            );

        let [tree_area, out_block_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(root_block.inner(frame.area()));

        let out_block = widgets::Block::bordered()
            .title(self.output_title())
            .border_set(Self::border_set(self.output_state.focus))
            .title_bottom(Line::from_iter(["S".green().bold(), "witch".cyan()]));

        let output_scrollbar = Scrollbar::new(widgets::ScrollbarOrientation::VerticalRight)
            .track_symbol(Some(symbols::border::THICK.vertical_right));

        let output_area = out_block.inner(out_block_area);
        frame.render_widget(root_block, frame.area());
        frame.render_widget(out_block, out_block_area);
        frame.render_stateful_widget(Component(&self.output), output_area, &mut self.output_state);
        frame.render_stateful_widget(
            output_scrollbar,
            out_block_area.inner(Margin::new(0, 1)),
            &mut self.output_state.vertical_scrollbar(),
        );
        self.render_tree(tree_area, frame);
    }

    pub fn update(&mut self) {
        let (e1, e2) = self.output.update();
        if let Err(e) = e1 {
            eprintln!("Failed to read stdout: {e}");
        }
        if let Err(e) = e2 {
            eprintln!("Failed to read stderr: {e}");
        }
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => self.exit = true,
            Event::Key(KeyEvent {
                code: KeyCode::Char('s'),
                ..
            }) => {
                self.output_state.switch();
            }
            Event::Key(KeyEvent {
                code: KeyCode::Tab, ..
            }) => self.focus_next(),
            Event::Key(KeyEvent {
                code: KeyCode::BackTab,
                ..
            }) => self.focus_prev(),
            Event::Mouse(MouseEvent {
                column,
                row,
                kind,
                modifiers,
            }) => {
                self.handle_mouse_event(Position::new(column, row), kind, modifiers);
            }
            _ => (),
        }
        Ok(())
    }

    fn handle_mouse_event(
        &mut self,
        pos: Position,
        kind: MouseEventKind,
        _modifiers: KeyModifiers,
    ) {
        let Some(scroll_state) = self.current_scrollbar(pos) else {
            return;
        };
        match kind {
            MouseEventKind::ScrollDown => {
                scroll_state.next();
            }
            MouseEventKind::ScrollUp => {
                scroll_state.prev();
            }
            _ => (),
        }
    }

    fn current_scrollbar(&mut self, pos: Position) -> Option<&mut widgets::ScrollbarState> {
        if self.output_state.area.contains(pos) {
            Some(self.output_state.vertical_scrollbar())
        } else if self.tree_state.area.contains(pos) {
            Some(&mut self.tree_state.scroll)
        } else {
            None
        }
    }

    fn output_title(&self) -> Span<'_> {
        match self.output_state.active() {
            ActiveStream::Stderr => "stderr".red().underlined(),
            ActiveStream::Stdout => "stdout".green().underlined(),
        }
    }

    fn render_tree(&mut self, area: Rect, frame: &mut Frame) {
        let Ok(root) = ProcessNode::from_pid_in(self.child.id(), &self.alloc) else {
            return;
        };
        let tree = Tree::new(root, vec![5, 15, 20, 8, 5]).max_depth(16).block(
            widgets::Block::bordered()
                .border_set(Self::border_set(self.tree_state.focus))
                .title("Processes".blue().underlined()),
        );
        frame.render_stateful_widget(Component(&tree), area, &mut self.tree_state);
    }
    fn focus_next(&mut self) {
        if self.tree_state.focus {
            self.tree_state.focus = false;
            self.output_state.focus = true;
        } else if self.output_state.focus {
            self.tree_state.focus = true;
            self.output_state.focus = false;
        }
    }
    fn focus_prev(&mut self) {
        if self.tree_state.focus {
            self.tree_state.focus = true;
            self.output_state.focus = false;
        } else if self.output_state.focus {
            self.tree_state.focus = false;
            self.output_state.focus = true;
        }
    }
    fn border_set(focused: bool) -> symbols::border::Set<'static> {
        if focused {
            symbols::border::THICK
        } else {
            symbols::border::HEAVY_TRIPLE_DASHED
        }
    }
}

fn main() -> Result<(), std::io::Error> {
    let mut args = std::env::args();
    let c = WaitChild(
        Command::new(args.nth(1).expect("Program to start expected"))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    ratatui::run(move |terminal| App::new(c, AllocInfo::new()).unwrap().run(terminal))
}
