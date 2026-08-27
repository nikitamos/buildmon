//! Wrapper around `StatefulWidget::State` that can hold additional information

use ratatui::prelude::*;

#[derive(Debug, Clone, Default)]
pub struct ComponentState<S: ?Sized> {
    pub area: Rect,
    pub focus: bool,
    pub inner_state: S,
}

impl<S> ComponentState<S> {
    pub fn focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }
}

impl<S: ?Sized> std::ops::DerefMut for ComponentState<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner_state
    }
}

impl<S: ?Sized> std::ops::Deref for ComponentState<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.inner_state
    }
}

impl<S> From<S> for ComponentState<S> {
    fn from(value: S) -> Self {
        Self { area: Default::default(), focus: Default::default(), inner_state: value }
    }
}

pub struct Component<W>(pub W);

impl<W> From<W> for Component<W> {
    fn from(value: W) -> Self {
        Component(value)
    }
}

impl<W: StatefulWidget> StatefulWidget for Component<W> {
    type State = ComponentState<W::State>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.area = area;
        self.0.render(area, buf, &mut state.inner_state);
    }
}
