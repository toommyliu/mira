#![expect(
    clippy::disallowed_methods,
    reason = "Sortable is the one owner of gpui's drag and drop events"
)]

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, DispatchPhase, Div, ElementId,
    InteractiveElement as _, IntoElement, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    RenderOnce, ScrollHandle, Size, Stateful, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, anchored, canvas, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::layer;
use crate::{ActiveTheme as _, motion};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Flow {
    #[default]
    Column,
    Row,
    Wrap,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pose {
    Rest,
    Sorting,
    Lifted,
}

const EDGE: Pixels = px(32.0);
const SCROLL_SPEED: f32 = 480.0;
const LONGEST_STEP: Duration = Duration::from_millis(50);

type ItemRenderer = Rc<dyn Fn((usize, Pose), &mut Window, &mut App) -> AnyElement>;
type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Sortable {
    container: Stateful<Div>,
    flow: Flow,
    keys: Rc<[ElementId]>,
    item: ItemRenderer,
    on_reorder: ReorderHandler,
    pinned: usize,
    scroll: Option<ScrollHandle>,
}

impl Sortable {
    pub fn new(
        container: Stateful<Div>,
        flow: Flow,
        keys: impl IntoIterator<Item = impl Into<ElementId>>,
        item: impl Fn((usize, Pose), &mut Window, &mut App) -> AnyElement + 'static,
        on_reorder: impl Fn(usize, usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            container,
            flow,
            keys: keys.into_iter().map(Into::into).collect(),
            item: Rc::new(item),
            on_reorder: Rc::new(on_reorder),
            pinned: 0,
            scroll: None,
        }
    }

    pub fn scrolls(mut self, handle: &ScrollHandle) -> Self {
        self.container = match self.flow {
            Flow::Row => self.container.overflow_x_scroll(),
            Flow::Column | Flow::Wrap => self.container.overflow_y_scroll(),
        }
        .track_scroll(handle);
        self.scroll = Some(handle.clone());
        self
    }

    pub fn pin_first(mut self, count: usize) -> Self {
        self.pinned = count;
        self
    }
}

#[derive(Default)]
struct State {
    flow: Flow,
    keys: Rc<[ElementId]>,
    pinned: usize,
    slots: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    scroll: Option<ScrollHandle>,
    scrolled_at: Option<Instant>,
    generation: usize,
    drag: Option<Drag>,
    settle: Option<Settle>,
    escape: Option<Subscription>,
}

struct Drag {
    key: ElementId,
    grab: Point<Pixels>,
    start: Point<Pixels>,
}

#[derive(Clone)]
struct Settle {
    index: usize,
    from: Point<Pixels>,
    to: Point<Pixels>,
    size: Size<Pixels>,
}

struct Aim {
    from: usize,
    to: usize,
    lifted: Point<Pixels>,
    size: Size<Pixels>,
    shifts: Vec<Point<Pixels>>,
}

impl State {
    fn aim(&self, pointer: Point<Pixels>) -> Option<Aim> {
        let drag = self.drag.as_ref()?;
        let from = self.keys.iter().position(|key| *key == drag.key)?;
        if from < self.pinned {
            return None;
        }
        let slots = self.slots.borrow();
        let size = slots.get(from)?.size;
        let movable = &slots[self.pinned..];
        let lifted = lifted_origin(
            self.flow,
            movable,
            from - self.pinned,
            pointer - drag.grab,
            self.scroll.as_ref().map(ScrollHandle::bounds),
        );
        let to = self.pinned + nearest(movable, Bounds::new(lifted, size).center());
        Some(Aim {
            from,
            to,
            lifted,
            size,
            shifts: shifts(self.flow, &slots, from, to),
        })
    }

    fn start(
        &mut self,
        key: ElementId,
        grab: Point<Pixels>,
        start: Point<Pixels>,
        escape: Subscription,
    ) {
        self.generation += 1;
        self.drag = Some(Drag { key, grab, start });
        self.settle = None;
        self.escape = Some(escape);
    }

    fn autoscroll(&mut self, pointer: Point<Pixels>) -> bool {
        let (Some(drag), Some(scroll)) = (&self.drag, &self.scroll) else {
            self.scrolled_at = None;
            return false;
        };
        let vertical = self.flow != Flow::Row;
        let along = |at: Point<Pixels>| f32::from(if vertical { at.y } else { at.x });
        let viewport = scroll.bounds();
        let (start, end) = if vertical {
            (viewport.top(), viewport.bottom())
        } else {
            (viewport.left(), viewport.right())
        };
        let pull = edge_pull(
            along(pointer) - along(drag.start),
            along(pointer),
            f32::from(start),
            f32::from(end),
        );
        let now = Instant::now();
        let elapsed = self
            .scrolled_at
            .map_or(Duration::ZERO, |then| now - then)
            .min(LONGEST_STEP);
        self.scrolled_at = (pull != 0.0).then_some(now);
        if pull == 0.0 {
            return false;
        }
        let offset = scroll.offset();
        let limit = -along(scroll.max_offset());
        let next = (along(offset) - pull * SCROLL_SPEED * elapsed.as_secs_f32()).clamp(limit, 0.0);
        scroll.set_offset(if vertical {
            point(offset.x, px(next))
        } else {
            point(px(next), offset.y)
        });
        if pull > 0.0 { next > limit } else { next < 0.0 }
    }

    fn finish(&mut self, pointer: Point<Pixels>, commit: bool) -> Option<(usize, usize)> {
        let aim = self.aim(pointer);
        self.drag.take()?;
        self.escape = None;
        self.scrolled_at = None;
        let aim = aim?;
        let rest = self.slots.borrow()[aim.from].origin;
        self.settle = Some(Settle {
            index: if commit { aim.to } else { aim.from },
            from: aim.lifted,
            to: if commit {
                rest + aim.shifts[aim.from]
            } else {
                rest
            },
            size: aim.size,
        });
        if commit {
            self.generation += 1;
        }
        commit.then_some((aim.from, aim.to))
    }
}

struct Dragged;

struct Hidden;

impl Render for Hidden {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl RenderOnce for Sortable {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            mut container,
            flow,
            keys,
            item,
            on_reorder,
            pinned,
            scroll,
        } = self;
        let id = container
            .interactivity()
            .element_id
            .clone()
            .expect("a Stateful div has an id");
        let state = window.use_keyed_state((id.clone(), "sortable"), cx, |_, _| State::default());
        let dragging = cx.has_active_drag();
        let pointer = window.mouse_position();
        let (aim, lost, scrolling) = state.update(cx, |state, _| {
            state.flow = flow;
            state.keys = keys.clone();
            state.pinned = pinned;
            state.scroll = scroll;
            state
                .slots
                .borrow_mut()
                .resize(keys.len(), Bounds::default());
            if !dragging {
                state.finish(pointer, false);
            }
            let aim = state.aim(pointer);
            let lost = aim.is_none() && state.drag.take().is_some();
            if lost {
                state.escape = None;
            }
            (aim, lost, state.autoscroll(pointer))
        });
        if lost {
            cx.stop_active_drag(window);
        }
        if scrolling {
            window.request_animation_frame();
        }
        let (generation, settle, slots) = {
            let state = state.read(cx);
            (state.generation, state.settle.clone(), state.slots.clone())
        };

        let settled = settle.is_some();
        let settling = settle
            .filter(|settle| settle.index < keys.len())
            .and_then(|settle| {
                let offset = settle.from - settle.to;
                let (x, y) = window.with_id(id.clone(), |window| {
                    (
                        motion::travel_from(
                            ("settle-x", generation),
                            f32::from(offset.x),
                            0.0,
                            window,
                            cx,
                        ),
                        motion::travel_from(
                            ("settle-y", generation),
                            f32::from(offset.y),
                            0.0,
                            window,
                            cx,
                        ),
                    )
                });
                (x != 0.0 || y != 0.0)
                    .then(|| (settle.index, settle.to + point(px(x), px(y)), settle.size))
            });
        if settled && settling.is_none() {
            state.update(cx, |state, _| state.settle = None);
        }

        let pose = if aim.is_some() {
            Pose::Sorting
        } else {
            Pose::Rest
        };
        let children: Vec<_> = window.with_id(id.clone(), |window| {
            window.with_id(generation, |window| {
                keys.iter()
                    .enumerate()
                    .map(|(index, key)| {
                        let source = aim.as_ref().filter(|aim| aim.from == index);
                        let shift = match &aim {
                            Some(aim) if aim.from != index => aim.shifts[index],
                            _ => Point::default(),
                        };
                        let x = motion::travel((key.clone(), "x"), f32::from(shift.x), window, cx);
                        let y = motion::travel((key.clone(), "y"), f32::from(shift.y), window, cx);
                        let content = match source {
                            Some(aim) => div()
                                .w(aim.size.width)
                                .h(aim.size.height)
                                .into_any_element(),
                            None => item((index, pose), window, cx),
                        };
                        let measure = slots.clone();
                        let weak = state.downgrade();
                        let key = key.clone();
                        div()
                            .id(key.clone())
                            .flex_none()
                            .relative()
                            .when(settling.is_some_and(|(at, ..)| at == index), |slot| {
                                slot.invisible()
                            })
                            .child(div().relative().left(px(x)).top(px(y)).child(content))
                            .child(
                                canvas(
                                    move |bounds, _, _| {
                                        if let Some(slot) = measure.borrow_mut().get_mut(index) {
                                            *slot = bounds;
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                            .when(index >= pinned, |slot| {
                                slot.on_drag(Dragged, move |_, grab, window, cx| {
                                    let escape = cx.intercept_keystrokes({
                                        let weak = weak.clone();
                                        move |event, window, cx| {
                                            if event.keystroke.key != "escape" {
                                                return;
                                            }
                                            let pointer = window.mouse_position();
                                            let cancelled = weak
                                                .update(cx, |state, _| {
                                                    let active = state.drag.is_some();
                                                    state.finish(pointer, false);
                                                    active
                                                })
                                                .unwrap_or(false);
                                            if cancelled {
                                                cx.stop_active_drag(window);
                                                cx.stop_propagation();
                                            }
                                        }
                                    });
                                    let start = window.mouse_position();
                                    weak.update(cx, |state, _| {
                                        state.start(key.clone(), grab, start, escape)
                                    })
                                    .ok();
                                    cx.new(|_| Hidden)
                                })
                            })
                            .into_any_element()
                    })
                    .collect()
            })
        });

        let lifted = aim
            .as_ref()
            .map(|aim| (aim.from, aim.lifted, aim.size))
            .or(settling)
            .map(|(index, origin, size)| {
                let theme = cx.theme().clone();
                deferred(
                    anchored().position(origin).snap_to_window().child(
                        div()
                            .w(size.width)
                            .h(size.height)
                            .rounded(theme.radius.item)
                            .bg(theme.overlay)
                            .shadow(theme.floating())
                            .child(item((index, Pose::Lifted), window, cx)),
                    ),
                )
                .with_priority(layer::DRAG)
            });

        let release = state.downgrade();
        container.children(children).children(lifted).child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let dropped = release
                            .update(cx, |state, _| state.finish(event.position, true))
                            .ok()
                            .flatten();
                        if let Some((from, to)) = dropped
                            && from != to
                        {
                            on_reorder(from, to, window, cx);
                        }
                    });
                },
            )
            .absolute(),
        )
    }
}

