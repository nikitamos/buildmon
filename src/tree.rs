//! Widget for displaying process tree.
use ratatui::{prelude::Buffer, style::Style, symbols::line, text::Span, widgets::Widget};

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
}

impl TreeInner {
    // #[inline(always)]
    // fn put_join(depth)
    #[inline(always)]
    fn put_default_join(&self, depth: u16, height: u16, buf: &mut Buffer) {
        if depth != 0 {
            buf.set_string(depth - 1, height, self.branch.bottom_left, Style::default());
        }
    }
    #[inline(always)]
    fn connect_siblings(&self, depth: u16, sibling_height: u16, height: u16, buf: &mut Buffer) {
        if depth == 0 {
            // Technically this is unreachable since the tree always has one root
            return;
        }
        let (low, high) = minmax(sibling_height, height);
        buf.set_string(depth - 1, low, self.branch.vertical_right, Style::default());
        for y in (low + 1)..(high) {
            buf.set_string(depth - 1, y, self.branch.vertical, Style::default());
        }
    }
    /// Renders node and all of its children.
    ///
    /// Return the height where the next node of the depth may be rendered
    pub fn render_node<S: TreeNode>(
        &self,
        node: S,
        buf: &mut Buffer,
        depth: u16,
        mut height: u16,
    ) -> u16 {
        let row = node.row();
        for (cell, offset) in row.iter().zip(&self.offsets) {
            buf.set_string(offset + depth, height, &cell.content, cell.style);
        }
        self.put_default_join(depth, height, buf);
        height += 1;
        let mut prev_height = height;

        for c in node.children() {
            self.connect_siblings(depth+1, prev_height, height, buf);
            prev_height = height;
            height = self.render_node(c, buf, depth + 1, height);
        }

        height
    }
}

impl<S: TreeNode> Tree<S> {
    pub fn new(root: S, offsets: Vec<u16>) -> Self {
        Self {
            root,
            inner: TreeInner {
                offsets,
                branch: line::ROUNDED,
            },
        }
    }
    pub fn branch(mut self, set: line::Set<'static>) -> Self {
        self.inner.branch = set;
        self
    }
}

impl<S: TreeNode> Widget for Tree<S> {
    fn render(self, _area: ratatui::prelude::Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let node = self.root;

        self.inner.render_node(node, buf, 0, 0);
    }
}
