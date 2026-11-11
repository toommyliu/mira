use gpui::{
    Canvas, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, PathBuilder,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, canvas, div,
    point, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    Theme,
    theme::{PANEL_INSET, ROW, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL},
};

pub(crate) const BAR_HEIGHT: Pixels = px(32.0);

pub(super) fn panel(theme: &Theme, id: &'static str, width: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .w(width)
        .p(PANEL_INSET)
        .flex()
        .flex_col()
        .rounded(theme.radius.panel)
        .bg(theme.overlay)
        .border_1()
        .border_color(theme.border)
        .shadow(theme.floating())
        .text_size(TEXT_BODY)
        .text_color(theme.text)
}

pub(super) fn row(theme: &Theme, id: impl Into<gpui::ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .h(ROW)
        .px(px(8.0))
        .rounded(theme.radius.item)
}

pub(super) fn pressable(theme: &Theme, row: Stateful<Div>) -> Stateful<Div> {
    let (hover, pressed) = (theme.fill_hover, theme.fill_pressed);
    row.cursor_pointer()
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(pressed))
}

pub(super) fn section(theme: &Theme, label: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(2.0))
        .text_size(TEXT_CAPTION)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.text_tertiary)
        .child(label)
}

pub(super) fn separator(theme: &Theme) -> impl IntoElement {
    div()
        .flex_none()
        .mx(px(8.0))
        .my(PANEL_INSET)
        .h(px(1.0))
        .bg(theme.hairline)
}

pub(super) fn status(theme: &Theme, text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pb(px(6.0))
        .text_size(TEXT_SMALL)
        .text_color(theme.text_secondary)
        .child(text.into())
}

pub(super) fn switch(theme: &Theme, on: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .when(on, |track| track.justify_end())
        .w(px(26.0))
        .h(px(16.0))
        .p(px(2.0))
        .rounded_full()
        .bg(if on {
            theme.accent
        } else {
            theme.border_strong
        })
        .child(div().size(px(12.0)).rounded_full().bg(theme.knob))
}

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Down,
    Right,
    Close,
}

pub(super) fn glyph(glyph: Glyph, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let c = bounds.center();
            let strokes: &[&[(f32, f32)]] = match glyph {
                Glyph::Down => &[&[(-3.5, -1.5), (0.0, 2.0), (3.5, -1.5)]],
                Glyph::Right => &[&[(-1.5, -3.5), (2.0, 0.0), (-1.5, 3.5)]],
                Glyph::Close => &[&[(-3.0, -3.0), (3.0, 3.0)], &[(-3.0, 3.0), (3.0, -3.0)]],
            };
            let mut path = PathBuilder::stroke(px(1.5));
            for stroke in strokes {
                for (index, (x, y)) in stroke.iter().enumerate() {
                    let at = point(c.x + px(*x), c.y + px(*y));
                    if index == 0 {
                        path.move_to(at);
                    } else {
                        path.line_to(at);
                    }
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .flex_none()
    .size(px(12.0))
}

pub(super) fn number_field(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    text: String,
    editing: bool,
    fresh: bool,
) -> Stateful<Div> {
    input(theme, id, text, editing, fresh)
        .w(px(52.0))
        .justify_end()
}

pub(super) fn text_field(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    text: String,
    placeholder: String,
    editing: bool,
    fresh: bool,
) -> Stateful<Div> {
    let empty = text.is_empty() && !editing;
    input(
        theme,
        id,
        if empty { placeholder } else { text },
        editing,
        fresh,
    )
    .w(px(180.0))
    .justify_start()
    .when(empty, |field| field.text_color(theme.text_tertiary))
}

fn input(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    text: String,
    editing: bool,
    fresh: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(22.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .bg(theme.fill)
        .border_1()
        .border_color(if editing { theme.accent } else { theme.border })
        .cursor_text()
        .text_size(TEXT_BODY)
        .overflow_hidden()
        .child(
            div()
                .line_height(px(16.0))
                .px(px(1.0))
                .when(editing && fresh && !text.is_empty(), |text| {
                    text.bg(theme.accent.opacity(0.35)).rounded(px(2.0))
                })
                .child(text),
        )
        .when(editing && !fresh, |field| {
            field.child(div().ml(px(1.0)).w(px(1.0)).h(px(14.0)).bg(theme.accent))
        })
}
