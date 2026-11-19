use std::rc::Rc;

use gpui::{
    ClipboardItem, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px, relative,
};
use mira_ui::{
    h_flex,
    primitives::{
        Badge, Button, Column, Field, Icon, Input, List, ListItem, Select, Tab, Table, Tabs, Toast,
        Toaster,
        button::{Size, Variant},
    },
    theme::SYSTEM_MONO,
    v_flex,
};

use super::super::{Screen, view::*};

#[derive(Clone)]
struct LogRow {
    time: &'static str,
    direction: &'static str,
    payload: SharedString,
}

pub(in crate::storyboard) struct Packets {
    target: Entity<Target>,
    page: usize,
    direction: usize,
    selected: Option<usize>,
    paused: bool,
    log: Vec<LogRow>,
    queue: Vec<SharedString>,
    queue_selected: Option<usize>,
    search: Entity<Input>,
    message: Entity<Input>,
    delay: Entity<Input>,
}

impl Packets {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let target = Target::new(sessions, cx);
        cx.observe(&target, |_, _, cx| cx.notify()).detach();
        let search = field("Filter log", "Filter log", "", true, cx);
        let message = field(
            "Message",
            "Enter a sample message",
            "Check inventory",
            false,
            cx,
        );
        let delay = field("Delay in milliseconds", "Delay in ms", "400", false, cx);
        observe_input(&search, cx);
        observe_input(&message, cx);
        let log = [
            (
                "14:32:18.102",
                "In",
                r#"{"room":"abbey","cell":"causeway"}"#,
            ),
            (
                "14:32:18.346",
                "In",
                r#"{"player":"Vesper","cell":"causeway"}"#,
            ),
            ("14:32:19.004", "Out", r#"{"request":"inventory"}"#),
            ("14:32:19.228", "In", r#"{"items":34,"temporary":2}"#),
            ("14:32:20.015", "Out", r#"{"cell":"west gate"}"#),
            ("14:32:20.357", "In", r#"{"cell":"west gate","players":2}"#),
            (
                "14:32:21.112",
                "In",
                r#"{"quest":"Recover the relics","current":12,"total":15}"#,
            ),
            (
                "14:32:22.058",
                "In",
                r#"{"item":"Relic fragment","quantity":1}"#,
            ),
        ]
        .into_iter()
        .map(|(time, direction, payload)| LogRow {
            time,
            direction,
            payload: payload.into(),
        })
        .collect();
        Self {
            target,
            page: 0,
            direction: 0,
            selected: Some(6),
            paused: false,
            log,
            queue: vec!["Check inventory".into(), "Read quest progress".into()],
            queue_selected: Some(0),
            search,
            message,
            delay,
        }
    }

    #[cfg(test)]
    pub(in crate::storyboard) fn preview_send(mut self) -> Self {
        self.page = 1;
        self
    }

    fn log(&self, tabs: Tabs, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let rows: Rc<[(usize, LogRow)]> = self
            .log
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, item)| {
                (self.direction == 0
                    || (self.direction == 1 && item.direction == "In")
                    || (self.direction == 2 && item.direction == "Out"))
                    && item.payload.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>()
            .into();
        let selected = self
            .selected
            .and_then(|id| rows.iter().position(|(index, _)| *index == id));
        let pick = rows.clone();
        let list = if rows.is_empty() {
            empty(
                "No packets shown",
                "Change the filters or resume capture.",
                cx,
            )
            .into_any_element()
        } else {
            let rows = rows.clone();
            let color = palette(cx).muted_foreground;
            v_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .px(px(12.0))
                .pt(px(4.0))
                .child(
                    List::new("log", "Packet log", rows.len(), move |index| {
                        let row = &rows[index].1;
                        ListItem::new(format!("{} {} {}", row.time, row.direction, row.payload))
                            .child(
                                h_flex()
                                    .gap(px(8.0))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .w(px(86.0))
                                            .flex_none()
                                            .font_family(SYSTEM_MONO)
                                            .text_size(px(11.0))
                                            .text_color(color)
                                            .child(row.time),
                                    )
                                    .child(
                                        div()
                                            .w(px(26.0))
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(color)
                                            .child(row.direction),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(SYSTEM_MONO)
                                            .child(row.payload.clone()),
                                    ),
                            )
                    })
                    .fill()
                    .selected(selected)
                    .on_select(choose(cx, move |view, index: usize, _| {
                        view.selected = pick.get(index).map(|(id, _)| *id);
                    })),
                )
                .into_any_element()
        };
        let side = window_width(Screen::Packets, window) >= px(760.0);
        let detail = selected
            .and_then(|index| rows.get(index).map(|(_, row)| row))
            .map(|row| {
                let copy = row.payload.clone();
                let contents = serde_json::from_str::<serde_json::Value>(&row.payload)
                    .ok()
                    .and_then(|value| serde_json::to_string_pretty(&value).ok())
                    .map_or_else(|| row.payload.clone(), SharedString::from);
                v_flex()
                    .flex_none()
                    .min_h_0()
                    .border_color(palette(cx).border)
                    .map(|panel| {
                        if side {
                            panel.w(px(340.0)).border_l_1()
                        } else {
                            panel.h(relative(0.4)).border_t_1()
                        }
                    })
                    .child(
                        h_flex()
                            .flex_none()
                            .gap(px(8.0))
                            .px(px(12.0))
                            .py(px(4.0))
                            .child(title(if row.direction == "In" {
                                "Inbound"
                            } else {
                                "Outbound"
                            }))
                            .child(meta(row.time, cx))
                            .child(spring())
                            .child(
                                command("copy", "Copy", Variant::Ghost, move |_, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy.to_string(),
                                    ));
                                    Toaster::push(Toast::new("Packet copied"), cx);
                                })
                                .size(Size::Small),
                            )
                            .child(
                                Button::icon("close-inspector", Icon::X, "Close inspector")
                                    .size(Size::Small)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.selected = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        scroll("packet-text")
                            .px(px(12.0))
                            .pt(px(8.0))
                            .font_family(SYSTEM_MONO)
                            .child(contents),
                    )
            });
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                compact_toolbar()
                    .child(tabs)
                    .child(
                        Select::new(
                            "direction",
                            "Direction",
                            ["All directions", "Incoming", "Outgoing"],
                            Some(self.direction),
                        )
                        .width(px(130.0))
                        .on_select(choose(cx, |view, filter, _| view.direction = filter)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(96.0))
                            .max_w(px(208.0))
                            .child(self.search.clone()),
                    )
                    .child(spring())
                    .when(
                        StatusPreview::current(cx) == StatusPreview::Current,
                        |bar| {
                            bar.child(status(
                                if self.paused {
                                    "Capture paused"
                                } else {
                                    "Capturing"
                                },
                                self.paused,
                                cx,
                            ))
                        },
                    )
                    .child(
                        command(
                            "capture",
                            if self.paused { "Resume" } else { "Pause" },
                            Variant::Outline,
                            change(cx, |view, _| view.paused = !view.paused),
                        )
                        .size(Size::Small),
                    )
                    .child(
                        command(
                            "clear",
                            "Clear log",
                            Variant::Ghost,
                            change(cx, |view, _| {
                                view.log.clear();
                                view.selected = None;
                            }),
                        )
                        .size(Size::Small)
                        .disabled(self.log.is_empty()),
                    ),
            )
            .children(
                status_preview(
                    "Starting capture…",
                    "Capture interrupted",
                    "The game session disconnected. The previous log is still available.",
                    cx,
                )
                .map(|message| message.px(px(16.0)).py(px(8.0))),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .map(|body| {
                        if side {
                            body.flex_row()
                        } else {
                            body.flex_col()
                        }
                    })
                    .child(list)
                    .children(detail),
            )
    }

    fn send(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let wide = window_width(Screen::Packets, window) >= px(784.0);
        let composer = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(12.0))
            .child(title("Compose"))
            .child(
                Field::new("Message", self.message.clone())
                    .description("Messages are simulated in this canvas."),
            )
            .child(Field::new("Delay (ms)", self.delay.clone()))
            .child(
                command(
                    "enqueue",
                    "Add to queue",
                    Variant::Outline,
                    change(cx, |view, cx| {
                        let message = view.message.read(cx).text().trim();
                        if !message.is_empty() {
                            view.queue.push(message.to_owned().into());
                        }
                    }),
                )
                .self_start()
                .disabled(self.message.read(cx).text().trim().is_empty()),
            );
        let queue = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.0))
            .child(title("Queue"))
            .child(
                Table::new(
                    "queue",
                    "Send queue",
                    vec![
                        Column::new("Order").numeric().width(px(56.0)),
                        Column::new("Message"),
                    ],
                    self.queue
                        .iter()
                        .enumerate()
                        .map(|(index, item)| vec![(index + 1).to_string().into(), item.clone()])
                        .collect(),
                )
                .selected(self.queue_selected)
                .on_select(choose(cx, |view, row, _| view.queue_selected = Some(row))),
            )
            .child(
                command(
                    "remove-selected",
                    "Remove selected",
                    Variant::Ghost,
                    change(cx, |view, _| {
                        if let Some(index) = view
                            .queue_selected
                            .take()
                            .filter(|index| *index < view.queue.len())
                        {
                            view.queue.remove(index);
                        }
                    }),
                )
                .self_start()
                .disabled(self.queue_selected.is_none() || self.queue.is_empty()),
            );
        v_flex()
            .flex_1()
            .min_h_0()
            .child(scroll("send-body").p(px(16.0)).child(if wide {
                h_flex()
                    .items_start()
                    .gap(px(24.0))
                    .child(composer)
                    .child(queue)
            } else {
                v_flex().gap(px(24.0)).child(composer).child(queue)
            }))
            .child(
                footer(cx)
                    .children(status_preview(
                        "Sending queued messages…",
                        "Queue paused",
                        "The session disconnected. Reconnect to retry the remaining messages.",
                        cx,
                    ))
                    .child(spring())
                    .child(
                        command(
                            "send",
                            "Run queue",
                            Variant::Primary,
                            change(cx, |view, cx| {
                                let count = view.queue.len();
                                if !view.paused {
                                    view.log.extend(view.queue.iter().cloned().map(|message| {
                                        LogRow {
                                            time: "14:32:23.005",
                                            direction: "Out",
                                            payload: message,
                                        }
                                    }));
                                }
                                view.queue.clear();
                                view.queue_selected = None;
                                Toaster::push(Toast::new(format!("Simulated {count} sends")), cx);
                            }),
                        )
                        .disabled(
                            self.queue.is_empty()
                                || StatusPreview::current(cx) == StatusPreview::Working,
                        ),
                    ),
            )
    }
}

impl Render for Packets {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.queue.len();
        let tabs = Tabs::new("pages", "Packet views", 2, move |index| {
            if index == 0 {
                Tab::new("Log")
            } else {
                Tab::new("Send").end_slot(Badge::new(count.to_string()))
            }
        })
        .selected(self.page)
        .on_select(choose(cx, |view, page, _| view.page = page));
        let body = if !self.target.read(cx).available(cx) {
            empty(
                "No tabs open",
                "Open a game session to inspect its log.",
                cx,
            )
            .into_any_element()
        } else if self.page == 0 {
            self.log(tabs, window, cx).into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .child(compact_toolbar().child(tabs))
                .child(self.send(window, cx))
                .into_any_element()
        };
        frame(
            tool_chrome(Screen::Packets, Some(self.target.clone()), cx),
            cx,
        )
        .child(body)
    }
}
