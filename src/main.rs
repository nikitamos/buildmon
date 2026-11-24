use core::fmt::NumBuffer;
use std::cell::{LazyCell, OnceCell, RefCell};
use std::default::Default;
use std::fmt::Write;
use std::io::{self};
use std::ops::DerefMut;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use buildmon::chart::{MemChartDataset, MemUsageByCategory};
use buildmon::classifier::{self};
use buildmon::component::{Component, ComponentState};
use buildmon::proc::{SystemInfo, SystemLoad};
use buildmon::std_streams::{ActiveStream, StdStreamsWidget, StdStreamsWidgetState};
use buildmon::tree::{EagerTreeNode, LazyRow, Tree, TreeNode, TreeState};
use buildmon::units::{MemUnit, Time};
use buildmon::{CompilationProcess, chart};
use buildmon::{ProcStat, ProcessError};
use bumpalo::Bump;
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyEventState,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::text::ToText;
use ratatui::widgets::{self, Axis, Block, Chart, Dataset, Paragraph, ScrollDirection, Scrollbar};
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
    stat: ProcStat,
    alloc: &'a AllocInfo,
    children: &'a [Option<ProcessNode<'a>>],
    uptime: u64,
    depth: usize,
    height: usize,
    children_count: usize,
}

impl<'a> ProcessNode<'a> {
    pub fn new(pid: u32, uptime: u64, alloc: &'a AllocInfo) -> buildmon::Result<Self> {
        // TODO: cache classification
        use bumpalo::collections::Vec;
        let proc = CompilationProcess::from_pid_in(pid, &alloc.procs)?;
        let children = Vec::from_iter_in(
            proc.children_pids(&alloc.arena, &mut alloc.str_buf())?,
            &alloc.arena,
        );
        let children = alloc.procs.alloc_slice_fill_with(children.len(), |i| {
            ProcessNode::new(children[i], uptime, alloc).ok()
        });
        let (depth, height, count) = children.iter().fold((0, 0, 0), |(d, h, count), c| {
            if let Some(c) = c {
                (d.max(c.depth), h + c.height, count + 1)
            } else {
                (d, h, count)
            }
        });
        let stat = proc.stat(&alloc.arena, &mut alloc.str_buf())?;
        Ok(Self {
            proc,
            alloc,
            children,
            uptime,
            stat,
            depth: depth + 1, // TODO: is 1 here necessary?
            height: height + 1,
            children_count: count,
        })
    }
    pub fn get_mem_usage(&self) -> MemUsageByCategory {
        self.children
            .iter()
            .flatten()
            .map(ProcessNode::get_mem_usage)
            .sum::<MemUsageByCategory>()
            + MemUsageByCategory::of_tool(
                self.stat.rss_bytes(),
                &classifier::classify_tool(self.proc).tool,
            )
    }
}

