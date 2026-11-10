use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, canvas, deferred, div, prelude::FluentBuilder as _, px,
};

use super::{Button, Icon, button::Size};
use crate::{
    ActiveTheme as _, Tone, motion, nav,
    theme::{LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL, mix},
};

const LINGER: Duration = Duration::from_secs(5);
const LIMIT: usize = 3;
const WIDTH: Pixels = px(340.0);
const GAP: Pixels = px(12.0);
const PEEK: Pixels = px(12.0);
const SHRINK: f32 = 0.05;
const UNMOUNT: Duration = Duration::from_millis(200);
const EDGE: Pixels = px(20.0);
const FALLBACK_HEIGHT: Pixels = px(58.0);

type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct Toast {
    title: SharedString,
    body: Option<SharedString>,
    tone: Tone,
    action: Option<(SharedString, ActionHandler)>,
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: None,
            tone: Tone::Neutral,
            action: None,
        }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(f)));
        self
    }

    fn stays(&self) -> bool {
        self.action.is_some() || self.tone == Tone::Danger
    }
}

#[derive(Clone)]
struct Shown {
    id: u64,
    toast: Toast,
    left: Option<Leaving>,
}

#[derive(Clone, Copy)]
struct Leaving {
    at: Instant,
    depth: usize,
    from: Pixels,
    behind: bool,
}

enum Timer {
    Running { deadline: Instant, _task: Task<()> },
    Paused(Duration),
}

#[derive(Default)]
pub struct Toaster {
    next: u64,
    shown: Vec<Shown>,
    hovered: bool,
    expanded: bool,
    timers: HashMap<u64, Timer>,
    heights: Rc<RefCell<HashMap<u64, Pixels>>>,
}

struct Stack(Entity<Toaster>);

impl Global for Stack {}

impl Toaster {
    pub fn init(cx: &mut App) {
        let stack = cx.new(|_| Toaster::default());
        cx.set_global(Stack(stack));
    }

    pub fn push(toast: Toast, cx: &mut App) {
        let stack = cx.global::<Stack>().0.clone();
        let stays = toast.stays();
        let reduce = cx.reduce_motion();
        stack.update(cx, |toasts, cx| {
            toasts.prune(reduce);
            let id = toasts.next;
            toasts.next += 1;
            toasts.shown.insert(
                0,
                Shown {
                    id,
                    toast,
                    left: None,
                },
            );
            if !stays {
                if toasts.expanded {
                    toasts.timers.insert(id, Timer::Paused(LINGER));
                } else {
                    toasts.start(id, LINGER, cx);
                }
            }
            cx.notify();
        });
    }

