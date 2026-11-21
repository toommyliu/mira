use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, TextRun, Window, canvas, deferred, div, prelude::FluentBuilder as _, px,
};

use super::{Button, Icon, Spinner, button::Size, icon};
use crate::{ActiveTheme as _, Tone, motion, nav};

const LINGER: Duration = Duration::from_secs(5);
const LIMIT: usize = 3;
const GAP: Pixels = px(14.0);
const PEEK: Pixels = px(14.0);
const SHRINK: f32 = 0.05;
const UNMOUNT: Duration = motion::GLIDE;
const FALLBACK_HEIGHT: Pixels = px(42.0);
const ICON: Pixels = px(14.0);

type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToastId(u64);

#[derive(Clone)]
pub struct Toast {
    title: SharedString,
    body: Option<SharedString>,
    tone: Tone,
    loading: bool,
    action: Option<(SharedString, ActionHandler)>,
}

#[derive(Clone, Copy)]
enum Lead {
    Spinner,
    Icon(Icon),
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: None,
            tone: Tone::Neutral,
            loading: false,
            action: None,
        }
    }

    pub fn loading(mut self) -> Self {
        self.loading = true;
        self
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
        self.loading || self.action.is_some() || self.tone == Tone::Danger
    }

    fn lead(&self) -> Option<Lead> {
        if self.loading {
            Some(Lead::Spinner)
        } else {
            (self.tone != Tone::Neutral).then(|| Lead::Icon(Icon::of(self.tone)))
        }
    }
}

