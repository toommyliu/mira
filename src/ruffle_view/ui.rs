use std::{
    f32::consts::{PI, TAU},
    time::Duration,
};

use gpui::{
    Animation, AnimationExt as _, Bounds, BoxShadow, Canvas, Div, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, PathBuilder, Pixels, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, canvas, div, fill, hsla, point,
    prelude::FluentBuilder as _, px, size,
};

pub(crate) const BAR_HEIGHT: Pixels = px(36.0);
pub(super) const ROW_HEIGHT: Pixels = px(28.0);
pub(super) const PANEL_PADDING: Pixels = px(4.0);
pub(super) const PANEL_RADIUS: Pixels = px(10.0);
pub(super) const CONTROL_RADIUS: Pixels = px(6.0);
pub(super) const TEXT_SIZE: Pixels = px(13.0);
pub(super) const SMALL_TEXT_SIZE: Pixels = px(12.0);
pub(super) const LABEL_SIZE: Pixels = px(11.0);

fn white(alpha: f32) -> Hsla {
    hsla(0.0, 0.0, 1.0, alpha)
}

pub(super) fn bar_background() -> Hsla {
    hsla(240.0 / 360.0, 0.02, 0.085, 1.0)
}

pub(super) fn panel_background() -> Hsla {
    hsla(240.0 / 360.0, 0.03, 0.12, 0.98)
}

pub(super) fn hairline() -> Hsla {
    white(0.08)
}

pub(super) fn hover() -> Hsla {
    white(0.07)
}

pub(super) fn pressed() -> Hsla {
    white(0.12)
}

pub(super) fn text() -> Hsla {
    white(0.94)
}

pub(super) fn secondary_text() -> Hsla {
    white(0.58)
}

pub(super) fn tertiary_text() -> Hsla {
    white(0.38)
}

pub(super) fn accent() -> Hsla {
    hsla(211.0 / 360.0, 1.0, 0.52, 1.0)
}

pub(super) fn panel(id: &'static str, width: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .w(width)
        .p(PANEL_PADDING)
        .flex()
        .flex_col()
        .rounded(PANEL_RADIUS)
        .bg(panel_background())
        .border_1()
        .border_color(white(0.1))
        .shadow(vec![
            BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.45),
                offset: point(px(0.0), px(10.0)),
                blur_radius: px(30.0),
                spread_radius: px(0.0),
                inset: false,
            },
            BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.3),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(3.0),
                spread_radius: px(0.0),
                inset: false,
            },
        ])
        .text_size(TEXT_SIZE)
        .text_color(text())
}

pub(super) fn row(id: impl Into<gpui::ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .h(ROW_HEIGHT)
        .px(px(8.0))
        .rounded(PANEL_RADIUS - PANEL_PADDING)
}

pub(super) fn pressable(row: Stateful<Div>) -> Stateful<Div> {
    row.cursor_pointer()
        .hover(|style| style.bg(hover()))
        .active(|style| style.bg(pressed()))
}

pub(super) fn section(label: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(2.0))
        .text_size(LABEL_SIZE)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(tertiary_text())
        .child(label)
}

pub(super) fn separator() -> impl IntoElement {
    div()
        .flex_none()
        .mx(px(8.0))
        .my(PANEL_PADDING)
        .h(px(1.0))
        .bg(hairline())
}

pub(super) fn status(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pb(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(secondary_text())
        .child(text.into())
}

pub(super) fn switch(on: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .when(on, |track| track.justify_end())
        .w(px(26.0))
        .h(px(16.0))
        .p(px(2.0))
        .rounded_full()
        .bg(if on { accent() } else { white(0.18) })
        .child(div().size(px(12.0)).rounded_full().bg(white(1.0)))
}

#[derive(Clone, Copy)]
pub(super) enum Glyph {
    Down,
    Right,
    Check,
}

pub(super) fn glyph(glyph: Glyph, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let c = bounds.center();
            let points: &[(f32, f32)] = match glyph {
                Glyph::Down => &[(-3.5, -1.5), (0.0, 2.0), (3.5, -1.5)],
                Glyph::Right => &[(-1.5, -3.5), (2.0, 0.0), (-1.5, 3.5)],
                Glyph::Check => &[(-4.0, 0.0), (-1.25, 3.0), (4.0, -3.5)],
            };
            let mut path = PathBuilder::stroke(px(1.5));
            for (index, (x, y)) in points.iter().enumerate() {
                let at = point(c.x + px(*x), c.y + px(*y));
                if index == 0 {
                    path.move_to(at);
                } else {
                    path.line_to(at);
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

pub(super) fn spinner() -> impl IntoElement {
    div().size(px(24.0)).with_animation(
        "spinner",
        Animation::new(Duration::from_millis(1000)).repeat(),
        |spinner, turn| {
            spinner.child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let width = 2.0;
                        let radius = f32::from(bounds.size.width) / 2.0 - width / 2.0;
                        let radii = point(px(radius), px(radius));
                        let center = bounds.center();
                        let at = |angle: f32| {
                            point(
                                center.x + px(radius * angle.cos()),
                                center.y + px(radius * angle.sin()),
                            )
                        };
                        let mut track = PathBuilder::stroke(px(width));
                        track.move_to(at(0.0));
                        track.arc_to(radii, px(0.0), false, true, at(PI));
                        track.arc_to(radii, px(0.0), false, true, at(0.0));
                        if let Ok(path) = track.build() {
                            window.paint_path(path, hsla(0.0, 0.0, 0.2, 1.0));
                        }
                        let start = turn * TAU;
                        let end = start + TAU / 4.0;
                        let color = hsla(0.0, 0.0, 0.85, 1.0);
                        let mut arc = PathBuilder::stroke(px(width));
                        arc.move_to(at(start));
                        arc.arc_to(radii, px(0.0), false, true, at(end));
                        if let Ok(path) = arc.build() {
                            window.paint_path(path, color);
                        }
                        for angle in [start, end] {
                            let cap = Bounds::centered_at(at(angle), size(px(width), px(width)));
                            window.paint_quad(fill(cap, color).corner_radii(px(width / 2.0)));
                        }
                    },
                )
                .size_full(),
            )
        },
    )
}

pub(super) fn number_field(
    id: impl Into<gpui::ElementId>,
    text: String,
    editing: bool,
    fresh: bool,
) -> Stateful<Div> {
    input(id, text, editing, fresh).w(px(52.0)).justify_end()
}

pub(super) fn text_field(
    id: impl Into<gpui::ElementId>,
    text: String,
    placeholder: &'static str,
    editing: bool,
    fresh: bool,
) -> Stateful<Div> {
    let empty = text.is_empty() && !editing;
    input(
        id,
        if empty { placeholder.into() } else { text },
        editing,
        fresh,
    )
    .w(px(180.0))
    .justify_start()
    .when(empty, |field| field.text_color(tertiary_text()))
}

fn input(
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
        .bg(white(0.06))
        .border_1()
        .border_color(if editing { accent() } else { white(0.1) })
        .cursor_text()
        .text_size(TEXT_SIZE)
        .overflow_hidden()
        .child(
            div()
                .line_height(px(16.0))
                .px(px(1.0))
                .when(editing && fresh && !text.is_empty(), |text| {
                    text.bg(accent().opacity(0.35)).rounded(px(2.0))
                })
                .child(text),
        )
        .when(editing && !fresh, |field| {
            field.child(div().ml(px(1.0)).w(px(1.0)).h(px(14.0)).bg(accent()))
        })
}
