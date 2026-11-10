use std::{cell::Cell, rc::Rc};

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, prelude::FluentBuilder as _, px,
};

use super::label::{ToggleHandler, caption, labelled};
use crate::{ActiveTheme as _, motion, theme::mix};

const TRACK_W: f32 = 28.0;
const TRACK_H: f32 = 16.0;
const THUMB: f32 = 12.0;
const STRETCH: f32 = 4.0;
const DRAG: Pixels = px(6.0);
const SWITCH_GROUP: &str = "switch";

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    label: SharedString,
    description: Option<SharedString>,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            label: label.into(),
            description: None,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
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
        let track = mix(theme.border_strong, theme.accent, t);
        let on = self.on;
        let (row, ring) = labelled(
            self.id.clone(),
            self.label.clone(),
            self.description.clone(),
            self.disabled,
            window,
            cx,
        );
        let travel = (TRACK_W - THUMB - 4.0) * t;
        let drag_start = window
            .use_keyed_state((self.id.clone(), "drag"), cx, |_, _| {
                Rc::new(Cell::new(None))
            })
            .read(cx)
            .clone();
        let pressable = !self.disabled;
        row.role(Role::Switch)
            .group(SWITCH_GROUP)
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .child(
                div()
                    .flex_none()
                    .mt(px(1.0))
                    .w(px(TRACK_W))
                    .h(px(TRACK_H))
                    .p(px(2.0))
                    .rounded_full()
                    .bg(track)
                    .when(ring, |control| control.child(theme.focus_ring(px(8.0))))
                    .child(
                        div()
                            .id("thumb")
                            .ml(px(travel))
                            .size(px(THUMB))
                            .when(pressable, |thumb| {
                                // GPUI tracks a group press only for elements with their own hitbox.
                                // The empty hover style gives the thumb one.
                                thumb.group_hover(SWITCH_GROUP, |style| style).group_active(
                                    SWITCH_GROUP,
                                    move |style| {
                                        style.w(px(THUMB + STRETCH)).ml(px(travel - STRETCH * t))
                                    },
                                )
                            })
                            .rounded_full()
                            .bg(theme.knob)
                            .shadow(theme.raised()),
                    ),
            )
            .child(caption(self.label, self.description, cx))
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

fn dragged_to(start: Pixels, end: Pixels) -> Option<bool> {
    let distance = end - start;
    (distance.abs() >= DRAG).then_some(distance > Pixels::ZERO)
}
