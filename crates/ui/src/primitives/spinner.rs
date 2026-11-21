use gpui::{
    App, Bounds, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, point, prelude::FluentBuilder as _, px, svg,
};

use crate::{ActiveTheme as _, motion};

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    label: SharedString,
    color: Option<Hsla>,
    size: Pixels,
    value: Option<f32>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "Loading".into(),
            color: None,
            size: px(16.0),
            value: None,
        }
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

    pub fn value(mut self, value: Option<f32>) -> Self {
        self.value = value;
        self
    }
}

fn paint_arc(bounds: Bounds<Pixels>, fraction: f32, color: Hsla, window: &mut Window) {
    if fraction <= 0.0 {
        return;
    }
    let scale = bounds.size.width / 24.0;
    let radius = scale * 9.0;
    let center = bounds.center();
    let at = |turn: f32| {
        let angle = (turn * 360.0 - 90.0).to_radians();
        point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        )
    };
    let mut path = PathBuilder::stroke(scale * 2.5);
    path.move_to(at(0.0));
    let radii = point(radius, radius);
    if fraction >= 1.0 {
        path.arc_to(radii, px(0.0), false, true, at(0.5));
        path.arc_to(radii, px(0.0), false, true, at(1.0));
    } else {
        path.arc_to(radii, px(0.0), fraction > 0.5, true, at(fraction));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
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
        div()
            .id(self.id.clone())
            .role(Role::ProgressIndicator)
            .aria_label(self.label)
            .relative()
            .flex_none()
            .size(self.size)
            .when_some(self.value, |ring, value| {
                ring.aria_numeric_value(f64::from(value.clamp(0.0, 1.0) * 100.0))
                    .aria_min_numeric_value(0.0)
                    .aria_max_numeric_value(100.0)
            })
            .child(layer("icons/spinner-track.svg", color.opacity(0.2)))
            .child(match self.value {
                Some(value) => {
                    let shown =
                        motion::travel((self.id, "fill"), value.clamp(0.0, 1.0), window, cx);
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| paint_arc(bounds, shown, color, window),
                    )
                    .absolute()
                    .inset_0()
                    .into_any_element()
                }
                None => motion::spin(self.id, layer("icons/spinner-arc.svg", color), cx)
                    .into_any_element(),
            })
    }
}
