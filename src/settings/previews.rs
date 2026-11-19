use gpui::{
    Div, Hsla, ParentElement as _, Pixels, Styled as _, div, prelude::FluentBuilder as _, px,
    relative,
};
use mira_ui::{Theme, h_flex, v_flex};

pub(super) const HEIGHT: Pixels = px(48.0);
pub(super) const NARROW: Pixels = px(72.0);
pub(super) const WIDE: Pixels = px(88.0);

fn bar(color: Hsla, width: f32, height: f32) -> Div {
    div()
        .flex_none()
        .w(relative(width))
        .h(px(height))
        .rounded(px(height / 2.0))
        .bg(color)
}

fn frame(theme: &Theme, content: Div) -> Div {
    let row = |selected: bool, width: f32| {
        div()
            .flex()
            .items_center()
            .h(px(5.0))
            .px(px(1.5))
            .rounded(px(1.5))
            .when(selected, |row| row.bg(theme.sidebar_accent))
            .child(bar(
                theme
                    .sidebar_foreground
                    .opacity(if selected { 0.6 } else { 0.3 }),
                width,
                2.0,
            ))
    };
    h_flex()
        .items_stretch()
        .size_full()
        .rounded(theme.radius.small)
        .bg(theme.background)
        .overflow_hidden()
        .child(
            v_flex()
                .flex_none()
                .w(relative(0.34))
                .gap(px(2.0))
                .p(px(3.0))
                .rounded_l(theme.radius.small)
                .bg(theme.sidebar)
                .border_r_1()
                .border_color(theme.sidebar_border)
                .child(row(true, 0.75))
                .children([0.85, 0.6, 0.7].map(|width| row(false, width))),
        )
        .child(content.flex_1().min_w_0())
}

pub(super) fn theme(theme: &Theme) -> Div {
    let button = |width: f32, color: Hsla| div().w(px(width)).h(px(6.0)).rounded(px(2.0)).bg(color);
    frame(
        theme,
        v_flex()
            .gap(px(3.0))
            .p(px(5.0))
            .child(bar(theme.foreground.opacity(0.8), 0.55, 3.0))
            .child(bar(theme.muted_foreground.opacity(0.6), 0.9, 2.0))
            .child(bar(theme.muted_foreground.opacity(0.6), 0.7, 2.0))
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(2.0))
                    .child(button(14.0, theme.primary))
                    .child(button(10.0, theme.secondary)),
            ),
    )
}

pub(super) fn split(light: &Theme, dark: &Theme) -> Div {
    let half = |theme: &Theme, end: bool| {
        let place = |half: Div| if end { half.right_0() } else { half.left_0() };
        place(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(relative(0.5))
                .overflow_hidden(),
        )
        .child(
            place(div().absolute().top_0().bottom_0().w(relative(2.0))).child(self::theme(theme)),
        )
    };
    div()
        .relative()
        .size_full()
        .child(half(light, false))
        .child(half(dark, true))
}

pub(super) fn client(theme: &Theme) -> Div {
    frame(
        theme,
        v_flex()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(8.0))
                    .px(px(4.0))
                    .gap(px(2.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .children([8.0, 11.0, 8.0].map(|width| {
                        div()
                            .w(px(width))
                            .h(px(2.0))
                            .rounded(px(1.0))
                            .bg(theme.muted_foreground.opacity(0.6))
                    })),
            )
            .child(div().flex_1().m(px(3.0)).rounded(px(2.0)).bg(theme.muted)),
    )
}

pub(super) fn manager(theme: &Theme) -> Div {
    let card = || {
        v_flex()
            .flex_1()
            .justify_center()
            .gap(px(2.0))
            .px(px(3.0))
            .rounded(px(2.0))
            .bg(theme.card)
            .border_1()
            .border_color(theme.border)
            .child(bar(theme.foreground.opacity(0.7), 0.6, 2.0))
            .child(bar(theme.muted_foreground.opacity(0.5), 0.85, 1.5))
    };
    let row = || {
        h_flex()
            .flex_1()
            .items_stretch()
            .gap(px(2.0))
            .children([card(), card()])
    };
    frame(
        theme,
        v_flex()
            .gap(px(2.0))
            .p(px(3.0))
            .child(
                h_flex().flex_none().gap(px(2.0)).children(
                    [8.0, 11.0, 8.0]
                        .map(|width| div().w(px(width)).h(px(3.0)).rounded_full().bg(theme.muted)),
                ),
            )
            .child(row())
            .child(row()),
    )
}