    fn start(&mut self, id: u64, after: Duration, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |toasts, cx| {
            cx.background_executor().timer(after).await;
            toasts
                .update(cx, |toasts, cx| {
                    toasts.dismiss(id);
                    cx.notify();
                })
                .ok();
        });
        self.timers.insert(
            id,
            Timer::Running {
                deadline: Instant::now() + after,
                _task: task,
            },
        );
    }

    fn open(&mut self, open: bool, cx: &mut Context<Self>) {
        if open == self.expanded {
            return;
        }
        self.expanded = open;
        let now = Instant::now();
        let timers: Vec<(u64, Duration)> = self
            .timers
            .iter()
            .map(|(id, timer)| match timer {
                Timer::Running { deadline, .. } => (*id, deadline.saturating_duration_since(now)),
                Timer::Paused(left) => (*id, *left),
            })
            .collect();
        for (id, left) in timers {
            if open {
                self.timers.insert(id, Timer::Paused(left));
            } else {
                self.start(id, left, cx);
            }
        }
    }

    fn prune(&mut self, reduce: bool) {
        let mut heights = self.heights.borrow_mut();
        self.shown.retain(|shown| {
            let keep = shown
                .left
                .is_none_or(|left| !reduce && left.at.elapsed() < UNMOUNT);
            if !keep {
                heights.remove(&shown.id);
            }
            keep
        });
    }

    fn present(&self) -> impl Iterator<Item = &Shown> {
        self.shown.iter().filter(|shown| shown.left.is_none())
    }

    fn dismiss(&mut self, id: u64) {
        self.timers.remove(&id);
        let present: Vec<u64> = self.present().map(|shown| shown.id).collect();
        let Some(depth) = present.iter().position(|shown| *shown == id) else {
            return;
        };
        let from = {
            let heights = self.heights.borrow();
            let height_of = |id: &u64| heights.get(id).copied().unwrap_or(FALLBACK_HEIGHT);
            let before = present[..depth].iter().map(height_of).sum();
            rest(depth, self.expanded, before, height_of(&present[0]))
        };
        let behind = !self.expanded && depth > 0;
        if let Some(shown) = self.shown.iter_mut().find(|shown| shown.id == id) {
            shown.left = Some(Leaving {
                at: Instant::now(),
                depth,
                from,
                behind,
            });
        }
    }

    pub fn layer(window: &mut Window, cx: &mut App) -> AnyElement {
        let stack = cx.global::<Stack>().0.clone();
        let focus = nav::focus_handle("toasts-focus", window, cx);
        let reduce = cx.reduce_motion();
        let focused = focus.contains_focused(window, cx) && window.last_input_was_keyboard();
        let (shown, expanded, heights) = stack.update(cx, |toasts, cx| {
            toasts.prune(reduce);
            if toasts.shown.is_empty() {
                toasts.hovered = false;
            }
            let leaving = toasts.shown.iter().any(|shown| shown.left.is_some());
            toasts.open(
                toasts.hovered || focused || (leaving && toasts.expanded),
                cx,
            );
            (
                toasts.shown.clone(),
                toasts.expanded,
                toasts.heights.clone(),
            )
        });
        if shown.is_empty() {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let height_of = |id: u64| {
            heights
                .borrow()
                .get(&id)
                .copied()
                .unwrap_or(FALLBACK_HEIGHT)
        };
        let present: Vec<u64> = shown
            .iter()
            .filter(|shown| shown.left.is_none())
            .map(|shown| shown.id)
            .collect();
        let front = present.first().map_or(FALLBACK_HEIGHT, |id| height_of(*id));
        let mut before = Pixels::ZERO;
        let mut slots = HashMap::new();
        for (depth, id) in present.iter().enumerate() {
            slots.insert(
                *id,
                (depth, rest(depth.min(LIMIT), expanded, before, front)),
            );
            if depth < LIMIT {
                before += height_of(*id);
            }
        }
        let visible = present.len().min(LIMIT);
        let stack_height = match (expanded, visible) {
            (_, 0) => front,
            (true, count) => {
                present[..count]
                    .iter()
                    .map(|id| height_of(*id))
                    .sum::<Pixels>()
                    + GAP * (count - 1) as f32
            }
            (false, count) => {
                let deepest = count - 1;
                rest(deepest, false, Pixels::ZERO, front) + scaled(front, deepest)
            }
        };
        let stack_height = px(motion::glide(
            "toasts-height",
            None,
            f32::from(stack_height),
            window,
            cx,
        ));
        let mut cards: Vec<AnyElement> = Vec::new();
        for entry in shown.iter().rev() {
            let id = entry.id;
            let own = height_of(id);
            let (depth, offset, opacity, behind) = match (entry.left, slots.get(&id)) {
                (Some(left), _) if left.depth >= LIMIT => continue,
                (Some(left), _) => {
                    window.request_animation_frame();
                    let (offset, fade) = if left.behind {
                        (-(scaled(front, left.depth) * 0.4), UNMOUNT)
                    } else {
                        (left.from - own, motion::GLIDE)
                    };
                    let t = left.at.elapsed().as_secs_f32() / fade.as_secs_f32();
                    (left.depth, offset, 1.0 - motion::ease(t), left.behind)
                }
                (None, Some(&(depth, offset))) => {
                    let opacity = motion::glide(
                        ("toast-opacity", id),
                        Some(0.0),
                        if depth < LIMIT { 1.0 } else { 0.0 },
                        window,
                        cx,
                    );
                    (depth, offset, opacity, !expanded && depth > 0)
                }
                (None, None) => continue,
            };
            let at = depth.min(LIMIT);
            let unmeasured = entry.left.is_none() && !heights.borrow().contains_key(&id);
            let (offset, inset, tucked, opacity) = if unmeasured {
                (-own, Pixels::ZERO, 0.0, 0.0)
            } else {
                let leaving_behind = entry.left.is_some_and(|left| left.behind);
                let inset = if expanded || leaving_behind {
                    Pixels::ZERO
                } else {
                    WIDTH * SHRINK / 2.0 * at as f32
                };
                let offset = px(motion::glide(
                    ("toast-y", id),
                    Some(f32::from(-own)),
                    f32::from(offset),
                    window,
                    cx,
                ));
                let inset = px(motion::glide(
                    ("toast-x", id),
                    None,
                    f32::from(inset),
                    window,
                    cx,
                ));
                let tucked = motion::glide(
                    ("toast-tucked", id),
                    None,
                    if behind { 1.0 } else { 0.0 },
                    window,
                    cx,
                );
                (offset, inset, tucked, opacity)
            };
            if depth >= LIMIT && opacity <= 0.0 {
                continue;
            }
            let depth = at;
            let height = own + (scaled(front, depth) - own) * tucked;
            let content = 1.0 - tucked;
            let bg = if expanded {
                theme.overlay
            } else {
                mix(
                    theme.overlay,
                    gpui::black(),
                    theme.stack_tint * depth as f32,
                )
            };
            let measured = heights.clone();
            cards.push(
                div()
                    .absolute()
                    .bottom(offset)
                    .left(inset)
                    .right(inset)
                    .when(tucked > 0.0, |slot| slot.h(height))
                    .opacity(opacity)
                    .child(
                        canvas(
                            move |area, window, _| {
                                if tucked > 0.0 {
                                    return;
                                }
                                let previous = measured.borrow_mut().insert(id, area.size.height);
                                if previous != Some(area.size.height) {
                                    window.request_animation_frame();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(card(&stack, entry, content, bg, &theme))
                    .into_any_element(),
            );
        }
        let hover_stack = stack.clone();
        deferred(
            div()
                .id("toasts")
                .track_focus(&focus)
                .absolute()
                .right(EDGE)
                .bottom(EDGE)
                .w(WIDTH)
                .h(stack_height)
                .children(cards)
                .child(div().id("toasts-hover").absolute().inset_0().on_hover(
                    move |hovered, _, cx| {
                        hover_stack.update(cx, |toasts, cx| {
                            toasts.hovered = *hovered;
                            cx.notify();
                        })
                    },
                )),
        )
        .with_priority(super::layer::TOASTS)
        .into_any_element()
    }
}

fn rest(depth: usize, expanded: bool, before: Pixels, front: Pixels) -> Pixels {
    if expanded {
        before + GAP * depth as f32
    } else {
        PEEK * depth as f32 + front * SHRINK / 2.0 * depth as f32
    }
}

fn scaled(front: Pixels, depth: usize) -> Pixels {
    front * (1.0 - SHRINK * depth as f32)
}

fn card(
    stack: &Entity<Toaster>,
    shown: &Shown,
    content: f32,
    bg: gpui::Hsla,
    theme: &crate::Theme,
) -> impl IntoElement + gpui::Styled + 'static {
    let id = shown.id;
    let toast = &shown.toast;
    let (_, _, tone_text) = theme.tone(toast.tone);
    let dismiss = {
        let stack = stack.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            stack.update(cx, |toasts, cx| {
                toasts.dismiss(id);
                cx.notify();
            });
        }
    };
    div()
        .id(("toast", id))
        .role(if toast.tone == Tone::Danger {
            Role::Alert
        } else {
            Role::Status
        })
        .aria_label(toast.title.clone())
        .when_some(toast.body.clone(), |card, body| card.aria_description(body))
        .occlude()
        .flex()
        .size_full()
        .overflow_hidden()
        .py(px(10.0))
        .pl(px(14.0))
        .pr(px(6.0))
        .rounded(theme.radius.panel)
        .bg(bg)
        .border_1()
        .border_color(theme.border)
        .shadow(theme.floating())
        .when(content > 0.0, |card| {
            card.child(
                div()
                    .flex()
                    .flex_1()
                    .items_start()
                    .gap(px(12.0))
                    .opacity(content)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .py(px(2.0))
                            .child(
                                div()
                                    .text_size(TEXT_BODY)
                                    .line_height(LINE_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if toast.tone == Tone::Neutral {
                                        theme.text
                                    } else {
                                        tone_text
                                    })
                                    .child(toast.title.clone()),
                            )
                            .when_some(toast.body.clone(), |column, body| {
                                column.child(
                                    div()
                                        .text_size(TEXT_SMALL)
                                        .line_height(LINE_SMALL)
                                        .text_color(theme.text_secondary)
                                        .child(body),
                                )
                            }),
                    )
                    .when_some(toast.action.clone(), |row, (label, action)| {
                        let stack = stack.clone();
                        row.child(Button::new("action", label).size(Size::Small).on_click(
                            move |_, window, cx| {
                                if !stack.read(cx).present().any(|shown| shown.id == id) {
                                    return;
                                }
                                action(window, cx);
                                stack.update(cx, |toasts, cx| {
                                    toasts.dismiss(id);
                                    cx.notify();
                                });
                            },
                        ))
                    })
                    .child(
                        Button::icon("dismiss", Icon::X, "Dismiss")
                            .size(Size::Small)
                            .on_click(dismiss),
                    ),
            )
        })
}
