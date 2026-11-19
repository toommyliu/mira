use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, RenderOnce, SharedString, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, deferred, div,
    prelude::FluentBuilder as _, px,
};

use super::{Align, Placement, Side, placement::COLLISION_MARGIN};
use crate::{ActiveTheme as _, Theme, motion::From};

#[derive(IntoElement)]
pub struct Tooltip {
    id: ElementId,
    label: SharedString,
    trigger: AnyElement,
    placement: Placement,
}

impl Tooltip {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        trigger: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            trigger: trigger.into_any_element(),
            placement: Placement::new(Side::Top, Align::Center),
        }
    }

    pub fn align_start(mut self) -> Self {
        self.placement.align = Align::Start;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
}

impl RenderOnce for Tooltip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let trigger = div()
            .id(self.id.clone())
            .relative()
            .min_w_0()
            .aria_description(self.label.clone())
            .child(self.trigger);
        hover(
            trigger,
            (self.id, "tooltip").into(),
            false,
            self.placement,
            window,
            cx,
            move |_, from, theme| panel(self.label, from, theme),
        )
    }
}

#[derive(Default)]
struct Tip {
    over_trigger: bool,
    over_panel: bool,
    focused: bool,
    shown: bool,
    dismissed: bool,
    close: Option<Task<()>>,
    trigger: Rc<Cell<Bounds<Pixels>>>,
    size: Rc<Cell<Size<Pixels>>>,
}

impl Tip {
    fn pointer(&mut self, over_trigger: bool, entered: bool, cx: &mut Context<Self>) {
        if over_trigger {
            self.over_trigger = entered;
            if entered {
                self.dismissed = false;
            }
        } else {
            self.over_panel = entered;
        }
        if self.over_trigger || self.over_panel {
            self.close = None;
            self.shown = true;
        } else {
            // Keep the popup mounted while the pointer crosses the gap beside its anchor.
            self.close = Some(cx.spawn(async move |tip, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let _ = tip.update(cx, |tip, cx| {
                    tip.shown = false;
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }
}

pub fn attach(
    trigger: Stateful<Div>,
    id: impl Into<ElementId>,
    label: SharedString,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    hover(
        trigger,
        id.into(),
        focused,
        Placement::new(Side::Top, Align::Center),
        window,
        cx,
        move |_, from, theme| panel(label, from, theme),
    )
}

pub(crate) fn hover<E: IntoElement>(
    trigger: Stateful<Div>,
    id: ElementId,
    focused: bool,
    placement: Placement,
    window: &mut Window,
    cx: &mut App,
    content: impl FnOnce(Bounds<Pixels>, From, &Theme) -> E,
) -> Stateful<Div> {
    let tip = window.use_keyed_state(id.clone(), cx, |_, _| Tip::default());
    let (shown, bounds, measured) = tip.update(cx, |tip, _| {
        if focused && !tip.focused {
            tip.dismissed = false;
        }
        tip.focused = focused;
        (
            (tip.shown || focused) && !tip.dismissed,
            tip.trigger.clone(),
            tip.size.clone(),
        )
    });
    let at = bounds.get();
    let hover = tip.clone();
    let press = tip.clone();
    let escape = tip.clone();
    trigger
        .on_hover(move |over, _, cx| hover.update(cx, |tip, cx| tip.pointer(true, *over, cx)))
        .on_key_down(move |event, _, cx| {
            if shown && event.keystroke.key == "escape" {
                escape.update(cx, |tip, cx| {
                    tip.dismissed = true;
                    tip.over_panel = false;
                    cx.notify();
                });
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            press.update(cx, |tip, cx| {
                tip.dismissed = true;
                tip.over_panel = false;
                cx.notify();
            })
        })
        .child(
            canvas(
                move |area, window, _| {
                    if bounds.replace(area) != area && shown {
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .when(shown, |trigger| {
            let viewport = window.viewport_size();
            let (positioner, from) = placement.resolve(at, measured.get(), viewport);
            let panel = div()
                .id((id, "floating-help"))
                .relative()
                .max_w(viewport.width - COLLISION_MARGIN * 2.0)
                .on_hover(move |over, _, cx| {
                    tip.update(cx, |tip, cx| tip.pointer(false, *over, cx))
                })
                .child(content(at, from, cx.theme()))
                .child(
                    canvas(
                        move |area, window, _| {
                            if measured.replace(area.size) != area.size {
                                window.request_animation_frame();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                );
            trigger.child(deferred(positioner.child(panel)).with_priority(super::layer::TOOLTIP))
        })
}

fn panel(label: SharedString, from: From, theme: &Theme) -> impl IntoElement + use<> {
    let panel = div()
        .max_w(px(320.0))
        .py(theme.space(1.5))
        .px(theme.space(3.0))
        .rounded(theme.radius.control)
        .bg(theme.foreground)
        .text_color(theme.background)
        .text_size(theme.typography.body)
        .line_height(theme.typography.secondary_line)
        .child(label);
    crate::motion::arrive_for("tooltip-enter", panel, from, crate::motion::TRAVEL)
}
