//! Widget for displaying process tree.
use ratatui::{
    prelude::*,
    style::Style,
    symbols::line,
    text::Span,
    widgets::{Block, Scrollbar, ScrollbarState, Widget},
};

const ELLIPSIS: &str = "…";

use crate::minmax;

pub trait TreeNode {
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>>;
    fn children(self) -> impl Iterator<Item = Self>;
}

pub trait EagerTreeNode: TreeNode {
    fn subtree_height(&self) -> usize;
    fn subtree_depth(&self) -> u16;
    fn children(&self) -> impl Iterator<Item = &Self>;
}

impl<T> TreeNode for &T
where
    T: EagerTreeNode,
{
    fn row<'s>(&'s self) -> LazyRow<'s, impl Iterator<Item = Span<'s>>> {
        TreeNode::row(*self)
    }

    fn children(self) -> impl Iterator<Item = Self> {
        EagerTreeNode::children(self)
    }
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

pub struct Tree<'a, S: TreeNode> {
    root: S,
    inner: TreeInner,
    block: Option<Block<'a>>,
    scrollbar: Option<Scrollbar<'a>>,
}

#[derive(Debug, Clone, Default)]
pub struct TreeState {
    pub scroll: ScrollbarState,
}

#[derive(Debug, Clone)]
struct TreeInner {
    column_lens: Vec<u16>,
    branch: line::Set<'static>,
    max_depth: u16, // TODO: determine what should happen when max depth is exceeded
}

#[derive(Debug)]
struct NodeRenderContext {
    area: Rect,
    scroll: u16,
}

impl NodeRenderContext {
    #[inline(always)]
    fn is_node_visible(&self, node_height: u16) -> bool {
        self.scroll <= node_height && node_height < self.area.height + self.scroll
    }
}

impl TreeInner {
    /// Additional offset before row used to draw branch
    const BRIDGE_LEN: u16 = 1;

    /// Prints joint at given coordinates relative to top left corner of the `area`
    #[inline(always)]
    fn put_default_joint(&self, x: u16, y: u16, buf: &mut Buffer, area: &Rect) {
        if x != 0 {
            buf.set_string(
                area.x + x - 1,
                area.y + y,
                self.branch.bottom_left,
                Style::default(),
            );
        }
        buf.set_string(
            area.x + x,
            area.y + y,
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
        ctx: &NodeRenderContext,
    ) {
        let area = &ctx.area;
        if depth == 0 {
            // Technically this is unreachable since the tree always has one root
            return;
        }
        let (low, high) = minmax(sibling_height, height);
        if ctx.is_node_visible(low) {
            buf.set_string(
                area.x + depth - 1,
                area.y + low - ctx.scroll,
                self.branch.vertical_right,
                Style::default(),
            );
        }
        for y in (low + 1).saturating_sub(ctx.scroll)..(high.saturating_sub(ctx.scroll)) {
            buf.set_string(
                area.x + depth - 1,
                area.y + y,
                self.branch.vertical,
                Style::default(),
            );
        }
    }
    /// Renders node and all of its children that fit in the area.
    ///
    /// Returns the height where the next node of the depth may be rendered.
    /// Returned value is strictly greater than `height`
    pub fn render_node<S: TreeNode>(
        &self,
        node: S,
        buf: &mut Buffer,
        depth: u16,
        node_height: u16,
        ctx: &NodeRenderContext,
    ) -> u16 {
        if node_height >= ctx.area.height + ctx.scroll {
            eprintln!(
                "rejected: {:?}, h={node_height}, scroll={}, area={:?}",
                node.row().iter().next().unwrap(),
                ctx.scroll,
                ctx.area
            );
            return node_height;
        }

        let visible = node_height >= ctx.scroll;
        if visible {
            let row = node.row();
            self.print_row(buf, depth, node_height - ctx.scroll, row, &ctx.area);
            self.put_default_joint(depth, node_height - ctx.scroll, buf, &ctx.area);
        }
        let mut height = node_height;
        height += 1;
        let mut prev_height = height;

        for c in node.children() {
            self.connect_siblings(depth + 1, prev_height, height, buf, &ctx);
            prev_height = height;
            height = self.render_node(c, buf, depth + 1, height, &ctx);
        }

        // Replace bridge if node has children.
        if height != prev_height && visible {
            let replacement = if node_height == 0 {
                self.branch.top_left
            } else {
                self.branch.horizontal_down
            };
            buf.set_string(
                ctx.area.x + depth,
                ctx.area.y + node_height - ctx.scroll,
                replacement,
                Style::default(),
            );
        }

        height
    }

    /// Prints row starting in given coordinates relative to top left
    /// corner of the `area`.
    fn print_row<'a>(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        row: LazyRow<'a, impl Iterator<Item = Span<'a>>>,
        area: &Rect,
    ) {
        let mut row = row.iter().zip(self.column_lens.iter().copied());
        let (first, len) = row.next().expect("Rows must not be empty");
        Self::print_string_sentinel(
            buf,
            area.x + Self::BRIDGE_LEN + x,
            area.y + y,
            &first.content,
            first.style,
            len as usize,
        );
        let mut offset = len + 1;
        for (cell, len) in row {
            Self::print_string_sentinel(
                buf,
                area.x + Self::BRIDGE_LEN + offset + self.max_depth,
                area.y + y,
                &cell.content,
                cell.style,
                len as usize,
            );
            offset += len + 1;
        }
    }

