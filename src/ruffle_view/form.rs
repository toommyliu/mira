use gpui::{
    Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
    px,
};
use mira_ui::{
    StyledExt as _, Theme, h_flex,
    primitives::Switch,
    theme::{DISABLED_OPACITY, ROW},
    v_flex,
};

pub(super) const LABEL_WIDTH: Pixels = px(96.0);

pub(super) fn note(text: impl Into<SharedString>, enabled: bool, theme: &Theme) -> Div {
    div()
        .text_small()
        .text_color(theme.text_tertiary)
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
    let header = h_flex()
        .gap(px(12.0))
        .min_h(ROW)
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
                .gap(px(10.0))
                .when_some(status, |row, (text, color)| {
                    row.child(
                        div()
                            .id("status")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .min_w_0()
                            .truncate()
                            .text_small()
                            .text_color(color)
                            .child(text),
                    )
                })
                .child(controls),
        );
    v_flex()
        .gap(px(4.0))
        .child(header)
        .child(note(subtitle, true, theme))
}

pub(super) fn row_label(label: impl Into<SharedString>, enabled: bool) -> Div {
    div()
        .min_w_0()
        .truncate()
        .when(!enabled, |label| label.opacity(DISABLED_OPACITY))
        .child(label.into())
}

pub(super) fn row(label: impl Into<SharedString>, enabled: bool, control: impl IntoElement) -> Div {
    h_flex()
        .gap(px(12.0))
        .min_h(ROW)
        .child(row_label(label, enabled).flex_none().w(LABEL_WIDTH))
        .child(div().flex_1().min_w_0().child(control))
}

pub(super) fn switch_row(label: impl Into<SharedString>, enabled: bool, switch: Switch) -> Div {
    h_flex()
        .justify_between()
        .gap(px(12.0))
        .min_h(ROW)
        .child(row_label(label, enabled))
        .child(switch.disabled(!enabled).hide_label())
}
