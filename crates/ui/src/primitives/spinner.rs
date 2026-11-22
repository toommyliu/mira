use std::time::Instant;

use gpui::{
    App, Bounds, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, point, px, svg,
};

use crate::{ActiveTheme as _, motion};

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    label: SharedString,
    color: Option<Hsla>,
    size: Pixels,
    countdown: Option<(Instant, Instant)>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "Loading".into(),
            color: None,
            size: px(16.0),
            countdown: None,
        }
    }

    pub fn countdown(mut self, from: Instant, until: Instant) -> Self {
        self.countdown = Some((from, until));
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().foreground);
        let layer = |path: &'static str, color: Hsla| {
            svg()
                .path(path)
                .absolute()
                .inset_0()
                .size(self.size)
                .text_color(color)
        };
        let spinner = div()
            .id(self.id.clone())
            .role(Role::ProgressIndicator)
            .aria_label(self.label)
            .relative()
            .flex_none()
            .size(self.size)
            .child(layer("icons/spinner-track.svg", color.opacity(0.2)));
        let Some((from, until)) = self.countdown else {
            return spinner.child(motion::spin(
                self.id,
                layer("icons/spinner-arc.svg", color),
                cx,
            ));
        };
        let now = Instant::now();
        if now < until {
            window.request_animation_frame();
        }
        let length = until.saturating_duration_since(from).as_secs_f32();
        let elapsed = if length > 0.0 {
            (now.saturating_duration_since(from).as_secs_f32() / length).min(1.0)
        } else {
            1.0
        };
        spinner
            .aria_numeric_value(f64::from((1.0 - elapsed) * 100.0))
            .aria_min_numeric_value(0.0)
            .aria_max_numeric_value(100.0)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| remaining_arc(bounds, elapsed, color, window),
                )
                .absolute()
                .inset_0(),
            )
    }
}

fn remaining_arc(bounds: Bounds<Pixels>, elapsed: f32, color: Hsla, window: &mut Window) {
    if elapsed >= 1.0 {
        return;
    }
    let side = bounds.size.width.min(bounds.size.height);
    let stroke = side * (2.5 / 24.0);
    let radius = side * (9.0 / 24.0);
    let center = bounds.center();
    let at = |turn: f32| {
        let angle = (turn * 360.0 - 90.0).to_radians();
        point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        )
    };
    let radii = point(radius, radius);
    let mut arc = PathBuilder::stroke(stroke);
    arc.move_to(at(elapsed));
    if elapsed <= 0.0 {
        arc.arc_to(radii, px(0.0), false, true, at(0.5));
        arc.arc_to(radii, px(0.0), false, true, at(1.0));
    } else {
        arc.arc_to(radii, px(0.0), elapsed < 0.5, true, at(1.0));
    }
    if let Ok(arc) = arc.build() {
        window.paint_path(arc, color);
    }
    let cap = point(stroke / 2.0, stroke / 2.0);
    for end in [at(elapsed), at(1.0)] {
        let mut dot = PathBuilder::fill();
        dot.move_to(point(end.x - cap.x, end.y));
        dot.arc_to(cap, px(0.0), false, true, point(end.x + cap.x, end.y));
        dot.arc_to(cap, px(0.0), false, true, point(end.x - cap.x, end.y));
        if let Ok(dot) = dot.build() {
            window.paint_path(dot, color);
        }
    }
}