    #[inline(always)]
    fn print_string_sentinel(
        buf: &mut Buffer,
        x: u16,
        y: u16,
        string: &str,
        style: Style,
        max_width: usize,
    ) {
        buf.set_stringn(x, y, string, max_width, style).0;
        // TODO: proper width counting
        let width = string.chars().count();
        if max_width < width {
            buf.set_string(x + max_width as u16 - 1, y, ELLIPSIS, Style::default());
        }
    }
}

impl<'a, S: TreeNode> Tree<'a, S> {
    pub fn new(root: S, column_lens: Vec<u16>) -> Self {
        Self {
            root,
            inner: TreeInner {
                column_lens,
                branch: line::ROUNDED,
                max_depth: 8,
            },
            block: None,
            scrollbar: None,
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
    pub fn block<'b>(self, block: Block<'b>) -> Tree<'b, S>
    where
        'a: 'b,
    {
        Tree {
            block: Some(block),
            scrollbar: self.scrollbar,
            root: self.root,
            inner: self.inner,
        }
    }
    pub fn scrollbar<'b>(self, scrollbar: Scrollbar<'b>) -> Tree<'b, S>
    where
        'a: 'b,
    {
        Tree {
            root: self.root,
            inner: self.inner,
            block: self.block,
            scrollbar: Some(scrollbar),
        }
    }

    #[must_use]
    fn render_frame(
        &self,
        area: Rect,
        buf: &mut Buffer,
        scroll_state: &mut ScrollbarState,
    ) -> Rect {
        if let Some(ref scrollbar) = self.scrollbar {
            scrollbar.clone().render(area, buf, scroll_state);
        }
        if let Some(ref block) = self.block {
            block.render(area, buf);
            block.inner(area)
        } else {
            area
        }
    }
}

#[cfg(false)]
impl<'a, S: EagerTreeNode> Tree<'a, S> {
    pub fn render_from_while_fits(&self, area: Rect, buf: &mut Buffer, initial_height: usize) {
        let mut node = &self.root;
        let mut remaining_children;
        let mut lines_skipped = 0;

        if node.subtree_height() <= initial_height {
            return;
        }
        while lines_skipped < initial_height {
            // When entering the loop we assume that `initial_line` lies within the
            // subtree of `node`
            remaining_children = node.children();
            for child in remaining_children {
                if lines_skipped <= lines_skipped + child.subtree_height() {
                    node = child;
                    lines_skipped += child.subtree_height();
                    break;
                }
                lines_skipped += child.subtree_height();
            }
        }
        self.inner.render_node(
            node,
            buf,
            todo!(),
            initial_height as u16,
            &NodeRenderContext {
                area,
                scroll: todo!(),
            },
        );
    }
}

impl<'a, S: TreeNode> Widget for Tree<'a, S> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let area = self.render_frame(area, buf, &mut ScrollbarState::new(67));
        let node = self.root;
        self.inner
            .render_node(node, buf, 0, 0, &NodeRenderContext { area, scroll: 0 });
    }
}

impl<'a, S> StatefulWidget for &Tree<'a, S>
where
    S: EagerTreeNode,
{
    type State = TreeState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let mut inner = self.inner.clone();
        inner.max_depth = self.root.subtree_depth();
        state.scroll = state
            .scroll
            .content_length(self.root.subtree_height())
            .viewport_content_length(area.height as usize);

        let area = self.render_frame(area, buf, &mut state.scroll);
        self.inner.render_node(
            &self.root,
            buf,
            0,
            0,
            &NodeRenderContext {
                area,
                scroll: state.scroll.get_position() as u16,
            },
        );
    }
}
