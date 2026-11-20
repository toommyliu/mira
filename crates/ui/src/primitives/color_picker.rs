use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    App, AppContext as _, Bounds, ElementId, Entity, Focusable as _, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, canvas, div, hsla,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px, relative,
};

use super::{Align, Button, Input, InputEvent, Placement, Popover, Side, button::Size};
use crate::{
    ActiveTheme as _, PointerCursor as _, Theme, h_flex, nav,
    theme::{parse_hex, to_hex},
    v_flex,
};

type ColorHandler = Rc<dyn Fn(Hsla, &mut Window, &mut App)>;

const PANEL_INSET: Pixels = px(4.0);
const AREA_WIDTH: Pixels = px(200.0);
const SQUARE_HEIGHT: Pixels = px(168.0);
const HUE_HEIGHT: Pixels = px(24.0);
const SQUARE_THUMB: Pixels = px(16.0);
const HUE_THUMB: Pixels = px(24.0);
const SWATCH: Pixels = px(14.0);

#[derive(Clone, Copy, PartialEq, Debug)]
struct Hsv {
    h: f32,
    s: f32,
    v: f32,
}

impl Hsv {
    fn of(color: Hsla, keep_hue: f32) -> Self {
        let rgb = color.to_rgb();
        let max = rgb.r.max(rgb.g).max(rgb.b);
        let min = rgb.r.min(rgb.g).min(rgb.b);
        let delta = max - min;
        let h = if delta <= f32::EPSILON {
            keep_hue
        } else if max == rgb.r {
            60.0 * ((rgb.g - rgb.b) / delta).rem_euclid(6.0)
        } else if max == rgb.g {
            60.0 * ((rgb.b - rgb.r) / delta + 2.0)
        } else {
            60.0 * ((rgb.r - rgb.g) / delta + 4.0)
        };
        Self {
            h,
            s: if max <= f32::EPSILON {
                0.0
            } else {
                delta / max
            },
            v: max,
        }
    }

