use std::time::{Duration, SystemTime};

use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, EntityId, FocusHandle,
    Focusable as _, Global, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone,
    components::{Empty, LogEntry, LogView},
    primitives::{
        Badge, Button, Field, Input, InputEvent, List, ListItem, MenuItem, Switch, Tab, Tabs,
        ToggleGroup,
        button::{Size, Variant},
    },
    theme::{LINE_SMALL, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL},
};

use super::{
    Client, RuffleView,
    client::{CloseSession, bar_title, drag_area, window_bar},
    packet_log::{Filter, Line, Packet, PacketLog, Visible},
    shown_tab::{self, ShownTab, watch_changes},
};
use crate::{
    automation::{
        settings::Automation,
        spammer::{self, Status},
    },
    bridge::packets::Sender,
    domain::packets::{self as packet, Delivery, Target},
    ruffle::Command,
};

actions!(mira, [OpenPackets]);

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 640.0;
const TARGETS: [Target; 2] = [Target::Server, Target::Client];

struct Open(WindowHandle<Packets>);

impl Global for Open {}

pub(crate) fn open(client: Entity<Client>, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |packets, window, cx| {
                window.activate_window();
                packets.show_active(cx);
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Packets".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let packets = cx.new(|cx| Packets::new(client, window, cx));
        window.focus(&packets.read(cx).focus.clone(), cx);
        packets
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open packets"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Log,
    Spammer,
}

pub(crate) struct Packets {
    client: Entity<Client>,
    focus: FocusHandle,
    shown: Option<ShownTab>,
    log: Option<Entity<PacketLog>>,
    _log_changes: Option<Subscription>,
    page: Page,
    filter: Filter,
    visible: Visible,
    selected: Option<u64>,
    times: bool,
    search: Entity<Input>,
    composer: Entity<Input>,
    target: Target,
    notice: Option<SharedString>,
    sending: Option<Task<()>>,
    adding: Entity<Input>,
    editing: Option<usize>,
    queued: Option<usize>,
    delay: Entity<Input>,
    _subscriptions: Vec<Subscription>,
}

impl Packets {
    fn new(client: Entity<Client>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            Input::new("Search packets", cx)
                .placeholder("Search")
                .search()
        });
        let composer = cx.new(|cx| Input::new("Packet", cx).placeholder("%xt%zm%cmd%{room}%…"));
        let adding =
            cx.new(|cx| Input::new("Packet to add", cx).placeholder("%xt%zm%cmd%{room}%…"));
        let delay = cx.new(|cx| {
            Input::new("Milliseconds between packets", cx)
                .placeholder("1000")
                .max_length(5)
        });
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event, cx| {
                if let InputEvent::Changed = event {
                    this.filter.query = input.read(cx).text().to_owned();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&composer, window, |this, input, event, _, cx| match event {
                InputEvent::Changed => {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                    this.notice = None;
                }
                InputEvent::Submitted => this.send_composed(cx),
            }),
            cx.subscribe(&adding, |this, input, event, cx| match event {
                InputEvent::Changed => input.update(cx, |input, cx| input.set_invalid(false, cx)),
                InputEvent::Submitted => this.add_typed(cx),
            }),
            cx.subscribe(&delay, |this, input, event, cx| {
                if let InputEvent::Changed = event {
                    let delay = parse_delay(input.read(cx).text());
                    input.update(cx, |input, cx| input.set_invalid(delay.is_none(), cx));
                    if let (Some(delay), Some(automation)) = (delay, this.automation()) {
                        automation.update_spammer(|settings| settings.delay = delay);
                    }
                }
            }),
            cx.observe(&client, |this, _, cx| this.show(None, cx)),
            cx.on_release(|this, cx| {
                for view in this.client.read(cx).views() {
                    view.read(cx).automation.set_capture(false);
                }
            }),
        ];
        let mut packets = Self {
            client,
            focus: cx.focus_handle(),
            shown: None,
            log: None,
            _log_changes: None,
            page: Page::Log,
            filter: Filter::default(),
            visible: Visible::default(),
            selected: None,
            times: false,
            search,
            composer,
            target: Target::Server,
            notice: None,
            sending: None,
            adding,
            editing: None,
            queued: None,
            delay,
            _subscriptions: subscriptions,
        };
        packets.show_active(cx);
        packets
    }

    fn show_active(&mut self, cx: &mut Context<Self>) {
        let active = shown_tab::active(&self.client, cx);
        self.show(active, cx);
    }

    fn show(&mut self, wanted: Option<EntityId>, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &self.client,
            wanted,
            |this: &mut Self, cx| this.show(None, cx),
            |automation, cx| {
                vec![
                    watch_changes(automation.watch_capture(), cx),
                    watch_changes(automation.watch_spammer(), cx),
                    watch_changes(automation.watch_spammer_status(), cx),
                ]
            },
            cx,
        );
        if !switched {
            return;
        }
        self.log = self
            .shown
            .as_ref()
            .and_then(|shown| shown.view.upgrade())
            .map(|view| view.read(cx).packets.clone());
        self._log_changes = self
            .log
            .as_ref()
            .map(|log| cx.observe(log, |_, _, cx| cx.notify()));
        self.visible = Visible::default();
        self.selected = None;
        self.editing = None;
        self.queued = None;
        self.notice = None;
        let delay = self
            .automation()
            .map(|automation| automation.spammer().delay.as_millis().to_string())
            .unwrap_or_default();
        self.delay.update(cx, |input, cx| input.set_text(delay, cx));
    }

    fn automation(&self) -> Option<&Automation> {
        self.shown.as_ref().map(|shown| &shown.automation)
    }

    fn view(&self) -> Option<Entity<RuffleView>> {
        self.shown.as_ref().and_then(|shown| shown.view.upgrade())
    }

    fn packet(&self, id: u64, cx: &App) -> Option<Packet> {
        match self.log.as_ref()?.read(cx).line(id)? {
            Line::Packet(packet) => Some(packet.clone()),
            Line::Lost { .. } => None,
        }
    }

    fn send(&mut self, target: Target, text: String, cx: &mut Context<Self>) {
        let Some(view) = self.view() else {
            return;
        };
        let (sent, reply) = async_channel::bounded(1);
        view.update(cx, |view, _| {
            view.session.command(Command::SendPacket {
                target,
                packet: text,
                sent,
            })
        });
        self.notice = None;
        self.sending = Some(cx.spawn(async move |this, cx| {
            let notice: SharedString = match reply.recv().await {
                Ok(Ok(Delivery::Sent)) => "Sent".into(),
                Ok(Ok(Delivery::NotInGame)) => "Not in game".into(),
                Ok(Ok(Delivery::Refused(problem))) => problem.message().into(),
                Ok(Err(error)) => error.to_string().into(),
                Err(_) => "The tab closed".into(),
            };
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(notice);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn send_composed(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_owned();
        if let Err(problem) = packet::check(self.target, &text) {
            self.composer
                .update(cx, |input, cx| input.set_invalid(true, cx));
            self.notice = Some(problem.message().into());
            cx.notify();
            return;
        }
        self.send(self.target, text, cx);
    }

    fn compose(
        &mut self,
        target: Target,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.target = target;
        self.page = Page::Log;
        self.composer
            .update(cx, |input, cx| input.set_text(text, cx));
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    fn queue(&mut self, target: Target, text: String, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        let mut added = false;
        automation.update_spammer(|settings| {
            if settings.packets.is_empty() {
                settings.target = target;
            }
            if settings.target == target {
                settings.packets.push(text);
                added = true;
            }
        });
        self.notice = Some(if added {
            "Added to the spammer".into()
        } else {
            format!(
                "The spammer sends to the {}. Clear it to send to the {}.",
                target_label(automation.spammer().target).to_lowercase(),
                target_label(target).to_lowercase()
            )
            .into()
        });
        cx.notify();
    }

    fn add_typed(&mut self, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        let text = self.adding.read(cx).text().trim().to_owned();
        let target = automation.spammer().target;
        if packet::check(target, &text).is_err() {
            self.adding
                .update(cx, |input, cx| input.set_invalid(true, cx));
            return;
        }
        let editing = self.editing.take();
        automation.update_spammer(|settings| match editing {
            Some(index) if index < settings.packets.len() => settings.packets[index] = text,
            _ => settings.packets.push(text),
        });
        self.adding.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    fn edit_queued(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self
            .automation()
            .and_then(|automation| automation.spammer().packets.get(index).cloned())
        else {
            return;
        };
        self.editing = Some(index);
        self.queued = Some(index);
        self.adding.update(cx, |input, cx| input.set_text(text, cx));
        window.focus(&self.adding.focus_handle(cx), cx);
        cx.notify();
    }

    fn remove_queued(&mut self, cx: &mut Context<Self>) {
        let (Some(index), Some(automation)) = (self.queued, self.automation()) else {
            return;
        };
        automation.update_spammer(|settings| {
            if index < settings.packets.len() {
                settings.packets.remove(index);
            }
        });
        self.queued = None;
        self.editing = None;
        cx.notify();
    }

    fn lines_text(&self, times: bool, cx: &App) -> String {
        let Some(log) = &self.log else {
            return String::new();
        };
        let log = log.read(cx);
        self.visible
            .ids()
            .filter_map(|id| log.line(id))
            .map(|line| {
                let (at, tag, text) = describe(line);
                if times {
                    format!("{}  {tag:<6}  {text}", clock(at))
                } else {
                    format!("{tag:<6}  {text}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let text = self.lines_text(true, cx);
        let directory = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some("packets.txt"));
        cx.spawn(async move |this, cx| {
            let notice = match chosen.await {
                Ok(Ok(Some(path))) => {
                    let written = cx
                        .background_spawn(async move { std::fs::write(&path, text) })
                        .await;
                    match written {
                        Ok(()) => return,
                        Err(error) => format!("Couldn't save: {error}"),
                    }
                }
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => format!("Couldn't save: {error}"),
            };
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(notice.into());
                cx.notify();
            });
        })
        .detach();
    }

    fn line_menu(&self, this: WeakEntity<Self>, id: u64, cx: &App) -> Vec<MenuItem> {
        let Some(packet) = self.packet(id, cx) else {
            return Vec::new();
        };
        let copy = packet.text.to_string();
        let (target, replay) = replay(&packet);
        let mut items = vec![
            MenuItem::action("Copy", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
            }),
            MenuItem::action(
                match target {
                    Target::Server => "Send again",
                    Target::Client => "Send to client",
                },
                {
                    let this = this.clone();
                    let replay = replay.clone();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.send(target, replay.clone(), cx));
                    }
                },
            ),
            MenuItem::action("Edit and send…", {
                let this = this.clone();
                let replay = replay.clone();
                move |window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.compose(target, replay.clone(), window, cx)
                    });
                }
            }),
        ];
        if packet.from == Sender::Client {
            items.push(MenuItem::action("Add to spammer", move |_, cx| {
                let _ = this.update(cx, |this, cx| this.queue(target, replay.clone(), cx));
            }));
        }
        items
    }

    fn log_page(&mut self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let Some(log) = self.log.clone() else {
            return div();
        };
        self.visible.refresh(log.read(cx), &self.filter);
        let capturing = automation.capturing();
        let counts = {
            let log = log.read(cx);
            (
                log.count(Sender::Client),
                log.count(Sender::Server),
                log.len(),
            )
        };
        let selected = self.selected.and_then(|id| self.visible.index_of(id));
        let this = cx.weak_entity();
        let times = self.times;
        let toggles = {
            let side =
                |id: &'static str, label: &str, count: usize, on: bool, flip: fn(&mut Filter)| {
                    let this = this.clone();
                    Button::new(id, format!("{label} {count}"))
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .pressed(on)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                flip(&mut this.filter);
                                cx.notify();
                            });
                        })
                };
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(side(
                    "show-client",
                    "Client",
                    counts.0,
                    self.filter.client,
                    |filter| filter.client = !filter.client,
                ))
                .child(side(
                    "show-server",
                    "Server",
                    counts.1,
                    self.filter.server,
                    |filter| filter.server = !filter.server,
                ))
        };
        let capture = {
            let automation = automation.clone();
            Switch::new("capture", "Capture", capturing)
                .on_toggle(move |on, _, _| automation.set_capture(on))
        };
        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.0))
            .child(capture)
            .child(toggles)
            .child(div().flex_1())
            .child(
                Button::new("times", "Times")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .pressed(times)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.times = !this.times;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("copy-lines", "Copy")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .disabled(self.visible.len() == 0)
                    .tooltip("Copy the lines shown")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = this.lines_text(this.times, cx);
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    })),
            )
            .child(
                Button::new("export-lines", "Export…")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .disabled(self.visible.len() == 0)
                    .tooltip("Save the lines shown to a text file")
                    .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
            )
            .child(
                Button::new("clear-lines", "Clear")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .disabled(counts.2 == 0)
                    .on_click({
                        let log = log.clone();
                        cx.listener(move |this, _, _, cx| {
                            log.update(cx, |log, cx| {
                                log.clear();
                                cx.notify();
                            });
                            this.selected = None;
                        })
                    }),
            );
        let lines = if self.visible.len() == 0 {
            let (title, body) = match (counts.2, capturing) {
                (0, false) => (
                    "Capture is off",
                    "Turn on Capture to record this tab's packets.",
                ),
                (0, true) => ("Waiting for packets", "Packets show here as they're sent."),
                _ => (
                    "No packets match",
                    "Change the search or show both directions.",
                ),
            };
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .rounded(theme.radius.panel)
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.surface)
                .child(Empty::new(title, body))
                .into_any_element()
        } else {
            let rows = this.clone();
            let menus = this.clone();
            let picks = this.clone();
            let opens = this.clone();
            div()
                .flex_1()
                .min_h_0()
                .child(
                    LogView::new(
                        "packet-log",
                        "Packets",
                        self.visible.len(),
                        move |index, cx| {
                            rows.upgrade()
                                .and_then(|this| {
                                    let this = this.read(cx);
                                    let id = this.visible.id(index)?;
                                    let log = this.log.as_ref()?.read(cx);
                                    Some(entry(log.line(id)?, times))
                                })
                                .unwrap_or_else(|| LogEntry::new("", ""))
                        },
                    )
                    .dropped(self.visible.dropped())
                    .selected(selected)
                    .on_select(move |index, _, cx| {
                        let _ = picks.update(cx, |this, cx| {
                            this.selected = this.visible.id(index);
                            cx.notify();
                        });
                    })
                    .on_activate(move |index, window, cx| {
                        let _ = opens.update(cx, |this, cx| {
                            if let Some(packet) =
                                this.visible.id(index).and_then(|id| this.packet(id, cx))
                            {
                                let (target, text) = replay(&packet);
                                this.compose(target, text, window, cx);
                            }
                        });
                    })
                    .menu(move |index, cx| {
                        menus
                            .upgrade()
                            .and_then(|this| {
                                let this = this.read(cx);
                                Some(this.line_menu(menus.clone(), this.visible.id(index)?, cx))
                            })
                            .unwrap_or_default()
                    }),
                )
                .into_any_element()
        };
        let detail = self
            .selected
            .and_then(|id| self.packet(id, cx))
            .map(|packet| detail(theme, &packet, cx));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(12.0))
            .child(toolbar)
            .child(self.search.clone())
            .child(lines)
            .children(detail)
            .child(self.composer_bar(theme, cx))
    }

    fn composer_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let this = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        ToggleGroup::new(
                            "send-target",
                            "Send to",
                            TARGETS.map(target_label).map(SharedString::from).to_vec(),
                            TARGETS.iter().position(|t| *t == self.target).unwrap_or(0),
                        )
                        .on_select(move |index, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.target = TARGETS[index];
                                this.composer
                                    .update(cx, |input, cx| input.set_invalid(false, cx));
                                cx.notify();
                            });
                        }),
                    )
                    .child(div().flex_1().min_w_0().child(self.composer.clone()))
                    .child(
                        Button::new("send-packet", "Send")
                            .size(Size::Small)
                            .variant(Variant::Primary)
                            .on_click(cx.listener(|this, _, _, cx| this.send_composed(cx))),
                    )
                    .child(
                        Button::new("queue-packet", "Add to spammer")
                            .size(Size::Small)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = this.composer.read(cx).text().trim().to_owned();
                                if let Err(problem) = packet::check(this.target, &text) {
                                    this.notice = Some(problem.message().into());
                                    cx.notify();
                                    return;
                                }
                                this.queue(this.target, text, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_SMALL)
                    .text_color(theme.text_tertiary)
                    .child(self.notice.clone().unwrap_or_else(|| PLACEHOLDERS.into())),
            )
    }

    fn spammer_page(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let settings = automation.spammer();
        let status = spammer_status(&settings, automation.spammer_status());
        let switching = automation.clone();
        let send = Switch::new("spammer-enabled", "Send", settings.enabled)
            .description(status)
            .disabled(settings.packets.is_empty() && !settings.enabled)
            .on_toggle(move |on, _, _| {
                switching.update_spammer(|settings| settings.enabled = on);
            });
        let targeting = automation.clone();
        let target = ToggleGroup::new(
            "spammer-target",
            "Send to",
            TARGETS.map(target_label).map(SharedString::from).to_vec(),
            TARGETS
                .iter()
                .position(|target| *target == settings.target)
                .unwrap_or(0),
        )
        .on_select(move |index, _, _| {
            targeting.update_spammer(|settings| settings.target = TARGETS[index]);
        });
        let packets: std::rc::Rc<[String]> = settings.packets.clone().into();
        let reordering = automation.clone();
        let this = cx.weak_entity();
        let picks = this.clone();
        let moves = this.clone();
        let list = if settings.packets.is_empty() {
            div()
                .rounded(theme.radius.panel)
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.surface)
                .child(Empty::new(
                    "Nothing to send",
                    "Add packets below, or right-click a client packet in the log.",
                ))
                .into_any_element()
        } else {
            List::new(
                "spammer-packets",
                "Packets to send",
                packets.len(),
                move |index| {
                    ListItem::new(packets[index].clone()).end_slot(format!("{:02}", index + 1))
                },
            )
            .selected(self.queued)
            .height(px(220.0))
            .on_reorder(move |from, to, _, cx| {
                reordering.update_spammer(|settings| {
                    let moved = settings.packets.remove(from);
                    settings.packets.insert(to, moved);
                });
                let _ = moves.update(cx, |this, cx| {
                    this.queued = Some(to);
                    this.editing = None;
                    cx.notify();
                });
            })
            .on_select(move |index, _, cx| {
                let _ = picks.update(cx, |this, cx| {
                    this.queued = Some(index);
                    cx.notify();
                });
            })
            .on_activate(move |index, window, cx| {
                let _ = this.update(cx, |this, cx| this.edit_queued(index, window, cx));
            })
            .into_any_element()
        };
        let editing = self.editing.is_some();
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(send)
            .child(Field::new("Send to", target))
            .child(Field::new("Every", self.delay.clone()).description(format!(
                "Milliseconds between packets, {} to {}.",
                spammer::DELAY_MS.0,
                thousands(spammer::DELAY_MS.1)
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(list)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(div().flex_1().min_w_0().child(self.adding.clone()))
                            .child(
                                Button::new("spammer-add", if editing { "Save" } else { "Add" })
                                    .size(Size::Small)
                                    .on_click(cx.listener(|this, _, _, cx| this.add_typed(cx))),
                            )
                            .child(
                                Button::new("spammer-remove", "Remove")
                                    .size(Size::Small)
                                    .variant(Variant::Ghost)
                                    .disabled(self.queued.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| this.remove_queued(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .line_height(LINE_SMALL)
                            .text_color(theme.text_tertiary)
                            .child(PLACEHOLDERS),
                    ),
            )
    }
}

fn replay(packet: &Packet) -> (Target, String) {
    match packet.from {
        Sender::Client => (Target::Server, packet::with_room_placeholder(&packet.text)),
        Sender::Server => (Target::Client, packet.text.to_string()),
    }
}

const PLACEHOLDERS: &str = "{room}, {map} and {player} fill in when the packet is sent.";

fn target_label(target: Target) -> &'static str {
    match target {
        Target::Server => "Server",
        Target::Client => "Client",
    }
}

fn thousands(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn parse_delay(text: &str) -> Option<Duration> {
    let ms: u64 = text.trim().parse().ok()?;
    (spammer::DELAY_MS.0..=spammer::DELAY_MS.1)
        .contains(&ms)
        .then(|| Duration::from_millis(ms))
}

fn spammer_status(settings: &spammer::Settings, status: Status) -> String {
    let every = match settings.delay.as_millis() {
        ms if ms % 1000 == 0 => format!("{} s", ms / 1000),
        ms => format!("{ms} ms"),
    };
    match status {
        Status::Failed(reason) => format!("Stopped: {reason}"),
        _ if !settings.enabled => "Sends each packet in order, then starts over.".into(),
        Status::Sent { index, count } => format!("Sent {} of {count} · every {every}", index + 1),
        Status::NotInGame => "Not in game · waiting".into(),
        Status::Idle => format!("Every {every}"),
    }
}

fn clock(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%H:%M:%S%.3f")
        .to_string()
}

fn describe(line: &Line) -> (SystemTime, &'static str, SharedString) {
    match line {
        Line::Packet(packet) => (
            packet.at,
            match packet.from {
                Sender::Client => "client",
                Sender::Server => "server",
            },
            packet.text.clone(),
        ),
        Line::Lost { at } => (*at, "lost", LOST.into()),
    }
}

const LOST: &str = "Packets were dropped here because the log fell behind";

fn entry(line: &Line, times: bool) -> LogEntry {
    let (at, entry) = match line {
        Line::Packet(packet) => {
            let (tag, tone) = match packet.from {
                Sender::Client => ("client", Tone::Accent),
                Sender::Server => ("server", Tone::Neutral),
            };
            (
                packet.at,
                LogEntry::new(tag, packet.preview.clone()).tone(tone),
            )
        }
        Line::Lost { at } => (*at, LogEntry::new("lost", LOST).tone(Tone::Warning)),
    };
    if times { entry.time(clock(at)) } else { entry }
}

fn detail(theme: &Theme, packet: &Packet, cx: &mut Context<Packets>) -> Div {
    let text: SharedString = if packet.text.starts_with('{') {
        serde_json::from_str::<serde_json::Value>(&packet.text)
            .ok()
            .and_then(|json| serde_json::to_string_pretty(&json).ok())
            .map_or_else(|| packet.text.clone(), SharedString::from)
    } else {
        packet.text.clone()
    };
    let copy = packet.text.to_string();
    let heading = format!(
        "{} · {} · {} characters",
        match packet.from {
            Sender::Client => "Client",
            Sender::Server => "Server",
        },
        clock(packet.at),
        thousands(packet.text.chars().count() as u64)
    );
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(TEXT_SMALL)
                        .line_height(LINE_SMALL)
                        .text_color(theme.text_secondary)
                        .child(heading),
                )
                .child(
                    Button::new("copy-packet", "Copy")
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                )
                .child(
                    Button::new("close-packet", "Close")
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected = None;
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .id("packet-detail")
                .h(px(140.0))
                .overflow_y_scroll()
                .p(px(10.0))
                .rounded(theme.radius.panel)
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.surface)
                .font_family(mira_ui::theme::SYSTEM_MONO)
                .text_size(TEXT_CAPTION)
                .line_height(px(17.0))
                .text_color(theme.text_secondary)
                .child(text),
        )
}

