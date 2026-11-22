use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, prelude::FluentBuilder as _, px,
};

use super::label::{ToggleHandler, caption, labelled};
use crate::{ActiveTheme as _, motion, theme::mix};

const TRACK_W: f32 = 28.0;
const TRACK_H: f32 = 16.0;
const THUMB: f32 = 14.0;
const DRAG: Pixels = px(6.0);

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    small: bool,
    label: SharedString,
    description: Option<SharedString>,
    show_label: bool,
    trailing: bool,
    end: Option<AnyElement>,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            small: false,
            label: label.into(),
            description: None,
            show_label: true,
            trailing: false,
            end: None,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn hide_label(mut self) -> Self {
        self.show_label = false;
        self
    }

    pub fn trailing(mut self) -> Self {
        self.trailing = true;
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = motion::travel(
            (self.id.clone(), "thumb"),
            if self.on { 1.0 } else { 0.0 },
            window,
            cx,
        );
        let theme = cx.theme().clone();
        let on = self.on;
        let (row, ring) = labelled(
            self.id.clone(),
            self.label.clone(),
            self.description.clone(),
            self.disabled,
            window,
            cx,
        );
        let drag_start = window
            .use_keyed_state((self.id.clone(), "drag"), cx, |_, _| {
                Rc::new(Cell::new(None))
            })
            .read(cx)
            .clone();
        let control = track(self.small, t, &theme)
            .id("track")
            .mt(px((20.0 - if self.small { 14.0 } else { TRACK_H }) / 2.0))
            .when(ring, |control| {
                control.border_color(theme.ring).child(theme.ring_around(
                    theme.ring,
                    px(TRACK_H / 2.0),
                    px(1.0),
                ))
            });
        let label = self
            .show_label
            .then(|| caption(self.label, self.description, cx));
        let end = self.end.map(|end| div().flex_none().child(end));
        row.role(Role::Switch)
            .group("switch")
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .map(|row| {
                if self.trailing {
                    row.children(label).children(end).child(control)
                } else {
                    row.child(control).children(label).children(end)
                }
            })
            .when_some(
                self.on_toggle.filter(|_| !self.disabled),
                |row, on_toggle| {
                    let (down, click, outside) =
                        (drag_start.clone(), drag_start.clone(), drag_start);
                    let released_outside = on_toggle.clone();
                    row.on_mouse_down(MouseButton::Left, move |event, _, _| {
                        down.set(Some(event.position.x))
                    })
                    .on_click(move |event, window, cx| {
                        click.set(None);
                        let next = match event {
                            ClickEvent::Mouse(press) => {
                                dragged_to(press.down.position.x, press.up.position.x)
                                    .unwrap_or(!on)
                            }
                            _ => !on,
                        };
                        if next != on {
                            on_toggle(next, window, cx);
                        }
                    })
                    .on_mouse_up_out(
                        MouseButton::Left,
                        move |event, window, cx| {
                            if let Some(next) = outside
                                .take()
                                .and_then(|start| dragged_to(start, event.position.x))
                                .filter(|next| *next != on)
                            {
                                released_outside(next, window, cx);
                            }
                        },
                    )
                },
            )
    }
}

pub(super) fn track(small: bool, t: f32, theme: &crate::Theme) -> gpui::Div {
    let (w, h, thumb) = if small {
        (24.0, 14.0, 12.0)
    } else {
        (TRACK_W, TRACK_H, THUMB)
    };
    div()
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .w(px(w))
        .h(px(h))
        .rounded_full()
        .border_1()
        .border_color(gpui::transparent_black())
        .bg(mix(off_fill(theme), theme.primary, t))
        .child(
            div()
                .flex_none()
                .size(px(thumb))
                .rounded_full()
                .ml(px((w - thumb - 2.0) * t))
                .bg(if theme.dark_mode {
                    mix(theme.foreground, theme.primary_foreground, t)
                } else {
                    theme.background
                }),
        )
}

fn off_fill(theme: &crate::Theme) -> gpui::Hsla {
    theme.input.opacity(if theme.dark_mode { 0.8 } else { 1.0 })
}

fn dragged_to(start: Pixels, end: Pixels) -> Option<bool> {
    let distance = end - start;
    (distance.abs() >= DRAG).then_some(distance > Pixels::ZERO)
}