#[derive(Clone)]
struct Shown {
    focus: gpui::FocusHandle,
    action_focus: gpui::FocusHandle,
    dismiss_focus: gpui::FocusHandle,
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

pub struct Toaster {
    focus: gpui::FocusHandle,
    return_focus: Option<gpui::FocusHandle>,
    inactive: bool,
    paused: bool,
    _subscriptions: [gpui::Subscription; 2],
    next: u64,
    shown: Vec<Shown>,
    hovered: bool,
    expanded: bool,
    timers: HashMap<u64, Timer>,
    heights: Rc<RefCell<HashMap<u64, Pixels>>>,
}

impl Toaster {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let owner = window.window_handle().window_id();
        cx.new(|cx: &mut Context<Self>| {
            let stack = cx.weak_entity();
            let keys = cx.intercept_keystrokes(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if window.window_handle().window_id() != owner
                    || !matches!(key, "f6" | "escape" | "tab")
                {
                    return;
                }
                let modified = event.keystroke.modifiers;
                if modified.control
                    || modified.platform
                    || modified.alt
                    || modified.function
                    || (modified.shift && key != "tab")
                {
                    return;
                }
                if window
                    .context_stack()
                    .iter()
                    .any(|ctx| ctx.contains("Modal"))
                {
                    return;
                }
                let _ = stack.update(cx, |stack, cx| {
                    stack.navigate(key, modified.shift, window, cx)
                });
            });
            let activation = cx.observe_window_activation(window, |stack, window, cx| {
                stack.inactive = !window.is_window_active();
                stack.pause_timers(stack.expanded || stack.inactive, cx);
                cx.notify();
            });
            Self {
                focus: cx.focus_handle(),
                return_focus: None,
                inactive: !window.is_window_active(),
                paused: !window.is_window_active(),
                next: 0,
                shown: Vec::new(),
                hovered: false,
                expanded: false,
                timers: HashMap::new(),
                heights: Rc::default(),
                _subscriptions: [keys, activation],
            }
        })
    }

    fn navigate(
        &mut self,
        key: &str,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inside = self.focus.contains_focused(window, cx);
        if key == "f6" {
            let newest = self.present().next().map(|s| s.focus.clone());
            if inside {
                self.restore(window, cx);
            } else if let Some(newest) = newest {
                self.return_focus = window.focused(cx);
                window.focus(&newest, cx);
            } else {
                return;
            }
        } else if inside {
            let active: Vec<_> = self.present().take(LIMIT).cloned().collect();
            if key == "escape" {
                let Some(current) = active
                    .iter()
                    .position(|s| s.focus.contains_focused(window, cx))
                else {
                    return;
                };
                self.dismiss_card(active[current].id, window, cx);
            } else {
                let stops: Vec<_> = active
                    .iter()
                    .flat_map(|s| {
                        [
                            Some(&s.focus),
                            s.toast.action.as_ref().map(|_| &s.action_focus),
                            Some(&s.dismiss_focus),
                        ]
                        .into_iter()
                        .flatten()
                    })
                    .collect();
                let current = stops.iter().position(|s| s.is_focused(window));
                let next = current
                    .and_then(|i| {
                        if backwards {
                            i.checked_sub(1)
                        } else {
                            Some(i + 1)
                        }
                    })
                    .and_then(|i| stops.get(i));
                if let Some(next) = next {
                    window.focus(next, cx);
                } else {
                    self.restore(window, cx);
                }
            }
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn dismiss_card(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let had_focus = self
            .present()
            .any(|shown| shown.id == id && shown.focus.contains_focused(window, cx));
        self.dismiss(id);
        if had_focus {
            let next = self.present().next().map(|shown| shown.focus.clone());
            if let Some(next) = next {
                window.focus(&next, cx);
            } else {
                self.restore(window, cx);
            }
        }
        cx.notify();
    }

    fn restore(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(previous) = self.return_focus.take() {
            window.focus(&previous, cx);
        } else {
            window.focus_next(cx);
        }
    }

    pub fn push(stack: &Entity<Self>, toast: Toast, cx: &mut App) -> ToastId {
        let reduce = cx.reduce_motion();
        stack.update(cx, |toasts, cx| {
            toasts.prune(reduce);
            let id = toasts.next;
            toasts.next += 1;
            let stays = toast.stays();
            toasts.shown.insert(
                0,
                Shown {
                    focus: cx.focus_handle(),
                    action_focus: cx.focus_handle(),
                    dismiss_focus: cx.focus_handle(),
                    id,
                    toast,
                    left: None,
                },
            );
            if !stays {
                toasts.linger(id, cx);
            }
            cx.notify();
            ToastId(id)
        })
    }

    pub fn replace(stack: &Entity<Self>, id: ToastId, toast: Toast, cx: &mut App) -> ToastId {
        if !stack.read(cx).present().any(|shown| shown.id == id.0) {
            return Self::push(stack, toast, cx);
        }
        stack.update(cx, |toasts, cx| {
            toasts.timers.remove(&id.0);
            if !toast.stays() {
                toasts.linger(id.0, cx);
            }
            if let Some(shown) = toasts.shown.iter_mut().find(|shown| shown.id == id.0) {
                shown.toast = toast;
            }
            cx.notify();
        });
        id
    }

    fn linger(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.paused {
            self.timers.insert(id, Timer::Paused(LINGER));
        } else {
            self.start(id, LINGER, cx);
        }
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

    fn pause_timers(&mut self, paused: bool, cx: &mut Context<Self>) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
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
            if paused {
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

    pub fn layer(stack: &Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let focus = stack.read(cx).focus.clone();
        let reduce = cx.reduce_motion();
        let focused = focus.contains_focused(window, cx) && nav::keyboard_input(window, cx);
        let (shown, expanded, heights) = stack.update(cx, |toasts, cx| {
            toasts.prune(reduce);
            if toasts.shown.is_empty() {
                toasts.hovered = false;
            }
            let leaving = toasts.shown.iter().any(|shown| shown.left.is_some());
            toasts.expanded = toasts.hovered || focused || (leaving && toasts.expanded);
            toasts.pause_timers(toasts.expanded || toasts.inactive, cx);
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
        let viewport_width = window.viewport_size().width;
        let edge = theme.space(4.0);
        let widest = shown
            .iter()
            .map(|shown| natural_width(&shown.toast, window, &theme))
            .fold(Pixels::ZERO, Pixels::max);
        let width = widest
            .min(theme.layout.overlay_max_width)
            .min((viewport_width - edge * 2.0).max(px(1.0)));
        let compact = viewport_width < theme.layout.compact_overlay;
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
                    width * SHRINK / 2.0 * at as f32
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
            let bg = theme.popover;
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
                    .child(card(
                        stack,
                        entry,
                        content,
                        bg,
                        &theme,
                        entry.focus.is_focused(window) && nav::keyboard_input(window, cx),
                    ))
                    .into_any_element(),
            );
        }
        let hover_stack = stack.clone();
        deferred(
            div().absolute().inset_0().overflow_hidden().child(
                div()
                    .absolute()
                    .left(edge)
                    .right(edge)
                    .bottom(edge)
                    .flex()
                    .map(|row| {
                        if compact {
                            row.justify_center()
                        } else {
                            row.justify_end()
                        }
                    })
                    .child(
                        div()
                            .id("toasts")
                            .track_focus(&focus)
                            .relative()
                            .w(width)
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
                    ),
            ),
        )
        .with_priority(super::layer::TOASTS)
        .into_any_element()
    }
}

fn natural_width(toast: &Toast, window: &Window, theme: &crate::Theme) -> Pixels {
    let line = |text: &SharedString, weight: FontWeight| {
        text.split('\n')
            .map(|part| {
                let run = TextRun {
                    len: part.len(),
                    font: gpui::Font {
                        weight,
                        ..gpui::font(theme.font_family.clone())
                    },
                    color: Hsla::default(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .layout_line(part, theme.typography.body, &[run], None)
                    .width
            })
            .fold(Pixels::ZERO, Pixels::max)
    };
    let gap = theme.space(3.0);
    let text = toast
        .body
        .as_ref()
        .map_or(Pixels::ZERO, |body| line(body, FontWeight::NORMAL))
        .max(line(&toast.title, FontWeight::MEDIUM));
    let action = toast.action.as_ref().map_or(Pixels::ZERO, |(label, _)| {
        line(label, FontWeight::MEDIUM) + theme.space(2.0) * 2.0 + px(2.0) + gap
    });
    let icon = toast
        .lead()
        .map_or(Pixels::ZERO, |_| ICON + theme.space(2.0));
    let chrome = theme.space(3.0) * 2.0 + px(2.0) + icon;
    (chrome + text + action + gap + theme.layout.control_sm).ceil()
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
    focused: bool,
) -> impl IntoElement + gpui::Styled + 'static {
    let layout = theme.layout;
    let typography = theme.typography;
    let id = shown.id;
    let toast = &shown.toast;
    let (solid, _, tone_text) = theme.tone(toast.tone);
    let dismiss = {
        let stack = stack.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            stack.update(cx, |toasts, cx| toasts.dismiss_card(id, window, cx));
        }
    };
    div()
        .id(("toast", id))
        .debug_selector(|| format!("toast: {}", toast.title))
        .track_focus(&shown.focus)
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
        .py(theme.space(2.0))
        .px(theme.space(3.0))
        .rounded(theme.radius.control)
        .bg(bg)
        .border_1()
        .border_color(theme.border)
        .shadow(theme.large_shadow())
        .when(focused, |card| {
            card.border_color(theme.ring).child(theme.halo(
                theme.ring.opacity(0.5),
                theme.radius.control,
                px(1.0),
                px(3.0),
            ))
        })
        .when(content > 0.0, |card| {
            card.child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap(layout.space(3.0))
                    .opacity(content)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .gap(layout.space(2.0))
                            .when_some(toast.lead(), |row, lead| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .h(typography.body_line)
                                        .mt(px(2.0))
                                        .child(match lead {
                                            Lead::Spinner => Spinner::new(("toast-spinner", id))
                                                .color(solid)
                                                .size(ICON)
                                                .into_any_element(),
                                            Lead::Icon(glyph) => {
                                                icon(glyph, solid).size(ICON).into_any_element()
                                            }
                                        }),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .py(px(2.0))
                                    .gap(theme.space(1.0))
                                    .child(
                                        div()
                                            .text_size(typography.body)
                                            .line_height(typography.body_line)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if toast.tone == Tone::Neutral {
                                                theme.foreground
                                            } else {
                                                tone_text
                                            })
                                            .child(toast.title.clone()),
                                    )
                                    .when_some(toast.body.clone(), |column, body| {
                                        column.child(
                                            div()
                                                .text_size(typography.body)
                                                .line_height(typography.body_line)
                                                .text_color(theme.muted_foreground)
                                                .child(body),
                                        )
                                    }),
                            ),
                    )
                    .when_some(toast.action.clone(), |row, (label, action)| {
                        let stack = stack.clone();
                        row.child(
                            Button::new("action", label)
                                .focus(shown.action_focus.clone())
                                .variant(super::button::Variant::Outline)
                                .size(Size::Small)
                                .on_click(move |_, window, cx| {
                                    if !stack.read(cx).present().any(|shown| shown.id == id) {
                                        return;
                                    }
                                    stack.update(cx, |toasts, cx| {
                                        toasts.dismiss_card(id, window, cx)
                                    });
                                    action(window, cx);
                                }),
                        )
                    })
                    .child(
                        Button::icon("dismiss", Icon::X, "Dismiss")
                            .focus(shown.dismiss_focus.clone())
                            .size(Size::Small)
                            .on_click(dismiss),
                    ),
            )
        })
}
