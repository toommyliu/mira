use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, ElementId, InteractiveElement as _, IntoElement, ListOffset, ListState,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, RenderOnce,
    ScrollHandle, Size, StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div,
    point, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, motion};

const INSET: Pixels = px(1.0);
const MIN_THUMB: Pixels = px(24.0);
const HOLD: Duration = Duration::from_millis(1000);
const FADE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
struct Drag {
    from: Pixels,
    at: Pixels,
    top: Pixels,
    scrolled: f32,
    max: f32,
    thumb: Pixels,
    travel: Pixels,
}

#[derive(Default)]
struct State {
    drag: Cell<Option<Drag>>,
    hovered: Cell<bool>,
    scrolled: Cell<Option<Instant>>,
    scrolled_to: Cell<Option<f32>>,
    last: Cell<Option<Extent>>,
    end: Cell<Option<(usize, Size<Pixels>, f32)>>,
    hide: Cell<Option<Task<()>>>,
}

impl State {
    fn reveal(&self, window: &mut Window) {
        self.scrolled.set(Some(Instant::now()));
        window.refresh();
    }

    fn opacity(&self, now: Instant, reduce_motion: bool) -> f32 {
        if self.drag.get().is_some() || self.hovered.get() {
            return 1.0;
        }
        let Some(at) = self.scrolled.get() else {
            return 0.0;
        };
        let Some(fading) = now.duration_since(at).checked_sub(HOLD) else {
            return 1.0;
        };
        if reduce_motion || fading >= FADE {
            return 0.0;
        }
        1.0 - motion::ease_out(fading.as_secs_f32() / FADE.as_secs_f32())
    }
}

#[derive(Clone)]
enum Scrolled {
    Handle(ScrollHandle),
    List(ListState),
}

#[derive(Clone, Copy, PartialEq)]
struct Extent {
    shown: f32,
    max: f32,
    scrolled: f32,
}

fn rows(list: &ListState) -> Option<Extent> {
    let viewport = list.viewport_bounds();
    let count = list.item_count();
    let top = list.logical_scroll_top();
    let mut shown = 0.0;
    let mut into = 0.0;
    for ix in top.item_ix..count {
        let Some(row) = list.bounds_for_item(ix) else {
            break;
        };
        if row.top() >= viewport.bottom() {
            break;
        }
        let height = row.size.height.max(px(1.0));
        if ix == top.item_ix {
            into = (top.offset_in_item / height).clamp(0.0, 1.0);
        }
        let visible = row.bottom().min(viewport.bottom()) - row.top().max(viewport.top());
        shown += (visible / height).max(0.0);
    }
    (shown > 0.0).then(|| Extent {
        shown,
        max: (count as f32 - shown).max(0.0),
        scrolled: top.item_ix as f32 + into,
    })
}

impl Scrolled {
    fn bounds(&self) -> Bounds<Pixels> {
        match self {
            Scrolled::Handle(handle) => handle.bounds(),
            Scrolled::List(list) => list.viewport_bounds(),
        }
    }

    fn extent(&self, state: &State) -> Option<Extent> {
        match self {
            Scrolled::Handle(handle) => {
                let max = handle.max_offset().y / px(1.0);
                Some(Extent {
                    shown: handle.bounds().size.height / px(1.0),
                    max,
                    scrolled: (-handle.offset().y / px(1.0)).clamp(0.0, max.max(0.0)),
                })
            }
            Scrolled::List(list) => {
                let count = list.item_count();
                let viewport = list.viewport_bounds().size;
                let measured = rows(list);
                if let Some(rows) = measured
                    && list.is_following_tail()
                {
                    state.end.set(Some((count, viewport, rows.scrolled)));
                }
                let mut extent = measured.or_else(|| {
                    let shown = state.last.get()?.shown;
                    let max = (count as f32 - shown).max(0.0);
                    Some(Extent {
                        shown,
                        max,
                        scrolled: (list.logical_scroll_top().item_ix as f32).min(max),
                    })
                })?;
                if let Some((_, _, end)) = state
                    .end
                    .get()
                    .filter(|&(at, size, _)| (at, size) == (count, viewport))
                {
                    extent.max = end;
                }
                Some(extent)
            }
        }
    }

    fn scroll_to(&self, scrolled: f32, extent: Extent) {
        let target = scrolled.clamp(0.0, extent.max);
        match self {
            Scrolled::Handle(handle) => {
                handle.set_offset(point(handle.offset().x, px(-target)));
            }
            Scrolled::List(list) if target >= extent.max => list.scroll_to_end(),
            Scrolled::List(list) => {
                let item_ix = target as usize;
                let height = list
                    .bounds_for_item(item_ix)
                    .map_or(px(0.0), |row| row.size.height);
                list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item: height * target.fract(),
                });
            }
        }
    }

    fn following(&self) -> bool {
        match self {
            Scrolled::Handle(_) => false,
            Scrolled::List(list) => list.is_following_tail(),
        }
    }
}

#[derive(IntoElement)]
pub struct Scrollbar {
    id: ElementId,
    handle: Scrolled,
    persistent: bool,
}

impl Scrollbar {
    pub const GUTTER: Pixels = px(6.0);

    pub fn new(id: impl Into<ElementId>, handle: ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle: Scrolled::Handle(handle),
            persistent: false,
        }
    }

    pub fn list(id: impl Into<ElementId>, list: ListState) -> Self {
        Self {
            id: id.into(),
            handle: Scrolled::List(list),
            persistent: false,
        }
    }

    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }
}