fn lifted_origin(
    flow: Flow,
    slots: &[Bounds<Pixels>],
    from: usize,
    origin: Point<Pixels>,
    viewport: Option<Bounds<Pixels>>,
) -> Point<Pixels> {
    let rest = slots[from];
    let mut area = slots.iter().fold(rest, |area, slot| area.union(slot));
    if let Some(viewport) = viewport {
        area = area.intersect(&viewport);
    }
    let clamp = |value: Pixels, low: Pixels, high: Pixels| value.min(high.max(low)).max(low);
    let x = clamp(origin.x, area.left(), area.right() - rest.size.width);
    let y = clamp(origin.y, area.top(), area.bottom() - rest.size.height);
    match flow {
        Flow::Column => point(rest.origin.x, y),
        Flow::Row => point(x, rest.origin.y),
        Flow::Wrap => point(x, y),
    }
}

fn edge_pull(moved: f32, at: f32, start: f32, end: f32) -> f32 {
    let edge = f32::from(EDGE);
    if moved > 0.0 && at > end - edge {
        ((at - end + edge) / edge).min(1.0)
    } else if moved < 0.0 && at < start + edge {
        -((start + edge - at) / edge).min(1.0)
    } else {
        0.0
    }
}

fn nearest(slots: &[Bounds<Pixels>], center: Point<Pixels>) -> usize {
    let distance = |slot: &Bounds<Pixels>| {
        let offset = slot.center() - center;
        let (x, y) = (f32::from(offset.x), f32::from(offset.y));
        x * x + y * y
    };
    slots
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
        .map_or(0, |(index, _)| index)
}

