use std::time::{Duration, Instant};

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, TextRun, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};

use crate::motion::ease_out;

pub const INTENT: Duration = Duration::from_millis(600);
pub const REWIND: Duration = Duration::from_millis(250);
pub const HOLD: Duration = Duration::from_millis(500);
pub const SPEED: f32 = 60.0;

#[derive(Default)]
struct State {
    hovered: Option<Instant>,
    room: Pixels,
    text: Pixels,
    wake: Option<Task<()>>,
}

#[derive(IntoElement)]
pub struct Marquee {
    id: ElementId,
    text: SharedString,
}

impl Marquee {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

enum Shown {
    End,
    Start,
    Scrolled(f32),
}

enum Next {
    Frame,
    After(Duration),
    Rest,
}

fn offset(overflow: f32, elapsed: Duration) -> (Option<f32>, Next) {
    let Some(moving) = elapsed.checked_sub(INTENT) else {
        return (None, Next::After(INTENT - elapsed));
    };
    if let Some(reading) = moving.checked_sub(REWIND + HOLD) {
        let read = reading.as_secs_f32() * SPEED;
        let next = if read < overflow {
            Next::Frame
        } else {
            Next::Rest
        };
        return (Some(read.min(overflow)), next);
    }
    match moving.checked_sub(REWIND) {
        Some(held) => (Some(0.0), Next::After(HOLD - held)),
        None => {
            let rewound = ease_out(moving.as_secs_f32() / REWIND.as_secs_f32());
            (Some(overflow * (1.0 - rewound)), Next::Frame)
        }
    }
}

impl RenderOnce for Marquee {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State::default());
        let (hovered, overflow) = {
            let state = state.read(cx);
            (state.hovered, f32::from(state.text - state.room))
        };
        let shown = match hovered.filter(|_| overflow > 0.0) {
            None => Shown::End,
            Some(_) if cx.reduce_motion() => Shown::Start,
            Some(since) => {
                let (offset, next) = offset(overflow, cx.background_executor().now() - since);
                match next {
                    Next::Frame => window.request_animation_frame(),
                    Next::After(pause) => state.update(cx, |state, cx| {
                        state.wake = Some(cx.spawn(async move |state, cx| {
                            cx.background_executor().timer(pause).await;
                            state.update(cx, |_, cx| cx.notify()).ok();
                        }));
                    }),
                    Next::Rest => {}
                }
                offset.map_or(Shown::End, Shown::Scrolled)
            }
        };
        let measured = state.clone();
        let measure = self.text.clone();
        let entered = state.clone();
        div()
            .id(self.id)
            .relative()
            .min_w_0()
            .overflow_hidden()
            .on_hover(move |inside, _, cx| {
                entered.update(cx, |state, cx| {
                    state.hovered = inside.then(|| cx.background_executor().now());
                    state.wake = None;
                    cx.notify();
                })
            })
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let style = window.text_style();
                        let size = style.font_size.to_pixels(window.rem_size());
                        let run = TextRun {
                            len: measure.len(),
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let text = window
                            .text_system()
                            .shape_line(measure.clone(), size, &[run], None)
                            .width;
                        let room = bounds.size.width;
                        let resized = measured.update(cx, |state, _| {
                            let resized = state.room != room || state.text != text;
                            state.room = room;
                            state.text = text;
                            resized
                        });
                        if resized {
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .truncate()
                    .map(|rest| match shown {
                        Shown::End => rest.text_ellipsis_start(),
                        Shown::Start => rest,
                        Shown::Scrolled(_) => rest.invisible(),
                    })
                    .child(self.text.clone()),
            )
            .map(|marquee| match shown {
                Shown::Scrolled(offset) => marquee.child(
                    div()
                        .debug_selector(|| "marquee-text".into())
                        .absolute()
                        .top_0()
                        .left(px(-offset))
                        .whitespace_nowrap()
                        .child(self.text),
                ),
                Shown::End | Shown::Start => marquee,
            })
    }
}
