//! Widget for displaying piped stdout/stderr of child process

use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStderr, ChildStdout};

use nonblock::NonBlockingReader;
use ratatui::prelude::*;
use ratatui::text::ToText;
use ratatui::widgets::{Paragraph, ScrollDirection, ScrollbarState, Wrap};

// #[derive(Debug)]
pub struct StdStreamsWidget {
    stdout: Option<NonBlockingReader<ChildStdout>>,
    stderr: Option<NonBlockingReader<ChildStderr>>,
    stdout_buf: String,
    stderr_buf: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum ActiveStream {
    #[default]
    Stderr,
    Stdout,
}

#[derive(Debug, Clone, Default)]
struct StreamState {
    vscroll: ScrollbarState,
    offset_x: usize,
}

impl StreamState {
    /// Scrolls content upwards, i.e. reduces number of the first line shown
    pub fn scroll_upwards(&mut self, count: usize) {
        self.set_vscroll_pos(self.vscroll.get_position().saturating_sub(count));
    }
    /// Scrolls content downwards, i.e. increases the number of the first line shown
    pub fn scroll_downwards(&mut self, count: usize) {
        self.set_vscroll_pos(self.vscroll.get_position().saturating_add(count));
    }
    fn set_vscroll_pos(&mut self, pos: usize) {
        self.vscroll = self.vscroll.position(pos)
    }
    pub fn set_height(&mut self, height: usize) {
        self.vscroll = self.vscroll.content_length(height);
    }
    pub fn offset_x(&self) -> usize {
        self.offset_x
    }
    pub fn offset_y(&self) -> usize {
        self.vscroll.get_position()
    }
}

#[derive(Debug, Clone, Default)]
pub struct StdStreamsWidgetState {
    active: ActiveStream,
    stdout: StreamState,
    stderr: StreamState,
}

impl StdStreamsWidgetState {
    fn current(&self) -> &StreamState {
        match self.active {
            ActiveStream::Stderr => &self.stderr,
            ActiveStream::Stdout => &self.stdout,
        }
    }
    fn current_mut(&mut self) -> &mut StreamState {
        match self.active {
            ActiveStream::Stderr => &mut self.stderr,
            ActiveStream::Stdout => &mut self.stdout,
        }
    }
    pub fn vertical_scrollbar(&mut self) -> &mut ScrollbarState {
        &mut self.current_mut().vscroll
    }
    pub fn active(&self) -> ActiveStream {
        self.active
    }
    pub fn switch(&mut self) {
        self.active = match self.active() {
            ActiveStream::Stderr => ActiveStream::Stdout,
            ActiveStream::Stdout => ActiveStream::Stderr,
        }
    }
    pub fn vertical_scroll(&mut self, direction: ScrollDirection, count: usize) {
        match direction {
            ScrollDirection::Forward => self.current_mut().scroll_downwards(count),
            ScrollDirection::Backward => self.current_mut().scroll_upwards(count),
        }
    }
}

impl StdStreamsWidget {
    pub fn new(child: &mut Child) -> Self {
        let stdout = child.stdout.take().map(Self::make_nonblock);
        let stderr = child.stderr.take().map(Self::make_nonblock);
        Self {
            stdout,
            stderr,
            stdout_buf: String::new(),
            stderr_buf: String::new(),
        }
    }

    fn make_nonblock<R: Read + AsRawFd>(r: R) -> NonBlockingReader<R> {
        match NonBlockingReader::from_fd(r) {
            Ok(r) => r,
            Err(e) => panic!("Failed to create non-blocking reader: {e}"),
        }
    }
    /// Tries to read any pending content from stdout and stderr.
    /// First element of the returned tuple represents errors occurred
    /// when reading stdout, the second represents errors when reading stderr
    pub fn update(&mut self) -> (std::io::Result<()>, std::io::Result<()>) {
        (
            Self::read(&mut self.stdout_buf, self.stdout.as_mut()),
            Self::read(&mut self.stderr_buf, self.stderr.as_mut()),
        )
    }
    #[inline(always)]
    fn read(
        buf: &mut String,
        r: Option<&mut NonBlockingReader<impl Read + AsRawFd>>,
    ) -> std::io::Result<()> {
        if let Some(r) = r {
            r.read_available_to_string(buf)?;
            // FIXME: sanitize control sequences in buf.
        }
        Ok(())
    }
}

impl Widget for &StdStreamsWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let mut state = StdStreamsWidgetState::default();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl StatefulWidget for &StdStreamsWidget {
    type State = StdStreamsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let buffer = match state.active {
            ActiveStream::Stderr => self.stderr_buf.as_str(),
            ActiveStream::Stdout => self.stdout_buf.as_str(),
        };
        let cur_state = state.current_mut();

        cur_state.set_height(buffer.lines().count());
        // What to do if number of lines doesn't fit in u16?
        let par = Paragraph::new(buf_to_text(buffer))
            .scroll((cur_state.offset_y() as u16, cur_state.offset_x() as u16))
            .wrap(Wrap { trim: false });
        par.render(area, buf);
    }
}

fn buf_to_text<'a>(buf: &'a str) -> Text<'a> {
    if buf.is_empty() {
        "stream is empty".to_text().centered().dark_gray().italic()
    } else {
        Text::raw(buf).dark_gray()
    }
}
