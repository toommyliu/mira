use gpui::{
    Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
    px,
};
use mira_ui::{
    StyledExt as _, Theme, h_flex,
    primitives::Switch,
    theme::{DISABLED_OPACITY, Layout},
    v_flex,
};

pub(super) const LABEL_WIDTH: Pixels = px(96.0);

pub(super) fn note(text: impl Into<SharedString>, enabled: bool, theme: &Theme) -> Div {
    let typography = theme.typography;
    div()
        .text_caption(typography)
        .text_color(theme.muted_foreground)
        .when(!enabled, |note| note.opacity(DISABLED_OPACITY))
        .child(text.into())
}

pub(super) fn section(
    title: &'static str,
    subtitle: &'static str,
    status: Option<(SharedString, Hsla)>,
    controls: impl IntoElement,
    theme: &Theme,
) -> Div {
    let layout = theme.layout;
    let typography = theme.typography;
    let header = h_flex()
        .gap(layout.space(3.0))
        .min_h(layout.control)
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .justify_end()
                .gap(layout.space(2.5))
                .when_some(status, |row, (text, color)| {
                    row.child(
                        div()
                            .id("status")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .min_w_0()
                            .truncate()
                            .text_caption(typography)
                            .text_color(color)
                            .child(text),
                    )
                })
                .child(controls),
        );
    v_flex()
        .gap(layout.space(1.0))
        .child(header)
        .child(note(subtitle, true, theme))
}

pub(super) fn row_label(label: impl Into<SharedString>, enabled: bool) -> Div {
    div()
        .min_w_0()
        .truncate()
        .font_weight(FontWeight::MEDIUM)
        .when(!enabled, |label| label.opacity(DISABLED_OPACITY))
        .child(label.into())
}

pub(super) fn row(
    layout: Layout,
    label: impl Into<SharedString>,
    enabled: bool,
    control: impl IntoElement,
) -> Div {
    h_flex()
        .gap(layout.space(2.0))
        .min_h(layout.control)
        .child(row_label(label, enabled).flex_none().w(LABEL_WIDTH))
        .child(div().flex_1().min_w_0().child(control))
}

pub(super) fn switch_row(
    layout: Layout,
    label: impl Into<SharedString>,
    enabled: bool,
    switch: Switch,
) -> Div {
    h_flex()
        .justify_between()
        .gap(layout.space(2.0))
        .min_h(layout.control)
        .child(row_label(label, enabled))
        .child(switch.disabled(!enabled).hide_label())
}
