use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, Global, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, RenderOnce, SharedString, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Task, WeakEntity, Window, canvas, deferred, div,
    prelude::FluentBuilder as _, px,
};

use super::{Align, Placement, Side, hover_card::Card, placement::COLLISION_MARGIN};
use crate::{ActiveTheme as _, Theme, motion};

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
            Help::Label(self.label),
            false,
            self.placement,
            window,
            cx,
        )
    }
}

pub(crate) enum Help {
    Label(SharedString),
    Card(Card),
}

impl Help {
    fn kind(&self) -> Kind {
        match self {
            Help::Label(_) => Kind::Tooltip,
            Help::Card(_) => Kind::Card,
        }
    }

    fn panel(self, theme: &Theme) -> Div {
        match self {
            Help::Label(label) => panel(label, theme),
            Help::Card(card) => super::hover_card::popup(card, theme),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Tooltip,
    Card,
}

impl Kind {
    fn open_delay(self) -> Duration {
        match self {
            Kind::Tooltip => Duration::ZERO,
            Kind::Card => Duration::from_millis(600),
        }
    }

    fn arrival(self) -> Duration {
        match self {
            Kind::Tooltip => motion::TRAVEL,
            Kind::Card => motion::ARRIVE,
        }
    }
}

const WARM: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Group {
    tip: Option<WeakEntity<Tip>>,
    closed: Option<Instant>,
}

#[derive(Default)]
struct Showing {
    tooltip: Group,
    card: Group,
}

impl Global for Showing {}

impl Showing {
    fn group(&mut self, kind: Kind) -> &mut Group {
        match kind {
            Kind::Tooltip => &mut self.tooltip,
            Kind::Card => &mut self.card,
        }
    }
}

enum Phase {
    Hidden,
    Waiting {
        _timer: Task<()>,
    },
    Open {
        entered: Option<Instant>,
    },
    Leaving {
        since: Instant,
        entrance: f32,
        _timer: Task<()>,
    },
}

impl Phase {
    fn on_screen(&self) -> bool {
        matches!(self, Phase::Open { .. } | Phase::Leaving { .. })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Over {
    Trigger,
    Panel,
}

struct Tip {
    kind: Kind,
    phase: Phase,
    over_trigger: bool,
    over_panel: bool,
    focused: bool,
    dismissed: bool,
    trigger: Rc<Cell<Bounds<Pixels>>>,
    size: Rc<Cell<Size<Pixels>>>,
}

impl Tip {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            phase: Phase::Hidden,
            over_trigger: false,
            over_panel: false,
            focused: false,
            dismissed: false,
            trigger: Rc::default(),
            size: Rc::default(),
        }
    }

    fn pointer(&mut self, over: Over, entered: bool, cx: &mut Context<Self>) {
        match over {
            Over::Trigger => {
                self.over_trigger = entered;
                if entered {
                    self.dismissed = false;
                }
            }
            Over::Panel => self.over_panel = entered,
        }
        let hovered = self.over_trigger || self.over_panel;
        let next = match self.phase {
            Phase::Hidden | Phase::Leaving { .. } if over == Over::Trigger && entered => {
                Some(self.approach(cx))
            }
            Phase::Waiting { .. } if !hovered => Some(Phase::Hidden),
            Phase::Open { .. } if !hovered => Some(self.close(cx)),
            _ => None,
        };
        if let Some(next) = next {
            self.phase = next;
        }
        cx.notify();
    }

    fn approach(&mut self, cx: &mut Context<Self>) -> Phase {
        let delay = self.kind.open_delay();
        if delay.is_zero() || self.warm(cx) {
            self.open(cx)
        } else {
            Phase::Waiting {
                _timer: after(delay, |tip, cx| tip.phase = tip.open(cx), cx),
            }
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) -> Phase {
        let entered = (!self.focused && !cx.reduce_motion() && !self.warm(cx))
            .then(|| cx.background_executor().now());
        let me = cx.weak_entity();
        let previous = cx
            .default_global::<Showing>()
            .group(self.kind)
            .tip
            .replace(me.clone())
            .filter(|previous| previous != &me)
            .and_then(|previous| previous.upgrade());
        if let Some(previous) = previous {
            previous.update(cx, |tip, cx| {
                tip.phase = Phase::Hidden;
                cx.notify();
            });
        }
        Phase::Open { entered }
    }

    fn entrance(&self, cx: &App) -> f32 {
        match self.phase {
            Phase::Open {
                entered: Some(start),
            } if !self.focused && !cx.reduce_motion() => {
                let elapsed = cx.background_executor().now() - start;
                motion::ease(elapsed.as_secs_f32() / self.kind.arrival().as_secs_f32())
            }
            _ => 1.0,
        }
    }

    fn warm(&self, cx: &mut Context<Self>) -> bool {
        let now = cx.background_executor().now();
        let me = cx.entity_id();
        let group = cx.default_global::<Showing>().group(self.kind);
        let closed_lately = group
            .closed
            .is_some_and(|closed| now.duration_since(closed) < WARM);
        let showing = group.tip.as_ref().and_then(WeakEntity::upgrade);
        closed_lately
            || match showing {
                Some(tip) if tip.entity_id() == me => self.phase.on_screen(),
                Some(tip) => tip.read(cx).phase.on_screen(),
                None => false,
            }
    }

    fn close(&self, cx: &mut Context<Self>) -> Phase {
        let now = cx.background_executor().now();
        cx.default_global::<Showing>().group(self.kind).closed = Some(now);
        self.fade(cx)
    }

    fn fade(&self, cx: &mut Context<Self>) -> Phase {
        if cx.reduce_motion() {
            Phase::Hidden
        } else {
            Phase::Leaving {
                since: cx.background_executor().now(),
                entrance: self.entrance(cx),
                _timer: after(motion::LEAVE, |tip, _| tip.phase = Phase::Hidden, cx),
            }
        }
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.dismissed = true;
        self.over_panel = false;
        match self.phase {
            Phase::Open { .. } => self.phase = self.fade(cx),
            Phase::Waiting { .. } => self.phase = Phase::Hidden,
            Phase::Hidden | Phase::Leaving { .. } => {}
        }
        cx.notify();
    }
}

fn after(
    delay: Duration,
    then: fn(&mut Tip, &mut Context<Tip>),
    cx: &mut Context<Tip>,
) -> Task<()> {
    cx.spawn(async move |tip, cx| {
        cx.background_executor().timer(delay).await;
        let _ = tip.update(cx, |tip, cx| {
            then(tip, cx);
            cx.notify();
        });
    })
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
        Help::Label(label),
        focused,
        Placement::new(Side::Top, Align::Center),
        window,
        cx,
    )
}

pub(crate) fn hover(
    trigger: Stateful<Div>,
    id: ElementId,
    help: Help,
    focused: bool,
    placement: Placement,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let kind = help.kind();
    let tip = window.use_keyed_state(id.clone(), cx, |_, _| Tip::new(kind));
    let (frame, bounds, measured) = tip.update(cx, |tip, cx| {
        if focused && !tip.focused {
            tip.dismissed = false;
        }
        tip.focused = focused;
        let frame = match tip.phase {
            _ if focused && !tip.dismissed => Some((1.0, 1.0)),
            Phase::Open { .. } => {
                let entrance = tip.entrance(cx);
                if entrance < 1.0 {
                    window.request_animation_frame();
                }
                Some((entrance, 1.0))
            }
            Phase::Leaving {
                since, entrance, ..
            } => motion::leaving(since, window, cx).map(|fade| (entrance, fade)),
            Phase::Hidden | Phase::Waiting { .. } => None,
        };
        (frame, tip.trigger.clone(), tip.size.clone())
    });
    let shown = frame.is_some();
    let at = bounds.get();
    let hover = tip.clone();
    let press = tip.clone();
    let escape = tip.clone();
    trigger
        .on_hover(move |over, _, cx| {
            hover.update(cx, |tip, cx| tip.pointer(Over::Trigger, *over, cx))
        })
        .on_key_down(move |event, _, cx| {
            if shown && event.keystroke.key == "escape" {
                escape.update(cx, |tip, cx| tip.dismiss(cx));
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            press.update(cx, |tip, cx| tip.dismiss(cx))
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
        .when_some(frame, |trigger, (entrance, fade)| {
            let viewport = window.viewport_size();
            let (positioner, from) = placement.resolve(at, measured.get(), viewport);
            let theme = cx.theme();
            let content = help
                .panel(theme)
                .font_family(theme.font_family.clone())
                .font_weight(gpui::FontWeight::NORMAL);
            let content = motion::anchored_frame(content, from, entrance);
            let selector = id.to_string();
            let panel = div()
                .debug_selector(|| selector)
                .id((id, "floating-help"))
                .relative()
                .max_w(viewport.width - COLLISION_MARGIN * 2.0)
                .opacity(fade)
                .when(matches!(kind, Kind::Card), |panel| {
                    panel.occlude().on_hover(move |over, _, cx| {
                        tip.update(cx, |tip, cx| tip.pointer(Over::Panel, *over, cx))
                    })
                })
                .child(content)
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

fn panel(label: SharedString, theme: &Theme) -> Div {
    div()
        .max_w(px(320.0))
        .py(theme.space(1.5))
        .px(theme.space(3.0))
        .rounded(theme.radius.control)
        .bg(theme.foreground)
        .text_color(theme.background)
        .text_size(theme.typography.body)
        .line_height(theme.typography.secondary_line)
        .child(label)
}
