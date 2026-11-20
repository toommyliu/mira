use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme as _;

const HEIGHTS: [f32; 3] = [4.0, 7.0, 10.0];

#[derive(IntoElement)]
pub struct Signal {
    label: SharedString,
    level: usize,
}

impl Signal {
    pub fn new(label: impl Into<SharedString>, level: usize) -> Self {
        Self {
            label: label.into(),
            level: level.min(HEIGHTS.len()),
        }
    }
}

impl RenderOnce for Signal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (lit, unlit) = (
            theme.foreground,
            theme
                .foreground
                .opacity(if theme.dark_mode { 0.16 } else { 0.12 }),
        );
        div()
            .id(self.label.clone())
            .role(Role::Image)
            .aria_label(self.label)
            .flex_none()
            .flex()
            .items_end()
            .gap(px(1.5))
            .h(px(10.0))
            .children(HEIGHTS.iter().enumerate().map(|(index, height)| {
                div()
                    .w(px(3.0))
                    .h(px(*height))
                    .rounded(px(1.0))
                    .bg(unlit)
                    .when(index < self.level, |bar| bar.bg(lit))
            }))
    }
}
