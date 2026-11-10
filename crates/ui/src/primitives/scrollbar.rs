use std::{cell::Cell, rc::Rc};

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Pixels, RenderOnce, ScrollHandle, Styled as _, Window,
    canvas, div, point, prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme as _;

const WIDTH: Pixels = px(6.0);
const INSET: Pixels = px(3.0);
const MIN_THUMB: Pixels = px(24.0);

#[derive(Default)]
struct Drag {
    from: Cell<Option<(Pixels, Pixels)>>,
}

#[derive(IntoElement)]
pub struct Scrollbar {
    id: ElementId,
    handle: ScrollHandle,
}

impl Scrollbar {
    pub fn new(id: impl Into<ElementId>, handle: ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle,
        }
    }
}

impl RenderOnce for Scrollbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let drag = window
            .use_keyed_state((self.id.clone(), "drag"), cx, |_, _| {
                Rc::new(Drag::default())
            })
            .read(cx)
            .clone();
        let handle = self.handle;
        let viewport = handle.bounds().size.height;
        let max = handle.max_offset().y;
        if max <= px(0.0) || viewport <= px(0.0) {
            return div().into_any_element();
        }
        let track = viewport - INSET * 2.0;
        let thumb = (track * (viewport / (viewport + max))).max(MIN_THUMB);
        let travel = track - thumb;
        let scrolled = (-handle.offset().y).clamp(px(0.0), max);
        let top = INSET + travel * (scrolled / max);
        let dragging = drag.from.get().is_some();
        let theme = cx.theme().clone();
        let page = handle.clone();
        let grab = drag.clone();
        let grab_handle = handle.clone();
        div()
            .id(self.id)
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(WIDTH + INSET * 2.0)
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
                    .w(WIDTH)
                    .h(thumb)
                    .rounded_full()
                    .bg(theme.text.opacity(if dragging { 0.36 } else { 0.2 }))
                    .hover(|style| style.bg(theme.text.opacity(0.32)))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        grab.from
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
                            let follow = drag.clone();
                            let handle = handle.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, _, window, _| {
                                let Some((start, offset)) = follow.from.get() else {
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
                            let release = drag.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, _, window, _| {
                                release.from.set(None);
                                window.refresh();
                            });
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .into_any_element()
    }
}