impl Render for Packets {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let picker = shown_tab::picker(
            "packets-tab",
            &self.client,
            self.shown.as_ref(),
            |this: &mut Self, id, cx| this.show(Some(id), cx),
            cx,
        );
        let automation = self.automation().cloned();
        let queued = automation
            .as_ref()
            .map_or(0, |automation| automation.spammer().packets.len());
        let labels = [("Log", 0), ("Spammer", queued)];
        let body = match (&automation, self.page) {
            (None, _) => div()
                .py(px(24.0))
                .text_center()
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
                .text_color(theme.text_tertiary)
                .child("No tabs open"),
            (Some(automation), Page::Log) => self.log_page(&theme, automation, cx),
            (Some(automation), Page::Spammer) => self.spammer_page(&theme, automation, cx),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .id("packets")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .child(mira_ui::nav::pointer_probe())
            .child(
                window_bar(window, cx)
                    .child(bar_title("Packets", cx))
                    .child(drag_area().flex_1())
                    .child(picker),
            )
            .child(
                div().flex_none().px(px(20.0)).child(
                    Tabs::new("packets-pages", "Pages", labels.len(), move |index| {
                        let (name, count) = labels[index];
                        let tab = Tab::new(name);
                        if count == 0 {
                            tab
                        } else {
                            tab.end_slot(Badge::new(count.to_string()))
                        }
                    })
                    .selected(self.page as usize)
                    .on_select({
                        let this = cx.weak_entity();
                        move |index, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.page = [Page::Log, Page::Spammer][index];
                                cx.notify();
                            });
                        }
                    }),
                ),
            )
            .child(
                div()
                    .id("packets-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .when(self.page == Page::Spammer, |body| body.overflow_y_scroll())
                    .p(px(20.0))
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spammer_delay_is_whole_milliseconds_in_range() {
        assert_eq!(parse_delay(" 250 "), Some(Duration::from_millis(250)));
        assert_eq!(parse_delay("9"), None);
        assert_eq!(parse_delay("60001"), None);
        assert_eq!(parse_delay("1.5"), None);
    }
}