impl<'a> TreeNode for ProcessNode<'a> {
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>> {
        let class = classifier::classify_tool(self.proc);
        let buf = &mut self.alloc.str_buf();
        let (mem, time) = {
            buf.clear();
            let _ = write!(buf, "{}", MemUnit::from_bytes(self.stat.rss_bytes()));
            let mem = self.alloc.procs.alloc_str(buf);
            buf.clear();
            let _ = write!(
                buf,
                "{}",
                Time::from_ms(self.uptime.saturating_sub(self.stat.start_time()))
            );
            let time = self.alloc.procs.alloc_str(buf);
            (mem as &str, time as &str)
        };
        LazyRow::new([
            class.tool.short_repr().green().bold(),
            Span::from(self.proc.executable_basename().unwrap_or_default()),
            Span::from(class.artifact.unwrap_or("")),
            Span::from(mem),
            Span::from(time),
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

#[derive(Debug, Default)]
struct UpdateData {
    mem_usage: Option<MemUsageByCategory>,
    tree_state: ComponentState<TreeState>,
}

// #[derive(Debug)]
struct App<'a> {
    exit: bool,
    child: WaitChild,
    alloc: AllocInfo,
    info: &'static SystemInfo,
    load: SystemLoad,
    chart_dataset: Box<MemChartDataset>,
    output: StdStreamsWidget,
    output_state: ComponentState<StdStreamsWidgetState>,
    tree_state: ComponentState<TreeState>,
    finish_state: Option<ExitStatus>,
    child_cmdline: &'a str,
}

const VERSION_HEADER: &str = concat!(env!("CARGO_BIN_NAME"), " v", env!("CARGO_PKG_VERSION"));
const MIN_WIDTH: u16 = 123;
const MIN_HEIGHT: u16 = 12;

impl<'a> App<'a> {
    pub fn new(
        mut child: WaitChild,
        alloc: AllocInfo,
        child_cmdline: &'a str,
    ) -> Result<Self, ProcessError> {
        Ok(Self {
            output: StdStreamsWidget::new(&mut child),
            output_state: Default::default(),
            exit: false,
            child,
            alloc,
            chart_dataset: MemChartDataset::new_boxed(),
            info: SystemInfo::get(),
            load: SystemLoad::new(),
            tree_state: ComponentState::<TreeState>::default().focus(true),
            finish_state: None,
            child_cmdline,
        })
    }

    pub fn run(&mut self, term: &mut DefaultTerminal) -> io::Result<ExitCode> {
        term.backend_mut().execute(EnableMouseCapture)?;
        let mut updates = UpdateData::default();
        while !self.exit {
            self.alloc.procs.reset();
            self.alloc.arena.reset();
            self.handle_events()?;
            if Self::sufficient_size(MIN_WIDTH, MIN_HEIGHT, term) {
                term.draw(|f| updates = self.draw(f))?;
                self.update(&mut updates);
            } else {
                term.draw(|f| self.draw_insufficient_size(f))?;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let exit_code = if let Some(status) = self.finish_state {
            status
                .code()
                .map(|x| ExitCode::from((x & 0xFF) as u32 as u8))
                .unwrap_or(ExitCode::FAILURE)
        } else {
            let _ = self
                .kill_child()
                .inspect_err(|e| eprintln!("failed to kill child: {e}"));
            ExitCode::FAILURE
        };
        term.backend_mut().execute(DisableMouseCapture)?;
        Ok(exit_code)
    }

    fn draw_insufficient_size(&mut self, frame: &mut Frame) {
        static MESSAGE: LazyLock<String> =
            LazyLock::new(|| format!("Terminal of size {MIN_WIDTH}x{MIN_HEIGHT} is required"));
        frame.render_widget((&*MESSAGE).to_text().centered(), frame.area());
    }

    fn sufficient_size(required_width: u16, required_height: u16, term: &DefaultTerminal) -> bool {
        term.size()
            .map(|Size { width, height }| width >= required_width && height >= required_height)
            .unwrap_or(false)
    }

    fn kill_child(&mut self) -> std::io::Result<()> {
        let code = unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        if code != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) -> UpdateData {
        let root_block = widgets::Block::bordered()
            .title(
                VERSION_HEADER
                    .gray()
                    .bold()
                    .italic()
                    .into_right_aligned_line(),
            )
            .title(self.child_cmdline.bold().into_left_aligned_line())
            .border_set(symbols::border::DOUBLE)
            .title_bottom(
                Line::from_iter(["Q".green().bold(), "uit".cyan()])
                    .alignment(HorizontalAlignment::Left),
            )
            .title_bottom(self.mem_consumption())
            .title_bottom(self.cpu_load());

        let [v_area, chart_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Fill(3),
                Constraint::Length(chart::MAX_POINTS as u16 + 10),
            ])
            .areas(root_block.inner(frame.area()));

        let [tree_area, out_block_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(v_area);

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
            self.output_state.vertical_scrollbar(),
        );

        let mut update_data = Default::default();
        if let Some(ref status) = self.finish_state {
            self.render_finish_message(frame, tree_area, status);
        } else {
            let Some((root, tree_state)) = self.render_tree(tree_area, frame) else {
                return UpdateData::default();
            };
            update_data = UpdateData {
                mem_usage: Some(root.get_mem_usage()),
                tree_state,
            };
        }
        self.render_chart(frame, chart_area);
        update_data
    }

    fn render_finish_message(&self, frame: &mut Frame<'_>, tree_area: Rect, status: &ExitStatus) {
        let mut line = Line::default();
        let mut buf = NumBuffer::new();
        if let Some(code) = status.code() {
            line.push_span("Process finished with exit code ".italic());
            let code = code.format_into(&mut buf).italic();
            if status.success() {
                line.push_span(code.green().italic().underlined());
            } else {
                line.push_span(code.red().italic().underlined());
            }
        } else if let Some(signal) = status.signal() {
            line.push_span("Process terminated by signal ".italic());
            line.push_span(signal.format_into(&mut buf).yellow().italic().underlined());
        }

        frame.render_widget(
            Paragraph::new(Text::from(line).alignment(HorizontalAlignment::Center))
                .block(Block::bordered().border_set(Self::border_set(self.tree_state.focus))),
            tree_area,
        );
    }

    fn render_chart(&self, frame: &mut Frame, area: Rect) {
        let x_axis = Axis::default().bounds(self.chart_dataset.x_bounds());
        let y_axis = Axis::default()
            .title("RAM")
            .labels(["0%", "50%", "100%"])
            .bounds(self.chart_dataset.y_bounds());

        let compile = Dataset::default()
            .graph_type(widgets::GraphType::Line)
            .marker(symbols::Marker::Braille)
            .name("Compile")
            .data(self.chart_dataset.compile())
            .red();
        let link = Dataset::default()
            .graph_type(widgets::GraphType::Line)
            .marker(symbols::Marker::Braille)
            .name("Link")
            .data(self.chart_dataset.link())
            .green();
        let build = Dataset::default()
            .graph_type(widgets::GraphType::Line)
            .marker(symbols::Marker::Braille)
            .name("Build")
            .data(self.chart_dataset.build())
            .blue();
        let other = Dataset::default()
            .graph_type(widgets::GraphType::Line)
            .marker(symbols::Marker::Braille)
            .name("Others")
            .data(self.chart_dataset.other())
            .gray();
        frame.render_widget(
            Chart::new(vec![compile, link, build, other])
                .block(Block::bordered().border_type(widgets::BorderType::LightDoubleDashed))
                .x_axis(x_axis)
                .y_axis(y_axis),
            area,
        );
    }

    fn mem_consumption(&mut self) -> Line<'static> {
        let used_ram = self.info.total_ram() - self.load.avail_ram();

        let color = match used_ram * 100 / self.info.total_ram() {
            ..50 => Color::Green,
            80.. => Color::Red,
            _ => Color::Yellow,
        };

        Line::from(format!(
            "{}/{} RAM",
            MemUnit::from_bytes(used_ram),
            MemUnit::from_bytes(self.info.total_ram())
        ))
        .alignment(HorizontalAlignment::Right)
        .fg(color)
    }
    fn cpu_load(&mut self) -> Line<'static> {
        Line::from(format!("{}% CPU", self.load.load_percent()))
            .alignment(HorizontalAlignment::Right)
            .fg(Color::Magenta)
    }

    pub fn update(&mut self, updates: &mut UpdateData) {
        let (e1, e2) = self.output.update();
        if let Err(e) = e1 {
            eprintln!("Failed to read stdout: {e}");
        }
        if let Err(e) = e2 {
            eprintln!("Failed to read stderr: {e}");
        }

        self.load.update();
        if let Some(usage) = updates.mem_usage.take() {
            self.chart_dataset
                .add_point(usage, self.load.uptime() as f64);
        }
        self.tree_state = updates.tree_state.clone();

        if let Ok(status) = self.child.try_wait() {
            self.finish_state = status;
        }
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(ke) => self.handle_key_event(ke.code, ke.modifiers, ke.kind, ke.state)?,
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
    fn handle_key_event(
        &mut self,
        code: KeyCode,
        _modifiers: KeyModifiers,
        _kind: KeyEventKind,
        _state: KeyEventState,
    ) -> io::Result<()> {
        match code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Char('s') => self.output_state.switch(),
            KeyCode::Tab => self.focus_next(),
            KeyCode::BackTab => self.focus_prev(),

            KeyCode::Up | KeyCode::Char('k') => self.scroll_current(ScrollDirection::Backward),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_current(ScrollDirection::Forward),
            _ => (),
        }
        Ok(())
    }

    fn scroll_current(&mut self, direction: ScrollDirection) {
        if self.tree_state.focus {
            self.tree_state.scroll.scroll(direction);
        }
        if self.output_state.focus {
            self.output_state.vertical_scroll(direction, 1);
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

    fn render_tree<'s>(
        &'s self,
        area: Rect,
        frame: &mut Frame,
    ) -> Option<(ProcessNode<'s>, ComponentState<TreeState>)> {
        let root = ProcessNode::new(self.child.id(), self.load.uptime(), &self.alloc).ok()?;
        let tree = Tree::new(root, vec![5, 15, 20, 8, 10])
            .max_depth(16)
            .block(
                widgets::Block::bordered()
                    .border_set(Self::border_set(self.tree_state.focus))
                    .title("Processes".blue().underlined()),
            )
            .scrollbar(
                Scrollbar::new(widgets::ScrollbarOrientation::VerticalRight)
                    .track_symbol(Some(symbols::border::THICK.vertical_right)),
            );
        let mut state = self.tree_state.clone();
        frame.render_stateful_widget(Component(&tree), area, &mut state);
        Some((tree.take_root(), state))
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

fn main() -> Result<ExitCode, std::io::Error> {
    let args: Vec<_> = std::env::args().collect();

    match args.get(1).map_or("--help", |x| x.as_str()) {
        "--license" => {
            println!("{VERSION_HEADER}");
            for _ in 0..VERSION_HEADER.len() {
                print!("=");
            }
            println!("\n{}\n", include_str!("../LICENSE"),);
            return Ok(ExitCode::SUCCESS);
        }
        "--help" => {
            println!("{VERSION_HEADER} - {}", env!("CARGO_PKG_DESCRIPTION"));
            println!("Print this message            >> {} --help", args[0]);
            println!("Print license                 >> {} --license", args[0]);
            println!("Run command under the monitor >> {} <command>", args[0]);
            println!(
                " Example                      >> {} ninja -C build -j6",
                args[0]
            );

            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

    let c = WaitChild(
        Command::new(args.get(1).expect("Program to start expected"))
            .args(args.get(2..).unwrap_or_default())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
    );

    ratatui::run(move |terminal| {
        (App::new(c, AllocInfo::new(), &args[1..].join(" ")).unwrap()).run(terminal)
    })
}
