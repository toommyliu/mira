use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, prelude::FluentBuilder as _,
    px, relative,
};

use crate::{
    ActiveTheme as _, PointerCursor as _, nav,
    theme::{TEXT_SMALL, tabular},
};

type ValueHandler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    label: SharedString,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    format: Rc<dyn Fn(f32) -> SharedString>,
    on_change: Option<ValueHandler<f32>>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, value: f32) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            min: 0.0,
            max: 1.0,
            step: 0.01,
            format: Rc::new(|value| format!("{value:.2}").into()),
            on_change: None,
        }
    }

    pub fn range(mut self, min: f32, max: f32, step: f32) -> Self {
        self.min = min;
        self.max = max;
        self.step = step;
        self
    }

    pub fn format(mut self, f: impl Fn(f32) -> SharedString + 'static) -> Self {
        self.format = Rc::new(f);
        self
    }

    pub fn on_change(mut self, f: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

#[derive(Default)]
struct Drag {
    track: Cell<Bounds<Pixels>>,
    dragging: Cell<bool>,
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let drag = window
            .use_keyed_state((self.id.clone(), "drag"), cx, |_, _| {
                Rc::new(Drag::default())
            })
            .read(cx)
            .clone();
        let ring = nav::keyboard_focused(&focus, window);
        let theme = cx.theme().clone();
        let (value, min, max, step) = (self.value, self.min, self.max, self.step);
        let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0);
        let snap = move |raw: f32| {
            ((raw - min) / step)
                .round()
                .mul_add(step, min)
                .clamp(min, max)
        };
        let set = {
            let on_change = self.on_change.clone();
            move |next: f32, window: &mut Window, cx: &mut App| {
                let next = snap(next);
                if (next - value).abs() > f32::EPSILON
                    && let Some(on_change) = &on_change
                {
                    on_change(next, window, cx);
                }
            }
        };
        let at_x = {
            let drag = drag.clone();
            move |x: Pixels| {
                let track = drag.track.get();
                let t = ((x - track.left()) / track.size.width).clamp(0.0, 1.0);
                min + t * (max - min)
            }
        };
        let label = (self.format)(value);
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .w_full()
            .child(
                div()
                    .id(self.id.clone())
                    .role(Role::Slider)
                    .aria_label(self.label)
                    .aria_numeric_value(value as f64)
                    .aria_min_numeric_value(min as f64)
                    .aria_max_numeric_value(max as f64)
                    .aria_numeric_value_step(step as f64)
                    .aria_value(label.clone())
                    .on_a11y_action(gpui::AccessibleAction::Increment, {
                        let set = set.clone();
                        move |_, window, cx| set(value + step, window, cx)
                    })
                    .on_a11y_action(gpui::AccessibleAction::Decrement, {
                        let set = set.clone();
                        move |_, window, cx| set(value - step, window, cx)
                    })
                    .track_focus(&focus.clone().tab_stop(true))
                    .on_key_down({
                        let set = set.clone();
                        move |event: &KeyDownEvent, window, cx| {
                            let big = (max - min) / 10.0;
                            let next = match event.keystroke.key.as_str() {
                                "right" | "up" => value + step,
                                "left" | "down" => value - step,
                                "pageup" => value + big,
                                "pagedown" => value - big,
                                "home" => min,
                                "end" => max,
                                _ => return,
                            };
                            set(next, window, cx);
                            cx.stop_propagation();
                        }
                    })
                    .on_mouse_down(MouseButton::Left, {
                        let set = set.clone();
                        let drag = drag.clone();
                        let at_x = at_x.clone();
                        let focus = focus.clone();
                        move |event, window, cx| {
                            window.focus(&focus, cx);
                            drag.dragging.set(true);
                            set(at_x(event.position.x), window, cx);
                        }
                    })
                    .flex_1()
                    .h(px(20.0))
                    .relative()
                    .pointer_cursor(&theme)
                    .child(
                        canvas(
                            {
                                let drag = drag.clone();
                                move |bounds, _, _| drag.track.set(bounds)
                            },
                            {
                                let drag = drag.clone();
                                move |_, _, window, _| {
                                    if !drag.dragging.get() {
                                        return;
                                    }
                                    let set = set.clone();
                                    let at_x = at_x.clone();
                                    window.on_mouse_event(
                                        move |event: &MouseMoveEvent, _, window, cx| {
                                            if event.pressed_button == Some(MouseButton::Left) {
                                                set(at_x(event.position.x), window, cx);
                                            }
                                        },
                                    );
                                    let drag = drag.clone();
                                    window.on_mouse_event(move |_: &MouseUpEvent, _, window, _| {
                                        drag.dragging.set(false);
                                        window.refresh();
                                    });
                                }
                            },
                        )
                        .absolute()
                        .top(px(8.0))
                        .left(px(7.0))
                        .right(px(7.0))
                        .h(px(4.0)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(8.0))
                            .left(px(7.0))
                            .right(px(7.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme.border)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_full()
                                    .bg(theme.text_secondary),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(7.0))
                            .right(px(7.0))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(3.0))
                                    .left(relative(fraction))
                                    .ml(px(-7.0))
                                    .size(px(14.0))
                                    .rounded_full()
                                    .bg(theme.knob)
                                    .border_1()
                                    .border_color(theme.border_strong)
                                    .shadow(theme.raised())
                                    .when(ring, |thumb| {
                                        thumb.child(theme.ring_around(
                                            theme.focus,
                                            px(7.0),
                                            px(1.0),
                                        ))
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .min_w(px(44.0))
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text_secondary)
                    .font_features(tabular())
                    .child(label),
            )
    }
}
