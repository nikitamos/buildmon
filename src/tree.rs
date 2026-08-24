//! Widget for displaying process tree.
use ratatui::{prelude::*, style::Style, symbols::line, text::Span, widgets::Widget};

use crate::minmax;

pub trait TreeNode {
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>>;
    fn children(self) -> impl Iterator<Item = Self>;
}

pub struct LazyRow<'a, I: Iterator<Item = Span<'a>>> {
    iter: I,
}

impl<'a> LazyRow<'a, std::iter::Empty<Span<'a>>> {
    pub fn empty() -> Self {
        Self {
            iter: std::iter::empty(),
        }
    }
}

impl<'a, I: Iterator<Item = Span<'a>>> LazyRow<'a, I> {
    pub fn iter(self) -> I {
        self.iter
    }

    pub fn new(iter: impl IntoIterator<IntoIter = I>) -> Self {
        Self {
            iter: iter.into_iter(),
        }
    }

    pub fn add_cell(self, cell: Span<'a>) -> LazyRow<'a, impl Iterator<Item = Span<'a>>> {
        LazyRow {
            iter: std::iter::chain(self.iter, std::iter::once(cell)),
        }
    }
}

pub struct Tree<S: TreeNode> {
    root: S,
    inner: TreeInner,
}
struct TreeInner {
    offsets: Vec<u16>,
    branch: line::Set<'static>,
    max_depth: u16, // TODO: determine what happens when max depth is exceeded
}

impl TreeInner {
    /// Additional offset before row used to draw branch
    const BRIDGE_LEN: u16 = 1;

    #[inline(always)]
    fn put_default_joint(&self, depth: u16, height: u16, buf: &mut Buffer, area: &Rect) {
        if depth != 0 {
            buf.set_string(
                area.x + depth - 1,
                area.y + height,
                self.branch.bottom_left,
                Style::default(),
            );
        }
        buf.set_string(
            area.x + depth,
            area.y + height,
            self.branch.horizontal,
            Style::default(),
        );
    }

    #[inline(always)]
    fn connect_siblings(
        &self,
        depth: u16,
        sibling_height: u16,
        height: u16,
        buf: &mut Buffer,
        area: &Rect,
    ) {
        if depth == 0 {
            // Technically this is unreachable since the tree always has one root
            return;
        }
        let (low, high) = minmax(sibling_height, height);
        buf.set_string(
            area.x + depth - 1,
            area.y + low,
            self.branch.vertical_right,
            Style::default(),
        );
        for y in (low + 1)..(high) {
            buf.set_string(
                area.x + depth - 1,
                area.y + y,
                self.branch.vertical,
                Style::default(),
            );
        }
    }
    /// Renders node and all of its children.
    ///
    /// Returns the height where the next node of the depth may be rendered.
    /// Returned value is strictly greater than `height`
    pub fn render_node<S: TreeNode>(
        &self,
        node: S,
        buf: &mut Buffer,
        depth: u16,
        node_height: u16,
        area: &Rect,
    ) -> u16 {
        if node_height >= area.height {
            return node_height;
        }

        let row = node.row();
        self.print_row(buf, depth, node_height, row, area);
        self.put_default_joint(depth, node_height, buf, area);
        let mut height = node_height;
        height += 1;
        let mut prev_height = height;

        for c in node.children() {
            if height >= area.height {
                break;
            }
            self.connect_siblings(depth + 1, prev_height, height, buf, area);
            prev_height = height;
            height = self.render_node(c, buf, depth + 1, height, area);
        }

        // Replace bridge if node has children.
        if height != prev_height {
            let replacement = if node_height == 0 {
                self.branch.top_left
            } else {
                self.branch.horizontal_down
            };
            buf.set_string(
                area.x + depth,
                area.y + node_height,
                replacement,
                Style::default(),
            );
        }

        height
    }

    fn print_row<'a>(
        &self,
        buf: &mut Buffer,
        depth: u16,
        height: u16,
        row: LazyRow<'a, impl Iterator<Item = Span<'a>>>,
        area: &Rect,
    ) {
        let mut row = row.iter().zip(self.offsets.iter().copied());
        let (first, offset) = row.next().expect("Rows must not be empty");
        buf.set_string(
            area.x + Self::BRIDGE_LEN + offset + depth,
            area.y + height,
            &first.content,
            first.style,
        );
        for (cell, offset) in row {
            buf.set_string(
                area.x + Self::BRIDGE_LEN + offset + self.max_depth,
                area.y + height,
                &cell.content,
                cell.style,
            );
        }
    }
}

impl<S: TreeNode> Tree<S> {
    pub fn new(root: S, offsets: Vec<u16>) -> Self {
        Self {
            root,
            inner: TreeInner {
                offsets,
                branch: line::ROUNDED,
                max_depth: 8,
            },
        }
    }
    pub fn branch(mut self, set: line::Set<'static>) -> Self {
        self.inner.branch = set;
        self
    }
    pub fn max_depth(mut self, depth: u16) -> Self {
        self.inner.max_depth = depth;
        self
    }
}

impl<S: TreeNode> Widget for Tree<S> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let node = self.root;

        self.inner.render_node(node, buf, 0, 0, &area);
    }
}