    fn color(self) -> Hsla {
        let c = self.v * self.s;
        let x = c * (1.0 - ((self.h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = self.v - c;
        let (r, g, b) = match (self.h.rem_euclid(360.0) / 60.0) as u8 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        gpui::Rgba {
            r: r + m,
            g: g + m,
            b: b + m,
            a: 1.0,
        }
        .into()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Area {
    Square,
    Hue,
}

struct Picker {
    hsv: Cell<Hsv>,
    value: Cell<Option<Hsla>>,
    dragging: Cell<Option<Area>>,
    square: Cell<Bounds<Pixels>>,
    hue: Cell<Bounds<Pixels>>,
    on_change: RefCell<Option<ColorHandler>>,
}

impl Picker {
    fn new() -> Self {
        Self {
            hsv: Cell::new(Hsv {
                h: 0.0,
                s: 0.0,
                v: 0.0,
            }),
            value: Cell::new(None),
            dragging: Cell::new(None),
            square: Cell::new(Bounds::default()),
            hue: Cell::new(Bounds::default()),
            on_change: RefCell::new(None),
        }
    }

    fn sync(&self, value: Hsla) {
        if self.dragging.get().is_some() || self.value.get() == Some(value) {
            return;
        }
        self.value.set(Some(value));
        if self.hsv.get().color() != value {
            self.hsv.set(Hsv::of(value, self.hsv.get().h));
        }
    }

    fn point(&self, area: Area, position: Point<Pixels>) {
        let mut hsv = self.hsv.get();
        match area {
            Area::Square => {
                let bounds = self.square.get();
                hsv.s = fraction(position.x - bounds.left(), bounds.size.width);
                hsv.v = 1.0 - fraction(position.y - bounds.top(), bounds.size.height);
            }
            Area::Hue => {
                let bounds = self.hue.get();
                hsv.h = 360.0 * fraction(position.x - bounds.left(), bounds.size.width);
            }
        }
        self.hsv.set(hsv);
    }

    fn commit(&self, window: &mut Window, cx: &mut App) {
        self.value.set(None);
        let on_change = self.on_change.borrow().clone();
        if let Some(on_change) = on_change {
            on_change(self.hsv.get().color(), window, cx);
        }
    }
}

fn fraction(offset: Pixels, length: Pixels) -> f32 {
    if length <= Pixels::ZERO {
        return 0.0;
    }
    (offset / length).clamp(0.0, 1.0)
}

struct Hex {
    input: Entity<Input>,
    _changes: Subscription,
}

#[derive(IntoElement)]
pub struct ColorPicker {
    id: ElementId,
    label: SharedString,
    value: Hsla,
    on_change: Option<ColorHandler>,
}

impl ColorPicker {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, value: Hsla) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            on_change: None,
        }
    }

    pub fn on_change(mut self, f: impl Fn(Hsla, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn width(theme: &Theme) -> Pixels {
        px(2.0) + theme.space(1.0) + SWATCH + theme.space(1.5) + hex_width(theme) + theme.space(2.0)
    }
}

fn hex_width(theme: &Theme) -> Pixels {
    (theme.typography.mono * 4.4).round()
}

impl RenderOnce for ColorPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let picker = window
            .use_keyed_state((self.id.clone(), "picker"), cx, |_, _| {
                Rc::new(Picker::new())
            })
            .read(cx)
            .clone();
        picker.sync(self.value);
        *picker.on_change.borrow_mut() = self.on_change;
        let label = self.label.clone();
        let hex = window.use_keyed_state((self.id.clone(), "hex"), cx, {
            let picker = picker.clone();
            move |window, cx| {
                let input = cx.new(|cx| {
                    Input::new(format!("{label} hex code"), cx)
                        .inline()
                        .max_length(7)
                });
                let changes =
                    cx.subscribe_in(&input, window, move |_, input, event, window, cx| {
                        let focused = input.read(cx).focus_handle(cx).is_focused(window);
                        if !focused || !matches!(event, InputEvent::Changed) {
                            return;
                        }
                        if let Some(color) = parse_hex(input.read(cx).text()) {
                            picker.hsv.set(Hsv::of(color, picker.hsv.get().h));
                            picker.commit(window, cx);
                        }
                    });
                Hex {
                    input,
                    _changes: changes,
                }
            }
        });
        let input = hex.read(cx).input.clone();
        let shown = picker.hsv.get().color();
        let text = to_hex(shown);
        let editing = input.read(cx).focus_handle(cx).is_focused(window);
        if !editing && input.read(cx).text() != text.as_ref() {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        let theme = cx.theme().clone();
        let typography = theme.typography;
        let height = theme.layout.control_sm;
        let swatch = Button::new(
            (self.id.clone(), "swatch"),
            format!("{} picker", self.label),
        )
        .variant(super::button::Variant::Ghost)
        .size(Size::XSmall)
        .swatch()
        .w(SWATCH)
        .h(SWATCH)
        .p_0()
        .child(div())
        .attach(
            div()
                .absolute()
                .inset_0()
                .rounded_full()
                .bg(shown)
                .border_1()
                .border_color(theme.foreground.opacity(0.15))
                .into_any_element(),
        );
        let width = Self::width(&theme);
        let swatch_end = px(1.0) + theme.space(1.0) + SWATCH;
        let id = self.id.clone();
        let popover = Popover::new((self.id.clone(), "popover"))
            .trigger(swatch)
            .placement(Placement {
                side_offset: px(4.0) + (height - SWATCH) / 2.0,
                align_offset: width - swatch_end,
                ..Placement::new(Side::Bottom, Align::End)
            })
            .width(AREA_WIDTH + PANEL_INSET * 2.0)
            .inset(PANEL_INSET)
            .content(move |_, window, cx| panel(&id, &picker, window, cx).into_any_element());
        h_flex()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .w(width)
            .h(height)
            .pl(theme.space(1.0))
            .pr(theme.space(2.0))
            .gap(theme.space(1.5))
            .rounded_full()
            .bg(theme.input_fill())
            .border_1()
            .border_color(if editing { theme.ring } else { theme.input })
            .text_color(theme.foreground)
            .font_family(theme.mono_family.clone())
            .text_size(typography.mono)
            .line_height(typography.secondary_line)
            .font_weight(FontWeight::MEDIUM)
            .child(popover)
            .child(div().w(hex_width(&theme)).child(input))
            .when(editing, |pill| {
                pill.child(theme.ring_around(theme.ring, height / 2.0, px(1.0)))
            })
    }
}

fn panel(
    id: &ElementId,
    picker: &Rc<Picker>,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let hsv = picker.hsv.get();
    let square_focus = nav::focus_handle((id.clone(), "square"), window, cx);
    let hue_focus = nav::focus_handle((id.clone(), "hue"), window, cx);
    let square_ring = nav::keyboard_focused(&square_focus, window, cx);
    let hue_ring = nav::keyboard_focused(&hue_focus, window, cx);
    let theme = cx.theme().clone();
    let corner = theme.radius.panel - PANEL_INSET;
    let pure = Hsv {
        h: hsv.h,
        s: 1.0,
        v: 1.0,
    }
    .color();
    let thumb = |size: Pixels, fill: Hsla, ring: bool| {
        div()
            .absolute()
            .size(size)
            .ml(-size / 2.0)
            .rounded_full()
            .border_2()
            .border_color(gpui::white())
            .bg(fill)
            .shadow(vec![
                gpui::BoxShadow {
                    color: gpui::black().opacity(0.12),
                    offset: gpui::point(px(0.0), px(0.0)),
                    blur_radius: px(0.0),
                    spread_radius: px(1.0),
                    inset: false,
                },
                gpui::BoxShadow {
                    color: gpui::black().opacity(0.24),
                    offset: gpui::point(px(0.0), px(1.0)),
                    blur_radius: px(1.5),
                    spread_radius: px(0.0),
                    inset: false,
                },
            ])
            .when(ring, |thumb| {
                thumb.child(theme.halo(theme.ring, size / 2.0, px(2.0), px(2.0)))
            })
    };
    let square = div()
        .id("square")
        .role(Role::Slider)
        .aria_label("Saturation and brightness")
        .aria_value(SharedString::from(format!(
            "Saturation {}%, brightness {}%",
            (hsv.s * 100.0).round(),
            (hsv.v * 100.0).round()
        )))
        .track_focus(&square_focus.clone().tab_stop(true))
        .relative()
        .w(AREA_WIDTH)
        .h(SQUARE_HEIGHT)
        .rounded_t(corner)
        .bg(pure)
        .pointer_cursor(&theme)
        .on_key_down({
            let picker = picker.clone();
            move |event: &KeyDownEvent, window, cx| {
                let step = if event.keystroke.modifiers.shift {
                    0.1
                } else {
                    0.01
                };
                let mut hsv = picker.hsv.get();
                match event.keystroke.key.as_str() {
                    "left" => hsv.s = (hsv.s - step).max(0.0),
                    "right" => hsv.s = (hsv.s + step).min(1.0),
                    "down" => hsv.v = (hsv.v - step).max(0.0),
                    "up" => hsv.v = (hsv.v + step).min(1.0),
                    _ => return,
                }
                picker.hsv.set(hsv);
                picker.commit(window, cx);
                window.refresh();
                cx.stop_propagation();
            }
        })
        .on_mouse_down(MouseButton::Left, {
            let picker = picker.clone();
            move |event, window, cx| {
                window.focus(&square_focus, cx);
                picker.dragging.set(Some(Area::Square));
                picker.point(Area::Square, event.position);
                window.refresh();
            }
        })
        .on_mouse_up(MouseButton::Left, release(picker))
        .on_mouse_up_out(MouseButton::Left, release(picker))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_t(corner)
                .bg(linear_gradient(
                    90.0,
                    linear_color_stop(gpui::white(), 0.0),
                    linear_color_stop(gpui::white().opacity(0.0), 1.0),
                )),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_t(corner)
                .bg(linear_gradient(
                    180.0,
                    linear_color_stop(gpui::black().opacity(0.0), 0.0),
                    linear_color_stop(gpui::black(), 1.0),
                )),
        )
        .child(tracker(picker, Area::Square).absolute().inset_0())
        .child(
            thumb(SQUARE_THUMB, hsv.color(), square_ring)
                .left(relative(hsv.s))
                .top(relative(1.0 - hsv.v))
                .mt(-SQUARE_THUMB / 2.0),
        );
    let stops =
        [0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0].map(|h: f32| hsla(h / 360.0, 1.0, 0.5, 1.0));
    let red = stops[0];
    let hue = div()
        .id("hue")
        .role(Role::Slider)
        .aria_label("Hue")
        .aria_numeric_value(hsv.h.round() as f64)
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(360.0)
        .track_focus(&hue_focus.clone().tab_stop(true))
        .relative()
        .w(AREA_WIDTH)
        .h(HUE_HEIGHT)
        .pointer_cursor(&theme)
        .on_key_down({
            let picker = picker.clone();
            move |event: &KeyDownEvent, window, cx| {
                let step = if event.keystroke.modifiers.shift {
                    10.0
                } else {
                    1.0
                };
                let mut hsv = picker.hsv.get();
                match event.keystroke.key.as_str() {
                    "left" | "down" => hsv.h = (hsv.h - step).max(0.0),
                    "right" | "up" => hsv.h = (hsv.h + step).min(360.0),
                    _ => return,
                }
                picker.hsv.set(hsv);
                picker.commit(window, cx);
                window.refresh();
                cx.stop_propagation();
            }
        })
        .on_mouse_down(MouseButton::Left, {
            let picker = picker.clone();
            move |event, window, cx| {
                window.focus(&hue_focus, cx);
                picker.dragging.set(Some(Area::Hue));
                picker.point(Area::Hue, event.position);
                window.refresh();
            }
        })
        .on_mouse_up(MouseButton::Left, release(picker))
        .on_mouse_up_out(MouseButton::Left, release(picker))
        .child(
            h_flex()
                .size_full()
                .child(div().w(HUE_THUMB / 2.0).h_full().rounded_bl(corner).bg(red))
                .children(stops.windows(2).map(|pair| {
                    div().flex_1().h_full().bg(linear_gradient(
                        90.0,
                        linear_color_stop(pair[0], 0.0),
                        linear_color_stop(pair[1], 1.0),
                    ))
                }))
                .child(div().w(HUE_THUMB / 2.0).h_full().rounded_br(corner).bg(red)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(HUE_THUMB / 2.0)
                .right(HUE_THUMB / 2.0)
                .child(tracker(picker, Area::Hue).absolute().inset_0())
                .child(
                    thumb(HUE_THUMB, pure, hue_ring)
                        .top_0()
                        .left(relative(hsv.h / 360.0)),
                ),
        );
    v_flex().child(square).child(hue)
}

fn release(picker: &Rc<Picker>) -> impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static {
    let picker = picker.clone();
    move |event, window, cx| {
        if let Some(area) = picker.dragging.take() {
            picker.point(area, event.position);
            picker.commit(window, cx);
            window.refresh();
        }
    }
}

fn tracker(picker: &Rc<Picker>, area: Area) -> gpui::Canvas<()> {
    let measure = picker.clone();
    let follow = picker.clone();
    canvas(
        move |bounds, _, _| match area {
            Area::Square => measure.square.set(bounds),
            Area::Hue => measure.hue.set(bounds),
        },
        move |_, _, window, _| {
            if follow.dragging.get() != Some(area) {
                return;
            }
            let moved = follow.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, _, window, _| {
                if event.pressed_button == Some(MouseButton::Left) {
                    moved.point(area, event.position);
                    window.refresh();
                }
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::Hsv;
    use crate::theme::{parse_hex, to_hex};

    #[test]
    fn hex_codes_survive_the_trip_through_hue_saturation_and_value() {
        for hex in [
            "#000000", "#FFFFFF", "#7F7F7F", "#FF0000", "#1E90FF", "#3A5F0B",
        ] {
            let color = parse_hex(hex).unwrap();
            assert_eq!(to_hex(Hsv::of(color, 0.0).color()), hex);
        }
    }

    #[test]
    fn a_gray_keeps_the_hue_the_picker_was_on() {
        let gray = parse_hex("#808080").unwrap();
        assert_eq!(Hsv::of(gray, 210.0).h, 210.0);
    }
}