impl RenderOnce for Scrollbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window
            .use_keyed_state((self.id.clone(), "state"), cx, |_, _| {
                Rc::new(State::default())
            })
            .read(cx)
            .clone();
        let handle = self.handle;
        let extent = handle.extent(&state);
        if extent.is_some() {
            state.last.set(extent);
        }
        let dragging = state.drag.get().is_some();
        let wheel = state.clone();
        let watched = handle.clone();
        let strip = div()
            .id(self.id)
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(Self::GUTTER)
            .child(
                canvas(
                    move |_, window, _| {
                        let now = watched.extent(&wheel).map(|extent| extent.scrolled);
                        let was = wheel.scrolled_to.replace(now);
                        if was.is_some() && was != now && !watched.following() {
                            wheel.scrolled.set(Some(Instant::now()));
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .when(dragging, |strip| {
                let follow = state.clone();
                let release = state.clone();
                let dragged = handle.clone();
                strip.child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, _| {
                            window.on_mouse_event(move |event: &MouseMoveEvent, _, window, _| {
                                let (Some(mut drag), Some(extent)) =
                                    (follow.drag.get(), follow.last.get())
                                else {
                                    return;
                                };
                                drag.at = event.position.y;
                                follow.drag.set(Some(drag));
                                let moved = (event.position.y - drag.from) / px(1.0);
                                let per_pixel = drag.max / (drag.travel / px(1.0));
                                let bounds = Extent {
                                    max: drag.max,
                                    ..extent
                                };
                                dragged.scroll_to(drag.scrolled + moved * per_pixel, bounds);
                                window.refresh();
                            });
                            window.on_mouse_event(move |_: &MouseUpEvent, _, window, _| {
                                release.drag.set(None);
                                release.reveal(window);
                            });
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            });
        let height = handle.bounds().size.height;
        let strip = strip.when(self.persistent, |strip| {
            let measured = handle.clone();
            let last = state.clone();
            let max = extent.map(|extent| extent.max);
            strip.child(
                canvas(
                    move |_, window, _| {
                        let now = measured.extent(&last).map(|extent| extent.max);
                        if (measured.bounds().size.height, now) != (height, max) {
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute(),
            )
        });
        let Some(extent) = extent.filter(|extent| extent.max > 0.0 && height > px(0.0)) else {
            state.hovered.set(false);
            return strip;
        };
        let hover = state.clone();
        let strip = strip.on_hover(move |hovered, window, _| {
            hover.hovered.set(*hovered);
            hover.reveal(window);
        });
        let now = Instant::now();
        let opacity = if self.persistent {
            1.0
        } else {
            state.opacity(now, cx.reduce_motion())
        };
        if opacity <= 0.0 {
            state.hide.take();
            return strip;
        }
        if opacity < 1.0 {
            window.request_animation_frame();
        } else if let Some(at) = state.scrolled.get()
            && let Some(left) = HOLD.checked_sub(now.duration_since(at))
        {
            let task = window.spawn(cx, async move |cx| {
                cx.background_executor().timer(left).await;
                cx.update(|window, _| window.refresh()).ok();
            });
            state.hide.set(Some(task));
        }
        let (thumb, travel, max) = match state.drag.get() {
            Some(drag) => (drag.thumb, drag.travel, drag.max),
            None => {
                let track = height - INSET * 2.0;
                let thumb = (track * (extent.shown / (extent.shown + extent.max))).max(MIN_THUMB);
                (thumb, track - thumb, extent.max)
            }
        };
        let top = match state.drag.get() {
            Some(drag) => (drag.top + drag.at - drag.from).clamp(INSET, INSET + travel),
            None => INSET + travel * (extent.scrolled.clamp(0.0, max) / max),
        };
        let theme = cx.theme().clone();
        let page = handle.clone();
        let grab = state.clone();
        strip
            .opacity(opacity)
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let local = event.position.y - page.bounds().top();
                let target = if local < top {
                    extent.scrolled - extent.shown
                } else {
                    extent.scrolled + extent.shown
                };
                page.scroll_to(target, extent);
                window.refresh();
                cx.stop_propagation();
            })
            .child(
                div()
                    .id("thumb")
                    .debug_selector(|| "scrollbar-thumb".into())
                    .absolute()
                    .top(top)
                    .right(INSET)
                    .w(Self::GUTTER - INSET * 2.0)
                    .h(thumb)
                    .rounded_full()
                    .bg(if dragging { theme.ring } else { theme.border })
                    .hover(|style| style.bg(theme.ring))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        if travel > px(0.0) {
                            grab.drag.set(Some(Drag {
                                from: event.position.y,
                                at: event.position.y,
                                top,
                                scrolled: extent.scrolled,
                                max,
                                thumb,
                                travel,
                            }));
                        }
                        window.refresh();
                        cx.stop_propagation();
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_for_a_moment_after_scrolling_then_fades() {
        let state = State::default();
        let at = Instant::now();
        assert_eq!(state.opacity(at, false), 0.0);
        state.scrolled.set(Some(at));
        assert_eq!(state.opacity(at + HOLD / 2, false), 1.0);
        let fading = state.opacity(at + HOLD + FADE / 2, false);
        assert!(fading > 0.0 && fading < 1.0);
        assert_eq!(state.opacity(at + HOLD + FADE, false), 0.0);
        assert_eq!(state.opacity(at + HOLD + FADE / 2, true), 0.0);
        state.hovered.set(true);
        assert_eq!(state.opacity(at + HOLD * 5, false), 1.0);
    }
}
