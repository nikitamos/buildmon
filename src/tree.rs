//! Widget for displaying process tree.
use ratatui::{prelude::Buffer, text::Span, widgets::Widget};

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
}

impl TreeInner {
    /// Renders node and all of its children.
    ///
    /// Return the height where the next node of the depth may be rendered
    pub fn render_node<'s, S: TreeNode>(
        &'s self,
        node: S,
        buf: &mut Buffer,
        depth: u16,
        mut height: u16,
    ) -> u16 {
        let row = node.row();
        for (cell, offset) in row.iter().zip(&self.offsets) {
            buf.set_string(offset + depth, height, &cell.content, cell.style);
        }
        height += 1;

        for c in node.children() {
            // I guess, here we will draw branches
            height = self.render_node(c, buf, depth + 1, height);
        }

        height
    }
}

impl<S: TreeNode> Tree<S> {
    pub fn new(root: S, offsets: Vec<u16>) -> Self {
        Self {
            root,
            inner: TreeInner { offsets },
        }
    }
}

impl<'a, 'this, S: TreeNode> Widget for Tree<S>
where
    'this: 'a,
{
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let node = self.root;

        self.inner.render_node(node, buf, 0, 0);
    }
}
