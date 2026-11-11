use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, DispatchPhase, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, RenderOnce, ScrollHandle,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div,
    point, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, motion};

const INSET: Pixels = px(3.0);
const MIN_THUMB: Pixels = px(24.0);
const HOLD: Duration = Duration::from_millis(1000);
const FADE: Duration = Duration::from_millis(200);

#[derive(Default)]
struct State {
    drag: Cell<Option<(Pixels, Pixels)>>,
    hovered: Cell<bool>,
    scrolled: Cell<Option<Instant>>,
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

#[derive(IntoElement)]
pub struct Scrollbar {
    id: ElementId,
    handle: ScrollHandle,
}

impl Scrollbar {
    pub const GUTTER: Pixels = px(12.0);

    pub fn new(id: impl Into<ElementId>, handle: ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle,
        }
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
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let wheel = wheel.clone();
                        let watched = watched.clone();
                        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, _| {
                            if phase == DispatchPhase::Capture
                                && watched.bounds().contains(&event.position)
                            {
                                wheel.reveal(window);
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            );
        let viewport = handle.bounds().size.height;
        let max = handle.max_offset().y;
        if max <= px(0.0) || viewport <= px(0.0) {
            state.hovered.set(false);
            return strip;
        }
        let now = Instant::now();
        let opacity = state.opacity(now, cx.reduce_motion());
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
        let track = viewport - INSET * 2.0;
        let thumb = (track * (viewport / (viewport + max))).max(MIN_THUMB);
        let travel = track - thumb;
        let scrolled = (-handle.offset().y).clamp(px(0.0), max);
        let top = INSET + travel * (scrolled / max);
        let dragging = state.drag.get().is_some();
        let theme = cx.theme().clone();
        let page = handle.clone();
        let grab = state.clone();
        let grab_handle = handle.clone();
        let hover = state.clone();
        strip
            .opacity(opacity)
            .on_hover(move |hovered, window, _| {
                hover.hovered.set(*hovered);
                hover.reveal(window);
            })
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let mut offset = page.offset();
                let local = event.position.y - page.bounds().top();
                offset.y = if local < top {
                    offset.y + viewport
                } else {
                    offset.y - viewport
                }
                .clamp(-max, px(0.0));
                page.set_offset(offset);
                window.refresh();
                cx.stop_propagation();
            })
            .child(
                div()
                    .id("thumb")
                    .absolute()
                    .top(top)
                    .right(INSET)
                    .w(Self::GUTTER - INSET * 2.0)
                    .h(thumb)
                    .rounded_full()
                    .bg(theme.text.opacity(if dragging { 0.36 } else { 0.2 }))
                    .hover(|style| style.bg(theme.text.opacity(0.32)))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        grab.drag
                            .set(Some((event.position.y, grab_handle.offset().y)));
                        window.refresh();
                        cx.stop_propagation();
                    }),
            )
            .when(dragging, |bar| {
                bar.child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, _| {
                            let follow = state.clone();
                            let handle = handle.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, _, window, _| {
                                let Some((start, offset)) = follow.drag.get() else {
                                    return;
                                };
                                if travel <= px(0.0) {
                                    return;
                                }
                                let moved = (event.position.y - start) * (max / travel);
                                handle.set_offset(point(
                                    handle.offset().x,
                                    (offset - moved).clamp(-max, px(0.0)),
                                ));
                                window.refresh();
                            });
                            let release = state.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, _, window, _| {
                                release.drag.set(None);
                                release.reveal(window);
                            });
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            })
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