fn shifts(flow: Flow, slots: &[Bounds<Pixels>], from: usize, to: usize) -> Vec<Point<Pixels>> {
    let along = |vertical: bool| {
        let start = |slot: &Bounds<Pixels>| if vertical { slot.top() } else { slot.left() };
        let end = |slot: &Bounds<Pixels>| {
            if vertical {
                slot.bottom()
            } else {
                slot.right()
            }
        };
        let gap = match slots {
            [first, second, ..] => start(second) - end(first),
            _ => Pixels::ZERO,
        };
        let mut order: Vec<usize> = (0..slots.len()).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let mut shifts = vec![Point::default(); slots.len()];
        let mut cursor = start(&slots[0]);
        for index in order {
            let shift = cursor - start(&slots[index]);
            shifts[index] = if vertical {
                point(Pixels::ZERO, shift)
            } else {
                point(shift, Pixels::ZERO)
            };
            cursor += end(&slots[index]) - start(&slots[index]) + gap;
        }
        shifts
    };
    match flow {
        Flow::Column => along(true),
        Flow::Row => along(false),
        Flow::Wrap => (0..slots.len())
            .map(|at| {
                let lands = if at == from {
                    to
                } else if from < at && at <= to {
                    at - 1
                } else if to <= at && at < from {
                    at + 1
                } else {
                    at
                };
                slots[lands].origin - slots[at].origin
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{Flow, shifts};

    #[test]
    fn rows_of_different_heights_close_up_around_the_moved_row() {
        let rows = [10.0, 30.0, 10.0].iter().scan(0.0, |top, height| {
            let row = Bounds::new(point(px(0.0), px(*top)), size(px(100.0), px(*height)));
            *top += height + 2.0;
            Some(row)
        });
        let rows: Vec<_> = rows.collect();
        let moved = shifts(Flow::Column, &rows, 2, 0);
        let tops: Vec<_> = rows
            .iter()
            .zip(&moved)
            .map(|(row, shift)| f32::from(row.top() + shift.y))
            .collect();
        assert_eq!(tops, [12.0, 24.0, 0.0]);
    }
}
