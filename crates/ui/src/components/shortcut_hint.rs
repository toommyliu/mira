use gpui::{
    App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Role,
    StatefulInteractiveElement as _, Styled as _, Window, anchored, deferred, div, px,
};

use crate::{ActiveTheme as _, primitives::layer, theme::TEXT_BODY};

pub fn shortcut_hint(
    label: &'static str,
    opacity: f32,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let viewport = window.viewport_size();
    let text_size = (f32::from(theme.typography.body) * 20.0 / f32::from(TEXT_BODY)).round();
    let panel = div()
        .id("shortcut-hint")
        .debug_selector(|| "shortcut-hint".into())
        .role(Role::Status)
        .aria_label(label)
        .opacity(opacity)
        .max_w(viewport.width - theme.space(8.0))
        .px(theme.space(6.0))
        .py(theme.space(3.0))
        .rounded_full()
        .bg(theme.foreground)
        .shadow(theme.large_shadow())
        .font_family(theme.font_family.clone())
        .text_size(px(text_size))
        .line_height(px((text_size * 1.4).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.background)
        .text_center()
        .child(label);
    deferred(
        anchored().position(gpui::Point::default()).child(
            div()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .flex_col()
                .items_center()
                .pt(viewport.height * 0.15)
                .child(panel),
        ),
    )
    .with_priority(layer::SHORTCUT_HINT)
}
