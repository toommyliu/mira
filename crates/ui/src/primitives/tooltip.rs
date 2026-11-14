use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    Anchor, AnyElement, App, Bounds, Div, ElementId, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, anchored, canvas, deferred, div, point,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _,
    theme::{LINE_SMALL, TEXT_SMALL},
};

const DELAY: Duration = Duration::from_millis(450);
const WARM: Duration = Duration::from_millis(400);
const GAP: Pixels = px(6.0);
const MARGIN: Pixels = px(8.0);
const HEIGHT: Pixels = px(26.0);
const INSET: Pixels = px(9.0);

#[derive(IntoElement)]
pub struct Tooltip {
    id: ElementId,
    label: SharedString,
    trigger: AnyElement,
    start: bool,
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
            start: false,
        }
    }

    pub fn align_start(mut self) -> Self {
        self.start = true;
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
        let id = ElementId::from((self.id, "tooltip"));
        let popup_id = id.clone();
        let (label, start) = (self.label, self.start);
        hover(trigger, id, false, window, cx, move |at, window, cx| {
            popup(popup_id, label, at, start, window, cx)
        })
    }
}

#[derive(Default)]
struct LastClosed(Option<Instant>);

impl gpui::Global for LastClosed {}

#[derive(Default)]
struct Tip {
    hovered: bool,
    shown: bool,
    generation: u64,
    trigger: Rc<Cell<Bounds<Pixels>>>,
}

pub fn attach(
    trigger: Stateful<Div>,
    id: impl Into<ElementId>,
    label: SharedString,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let id = id.into();
    let popup_id = id.clone();
    hover(trigger, id, focused, window, cx, move |at, window, cx| {
        popup(popup_id, label, at, false, window, cx)
    })
}

pub(crate) fn hover(
    trigger: Stateful<Div>,
    id: ElementId,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
    popup: impl FnOnce(Bounds<Pixels>, &Window, &App) -> AnyElement,
) -> Stateful<Div> {
    let tip = window.use_keyed_state(id.clone(), cx, |_, _| Tip::default());
    let (shown, bounds) = {
        let tip = tip.read(cx);
        (tip.shown || focused, tip.trigger.clone())
    };
    let at = bounds.get();
    let hover = tip.clone();
    let press = tip;
    trigger
        .on_hover(move |hovered, _, cx| {
            let hovered = *hovered;
            let generation = hover.update(cx, |tip, cx| {
                tip.hovered = hovered;
                tip.generation += 1;
                if !hovered && tip.shown {
                    tip.shown = false;
                    cx.default_global::<LastClosed>().0 = Some(Instant::now());
                    cx.notify();
                }
                tip.generation
            });
            if !hovered {
                return;
            }
            if cx
                .default_global::<LastClosed>()
                .0
                .is_some_and(|at| at.elapsed() < WARM)
            {
                hover.update(cx, |tip, cx| {
                    tip.shown = true;
                    cx.notify();
                });
                return;
            }
            let tip = hover.downgrade();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(DELAY).await;
                tip.update(cx, |tip, cx| {
                    if tip.hovered && tip.generation == generation {
                        tip.shown = true;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            press.update(cx, |tip, cx| {
                tip.hovered = false;
                tip.generation += 1;
                if tip.shown {
                    tip.shown = false;
                    cx.default_global::<LastClosed>().0 = Some(Instant::now());
                    cx.notify();
                }
            });
        })
        .child(
            canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        )
        .when(shown, |trigger| trigger.child(popup(at, window, cx)))
}

fn popup(
    id: ElementId,
    label: SharedString,
    trigger: Bounds<Pixels>,
    start: bool,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let room_above = trigger.top() - GAP - HEIGHT >= MARGIN;
    let room_below = trigger.bottom() + GAP + HEIGHT <= window.viewport_size().height - MARGIN;
    let above = room_above || !room_below;
    let x = if start {
        trigger.left() - INSET
    } else {
        trigger.center().x
    };
    let (position, anchor) = match (above, start) {
        (true, false) => (point(x, trigger.top() - GAP), Anchor::BottomCenter),
        (true, true) => (point(x, trigger.top() - GAP), Anchor::BottomLeft),
        (false, false) => (point(x, trigger.bottom() + GAP), Anchor::TopCenter),
        (false, true) => (point(x, trigger.bottom() + GAP), Anchor::TopLeft),
    };
    deferred(
        anchored()
            .position(position)
            .anchor(anchor)
            .snap_to_window_with_margin(MARGIN)
            .child(
                div()
                    .id((id, "tooltip"))
                    .py(px(4.0))
                    .px(px(8.0))
                    .rounded(theme.radius.control)
                    .bg(theme.overlay)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(theme.floating())
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_SMALL)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .whitespace_nowrap()
                    .child(label),
            ),
    )
    .with_priority(super::layer::TOOLTIP)
    .into_any_element()
}
