use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, FocusHandle,
    Focusable as _, FontWeight, Global, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions, actions, div, prelude::FluentBuilder as _, px, relative, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Align, Badge, Button, DropdownMenu, Icon, Input, InputEvent, List, ListItem, MenuItem,
        Numeric, Switch, Tab, Tabs, ToggleGroup,
        button::{Size, Variant},
    },
    theme::{SYSTEM_MONO, TEXT_BODY, TEXT_CAPTION},
    v_flex,
};

use super::{
    Client, RuffleView,
    client::CloseSession,
    packet_lines::{PacketLines, Row},
    packet_log::{Filter, Line, Packet, PacketLog, Visible},
    shown_tab::{self, ShownTab, watch_changes},
};
use crate::{
    automation::{
        settings::Automation,
        spammer::{self, Status},
    },
    bridge::packets::Sender,
    domain::packets::{self as packet, Delivery, Problem, Target},
    ruffle::Command,
};

actions!(mira, [OpenPackets]);

const WIDTH: f32 = 900.0;
const HEIGHT: f32 = 640.0;
const MIN_WIDTH: f32 = 560.0;
const MIN_HEIGHT: f32 = 420.0;
const SIDE_PANEL_FROM: Pixels = px(760.0);
const PANEL_WIDTH: Pixels = px(340.0);
const DELAY_WIDTH: Pixels = px(104.0);
const TARGETS: [Target; 2] = [Target::Server, Target::Client];
const EXAMPLE: &str = "%xt%zm%cmd%{room}%…";
const PLACEHOLDERS: &str = "{room}, {map} and {player} fill in when the packet is sent.";

struct Open(WindowHandle<Packets>);

impl Global for Open {}

pub(crate) fn open(client: Entity<Client>, target: shown_tab::Target, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |packets, window, cx| {
                window.activate_window();
                packets.retarget(target, cx);
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
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
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
        let packets = cx.new(|cx| Packets::new(client, target, window, cx));
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
    Send,
}

struct Notice {
    text: SharedString,
    tone: Tone,
}

impl Notice {
    fn new(text: impl Into<SharedString>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

pub(crate) struct Packets {
    client: Entity<Client>,
    focus: FocusHandle,
    tab: shown_tab::Target,
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
    notice: Option<Notice>,
    sending: Option<Task<()>>,
    adding: Entity<Input>,
    add_problem: Option<Problem>,
    editing: Option<usize>,
    queued: Option<usize>,
    delay: Entity<Input>,
    _subscriptions: Vec<Subscription>,
}

impl Packets {
    fn new(
        client: Entity<Client>,
        tab: shown_tab::Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            Input::new("Search packets", cx)
                .placeholder("Search")
                .search()
                .small()
        });
        let composer = cx.new(|cx| Input::new("Packet", cx).placeholder(EXAMPLE));
        let adding = cx.new(|cx| Input::new("Packet to queue", cx).placeholder(EXAMPLE));
        let delay = cx.new(|cx| {
            Input::new("Milliseconds between packets", cx)
                .numeric(Numeric {
                    min: spammer::DELAY_MS.0 as i64,
                    max: spammer::DELAY_MS.1 as i64,
                    step: 100,
                })
                .max_length(5)
                .suffix("ms")
        });
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event, cx| {
                if let InputEvent::Changed = event {
                    this.filter.query = input.read(cx).text().to_owned();
                    cx.notify();
                }
            }),
            cx.subscribe(&composer, |this, input, event, cx| match event {
                InputEvent::Changed => {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                    this.notice = None;
                    cx.notify();
                }
                InputEvent::Submitted => this.send_composed(cx),
            }),
            cx.subscribe(&adding, |this, input, event, cx| match event {
                InputEvent::Changed => {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                    this.add_problem = None;
                    cx.notify();
                }
                InputEvent::Submitted => this.add_typed(cx),
            }),
            cx.subscribe(&delay, |this, _, event, cx| {
                if let InputEvent::Submitted = event {
                    this.commit_delay(cx);
                }
            }),
            cx.on_blur(&delay.focus_handle(cx), window, |this, _, cx| {
                this.commit_delay(cx)
            }),
            cx.observe(&client, |this, _, cx| this.show(cx)),
            cx.on_release(|this, cx| {
                for view in this.client.read(cx).views() {
                    view.read(cx).automation.set_capture(false);
                }
            }),
        ];
        let mut packets = Self {
            client,
            focus: cx.focus_handle(),
            tab,
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
            add_problem: None,
            editing: None,
            queued: None,
            delay,
            _subscriptions: subscriptions,
        };
        packets.show(cx);
        packets
    }

    fn retarget(&mut self, tab: shown_tab::Target, cx: &mut Context<Self>) {
        self.tab = tab;
        self.show(cx);
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &mut self.tab,
            &self.client,
            |this: &mut Self, cx| this.show(cx),
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
        self.queued = None;
        self.notice = None;
        self.cancel_edit(cx);
        self.revert_delay(cx);
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
            let notice = match reply.recv().await {
                Ok(Ok(Delivery::Sent)) => Notice::new("Sent", Tone::Accent),
                Ok(Ok(Delivery::NotInGame)) => Notice::new("Not in game", Tone::Warning),
                Ok(Ok(Delivery::Refused(problem))) => Notice::new(problem.message(), Tone::Danger),
                Ok(Err(error)) => Notice::new(error.to_string(), Tone::Danger),
                Err(_) => Notice::new("The tab closed", Tone::Danger),
            };
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(notice);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn typed(input: &Entity<Input>, cx: &App) -> String {
        input.read(cx).text().trim().to_owned()
    }

    fn send_composed(&mut self, cx: &mut Context<Self>) {
        let text = Self::typed(&self.composer, cx);
        match packet::check(self.target, &text) {
            Ok(()) => self.send(self.target, text, cx),
            Err(Problem::Empty) => {}
            Err(problem) => {
                self.composer
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                self.notice = Some(Notice::new(problem.message(), Tone::Danger));
                cx.notify();
            }
        }
    }

    fn queue_composed(&mut self, cx: &mut Context<Self>) {
        let text = Self::typed(&self.composer, cx);
        match packet::check(self.target, &text) {
            Ok(()) => self.queue(self.target, text, cx),
            Err(Problem::Empty) => {}
            Err(problem) => {
                self.notice = Some(Notice::new(problem.message(), Tone::Danger));
                cx.notify();
            }
        }
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
        let settings = automation.update_spammer(|settings| {
            if settings.packets.is_empty() {
                settings.target = target;
            }
            if settings.target == target {
                settings.packets.push(text);
                added = true;
            }
        });
        self.notice = Some(if added {
            Notice::new("Added to the queue", Tone::Accent)
        } else {
            Notice::new(
                format!(
                    "The queue sends to the {}, not the {}",
                    target_word(settings.target),
                    target_word(target)
                ),
                Tone::Warning,
            )
        });
        cx.notify();
    }

    fn add_typed(&mut self, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        let text = Self::typed(&self.adding, cx);
        match packet::check(automation.spammer().target, &text) {
            Ok(()) => {}
            Err(Problem::Empty) => return,
            Err(problem) => {
                self.adding
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                self.add_problem = Some(problem);
                cx.notify();
                return;
            }
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

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        self.add_problem = None;
        self.adding.update(cx, |input, cx| {
            input.set_invalid(false, cx);
            input.set_text("", cx);
        });
        cx.notify();
    }

    fn remove_queued(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        let left = automation
            .update_spammer(|settings| {
                if index < settings.packets.len() {
                    settings.packets.remove(index);
                }
            })
            .packets
            .len();
        self.queued = (left > 0).then(|| index.min(left - 1));
        match self.editing {
            Some(editing) if editing == index => self.cancel_edit(cx),
            Some(editing) if editing > index => self.editing = Some(editing - 1),
            _ => {}
        }
        cx.notify();
    }

    fn move_queued(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        automation.update_spammer(|settings| {
            if from < settings.packets.len() && to < settings.packets.len() {
                let moved = settings.packets.remove(from);
                settings.packets.insert(to, moved);
            }
        });
        self.queued = Some(to);
        self.editing = self.editing.map(|editing| moved_index(editing, from, to));
        cx.notify();
    }

    fn commit_delay(&mut self, cx: &mut Context<Self>) {
        if let (Some(delay), Some(automation)) =
            (parse_delay(self.delay.read(cx).text()), self.automation())
        {
            automation.update_spammer(|settings| settings.delay = delay);
        }
        self.revert_delay(cx);
    }

    fn saved_delay(&self) -> String {
        self.automation()
            .map(|automation| automation.spammer().delay.as_millis().to_string())
            .unwrap_or_default()
    }

    fn revert_delay(&mut self, cx: &mut Context<Self>) {
        let saved = self.saved_delay();
        self.delay.update(cx, |input, cx| {
            if input.text() != saved {
                input.set_text(saved, cx);
            }
        });
    }

    fn set_direction(&mut self, from: Option<Sender>, cx: &mut Context<Self>) {
        self.filter.from = from;
        self.search.update(cx, |input, cx| {
            input.set_placeholder(
                match from {
                    None => "Search",
                    Some(Sender::Client) => "Search outbound packets",
                    Some(Sender::Server) => "Search inbound packets",
                },
                cx,
            )
        });
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
                    format!("{}  {tag:<4}  {text}", clock(at))
                } else {
                    format!("{tag:<4}  {text}")
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
            let problem = match chosen.await {
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
                this.notice = Some(Notice::new(problem, Tone::Danger));
                cx.notify();
            });
        })
        .detach();
    }

    fn packet_actions(&self, this: &WeakEntity<Self>, packet: &Packet) -> Vec<MenuItem> {
        let (target, replay) = replay(packet);
        let mut items = vec![
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
            let this = this.clone();
            items.push(MenuItem::action("Add to queue", move |_, cx| {
                let _ = this.update(cx, |this, cx| this.queue(target, replay.clone(), cx));
            }));
        }
        items
    }

    fn line_menu(&self, this: WeakEntity<Self>, id: u64, cx: &App) -> Vec<MenuItem> {
        let Some(packet) = self.packet(id, cx) else {
            return Vec::new();
        };
        let copy = packet.text.to_string();
        let mut items = vec![MenuItem::action("Copy", move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
        })];
        items.extend(self.packet_actions(&this, &packet));
        let (label, from) = match self.filter.from {
            Some(_) => ("Show all packets", None),
            None => match packet.from {
                Sender::Client => ("Show only outbound", Some(Sender::Client)),
                Sender::Server => ("Show only inbound", Some(Sender::Server)),
            },
        };
        items.push(MenuItem::separator());
        items.push(MenuItem::action(label, move |_, cx| {
            let _ = this.update(cx, |this, cx| this.set_direction(from, cx));
        }));
        items
    }

    fn log_menu(
        &self,
        (outbound, inbound): (usize, usize),
        cx: &mut Context<Self>,
    ) -> DropdownMenu {
        let this = cx.weak_entity();
        let from = self.filter.from;
        let times = self.times;
        let shown = self.visible.len() > 0;
        let counts = [
            ("All packets", None, outbound + inbound),
            ("Outbound", Some(Sender::Client), outbound),
            ("Inbound", Some(Sender::Server), inbound),
        ];
        DropdownMenu::new("log-menu")
            .trigger(
                Button::icon("log-menu-trigger", Icon::Ellipsis, "Log options")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .pressed(from.is_some())
                    .hide_tooltip(),
            )
            .align(Align::End)
            .items(move |_, _| {
                let mut items = vec![MenuItem::header("Show")];
                items.extend(counts.iter().map(|&(label, choice, count)| {
                    let this = this.clone();
                    MenuItem::radio(label, from == choice, move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.set_direction(choice, cx));
                    })
                    .end_slot(Badge::new(count.to_string()))
                }));
                items.push(MenuItem::separator());
                items.push(MenuItem::switch("Show times", times, {
                    let this = this.clone();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.times = !this.times;
                            cx.notify();
                        });
                    }
                }));
                items.push(MenuItem::separator());
                items.push(
                    MenuItem::action("Copy shown packets", {
                        let this = this.clone();
                        move |_, cx| {
                            if let Some(text) = this
                                .upgrade()
                                .map(|this| this.read(cx).lines_text(times, cx))
                            {
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            }
                        }
                    })
                    .disabled(!shown),
                );
                items.push(
                    MenuItem::action("Export shown packets…", {
                        let this = this.clone();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.export(cx));
                        }
                    })
                    .disabled(!shown),
                );
                items
            })
    }

    fn log_page(
        &mut self,
        theme: &Theme,
        automation: &Automation,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(log) = self.log.clone() else {
            return div();
        };
        self.visible.refresh(log.read(cx), &self.filter);
        let capturing = automation.capturing();
        let (empty, counts) = {
            let log = log.read(cx);
            (
                log.len() == 0,
                (log.count(Sender::Client), log.count(Sender::Server)),
            )
        };
        let capture = {
            let automation = automation.clone();
            Switch::new("capture", "Capture", capturing)
                .on_toggle(move |on, _, _| automation.set_capture(on))
        };
        let toolbar = h_flex()
            .flex_none()
            .h(px(40.0))
            .px(px(12.0))
            .gap(px(8.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(capture)
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .child(
                Button::new("clear-lines", "Clear")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .disabled(empty)
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
            )
            .child(self.log_menu(counts, cx));
        let lines = self.lines(theme, empty, capturing, cx);
        let side = window.viewport_size().width >= SIDE_PANEL_FROM;
        let panel = self
            .selected
            .and_then(|id| self.packet(id, cx))
            .map(|packet| self.packet_panel(theme, &packet, side, cx));
        v_flex()
            .flex_1()
            .min_h_0()
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .when(side, |body| body.flex_row())
                    .when(!side, |body| body.flex_col())
                    .flex_1()
                    .min_h_0()
                    .child(div().flex_1().min_w_0().min_h_0().child(lines))
                    .children(panel),
            )
            .child(self.composer_bar(theme, cx))
    }

    fn lines(
        &self,
        theme: &Theme,
        empty: bool,
        capturing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.visible.len() == 0 {
            let (title, body) = match (empty, capturing) {
                (true, false) => (
                    "Capture is off",
                    "Turn on Capture to record this tab's packets.",
                ),
                (true, true) => ("Waiting for packets", "Packets show here as they're sent."),
                (false, _) => ("No packets match", "Change the search or show all packets."),
            };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.surface)
                .child(Empty::new(title, body))
                .into_any_element();
        }
        let times = self.times;
        let selected = self.selected.and_then(|id| self.visible.index_of(id));
        let rows = cx.weak_entity();
        let menus = cx.weak_entity();
        let picks = cx.weak_entity();
        let opens = cx.weak_entity();
        PacketLines::new(
            self.visible.len(),
            move |index, cx| {
                rows.upgrade()
                    .and_then(|this| {
                        let this = this.read(cx);
                        let id = this.visible.id(index)?;
                        let log = this.log.as_ref()?.read(cx);
                        Some(row(log.line(id)?, times))
                    })
                    .unwrap_or_else(|| Row {
                        time: None,
                        tag: "",
                        tone: Tone::Neutral,
                        text: SharedString::default(),
                        actions: false,
                    })
            },
            move |index, cx| {
                menus
                    .upgrade()
                    .and_then(|this| {
                        let this = this.read(cx);
                        Some(this.line_menu(menus.clone(), this.visible.id(index)?, cx))
                    })
                    .unwrap_or_default()
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
                if let Some(packet) = this.visible.id(index).and_then(|id| this.packet(id, cx)) {
                    let (target, text) = replay(&packet);
                    this.compose(target, text, window, cx);
                }
            });
        })
        .into_any_element()
    }

    fn packet_panel(
        &self,
        theme: &Theme,
        packet: &Packet,
        side: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let text: SharedString = if packet.text.starts_with('{') {
            serde_json::from_str::<serde_json::Value>(&packet.text)
                .ok()
                .and_then(|json| serde_json::to_string_pretty(&json).ok())
                .map_or_else(|| packet.text.clone(), SharedString::from)
        } else {
            packet.text.clone()
        };
        let direction = match packet.from {
            Sender::Client => "Outbound",
            Sender::Server => "Inbound",
        };
        let copy = packet.text.to_string();
        let this = cx.weak_entity();
        let menu_packet = packet.clone();
        let header = h_flex()
            .flex_none()
            .h(px(36.0))
            .pl(px(12.0))
            .pr(px(6.0))
            .gap(px(8.0))
            .border_b_1()
            .border_color(theme.hairline)
            .text_small()
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child(direction),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.text_tertiary)
                    .child(clock(packet.at)),
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
                DropdownMenu::new("packet-menu")
                    .trigger(
                        Button::icon("packet-menu-trigger", Icon::Ellipsis, "Packet actions")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .hide_tooltip(),
                    )
                    .align(Align::End)
                    .items(move |_, cx| {
                        this.upgrade()
                            .map(|packets| packets.read(cx).packet_actions(&this, &menu_packet))
                            .unwrap_or_default()
                    }),
            )
            .child(
                Button::icon("close-packet", Icon::X, "Close")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selected = None;
                        cx.notify();
                    })),
            );
        v_flex()
            .flex_none()
            .min_h_0()
            .bg(theme.surface)
            .border_color(theme.hairline)
            .map(|panel| {
                if side {
                    panel.w(PANEL_WIDTH).border_l_1()
                } else {
                    panel.h(relative(0.4)).border_t_1()
                }
            })
            .child(header)
            .child(
                div()
                    .id("packet-text")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(12.0))
                    .py(px(8.0))
                    .font_family(SYSTEM_MONO)
                    .text_size(TEXT_CAPTION)
                    .line_height(px(17.0))
                    .text_color(theme.text_secondary)
                    .child(text),
            )
    }

    fn composer_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let this = cx.weak_entity();
        let empty = Self::typed(&self.composer, cx).is_empty();
        v_flex()
            .flex_none()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(10.0))
            .border_t_1()
            .border_color(theme.hairline)
            .child(
                h_flex()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.text_secondary)
                            .child("Send to"),
                    )
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
                                this.notice = None;
                                this.composer
                                    .update(cx, |input, cx| input.set_invalid(false, cx));
                                cx.notify();
                            });
                        }),
                    )
                    .child(div().flex_1().min_w_0().child(self.composer.clone()))
                    .child(
                        Button::new("send-packet", "Send")
                            .variant(Variant::Primary)
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.send_composed(cx))),
                    )
                    .child(
                        Button::new("queue-packet", "Add to queue")
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.queue_composed(cx))),
                    ),
            )
            .child(note_line(
                theme,
                self.notice
                    .as_ref()
                    .map(|notice| (notice.text.clone(), tone_text(theme, notice.tone))),
            ))
    }

    fn send_page(&mut self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let settings = automation.spammer();
        let (status, status_color) = queue_status(&settings, automation.spammer_status(), theme);
        let switching = automation.clone();
        let switch = Switch::new("send-queue", "Send queue", settings.enabled)
            .disabled(settings.packets.is_empty() && !settings.enabled)
            .on_toggle(move |on, _, _| {
                switching.update_spammer(|settings| settings.enabled = on);
            });
        let targeting = automation.clone();
        let target = ToggleGroup::new(
            "queue-target",
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
        let delay = div()
            .flex_none()
            .w(DELAY_WIDTH)
            .child(self.delay.clone())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.revert_delay(cx);
                }
            }));
        let bar = h_flex()
            .flex_none()
            .h(px(44.0))
            .px(px(12.0))
            .gap(px(12.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(switch)
            .child(
                div()
                    .id("queue-status")
                    .role(Role::Status)
                    .aria_label(status.clone())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_small()
                    .text_color(status_color)
                    .child(status),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.text_secondary)
                    .child("Send to"),
            )
            .child(target)
            .child(
                div()
                    .flex_none()
                    .text_color(theme.text_secondary)
                    .child("Every"),
            )
            .child(delay);
        let list = if settings.packets.is_empty() {
            div()
                .text_small()
                .text_color(theme.text_tertiary)
                .child(
                    "The queue is empty. Add a packet below, or right-click an outbound packet in the log.",
                )
                .into_any_element()
        } else {
            self.queued_list(theme, settings.packets, cx)
        };
        v_flex().flex_1().min_h_0().child(bar).child(
            v_flex()
                .id("queue-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(12.0))
                .gap(px(10.0))
                .child(list)
                .child(self.add_row(theme, cx)),
        )
    }

    fn queued_list(
        &self,
        theme: &Theme,
        packets: Vec<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let packets: Rc<[String]> = packets.into();
        let (queued, editing) = (self.queued, self.editing);
        let quiet = theme.text_tertiary;
        let accent = theme.accent_text;
        let this = cx.weak_entity();
        let rows = {
            let this = this.clone();
            let packets = packets.clone();
            move |index: usize| {
                let item = ListItem::new(packets[index].clone())
                    .start_slot(
                        div()
                            .min_w(px(14.0))
                            .text_small()
                            .text_color(quiet)
                            .child((index + 1).to_string()),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(SYSTEM_MONO)
                            .text_size(TEXT_CAPTION)
                            .child(packets[index].clone()),
                    );
                if editing == Some(index) {
                    item.end_slot(div().text_color(accent).child("Editing"))
                } else if queued == Some(index) {
                    item.end_slot(row_actions(&this, index))
                } else {
                    item
                }
            }
        };
        let picks = this.clone();
        let opens = this.clone();
        let moves = this.clone();
        let removes = this;
        div()
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "backspace" | "delete") {
                    let _ = removes.update(cx, |this, cx| {
                        if let Some(index) = this.queued {
                            this.remove_queued(index, cx);
                        }
                    });
                }
            })
            .child(
                List::new("queued-packets", "Queued packets", packets.len(), rows)
                    .selected(queued)
                    .on_select(move |index, _, cx| {
                        let _ = picks.update(cx, |this, cx| {
                            this.queued = Some(index);
                            cx.notify();
                        });
                    })
                    .on_activate(move |index, window, cx| {
                        let _ = opens.update(cx, |this, cx| this.edit_queued(index, window, cx));
                    })
                    .on_reorder(move |from, to, _, cx| {
                        let _ = moves.update(cx, |this, cx| this.move_queued(from, to, cx));
                    }),
            )
            .into_any_element()
    }

    fn add_row(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let empty = Self::typed(&self.adding, cx).is_empty();
        let input = div()
            .flex_1()
            .min_w_0()
            .child(self.adding.clone())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.editing.is_some() {
                    this.cancel_edit(cx);
                }
            }));
        let buttons = if self.editing.is_some() {
            h_flex()
                .gap(px(8.0))
                .child(
                    Button::new("queue-save", "Save")
                        .variant(Variant::Primary)
                        .disabled(empty)
                        .on_click(cx.listener(|this, _, _, cx| this.add_typed(cx))),
                )
                .child(
                    Button::new("queue-cancel", "Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                )
        } else {
            h_flex().child(
                Button::new("queue-add", "Add")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _, _, cx| this.add_typed(cx))),
            )
        };
        v_flex()
            .flex_none()
            .gap(px(6.0))
            .child(h_flex().gap(px(8.0)).child(input).child(buttons))
            .child(note_line(
                theme,
                self.add_problem
                    .map(|problem| (problem.message().into(), theme.danger_text)),
            ))
    }
}

fn row_actions(this: &WeakEntity<Packets>, index: usize) -> Div {
    let edits = this.clone();
    let removes = this.clone();
    h_flex()
        .my(px(-1.0))
        .gap(px(2.0))
        .child(
            Button::new(("edit-queued", index), "Edit")
                .size(Size::XSmall)
                .variant(Variant::Ghost)
                .on_click(move |_, window, cx| {
                    let _ = edits.update(cx, |this, cx| this.edit_queued(index, window, cx));
                }),
        )
        .child(
            Button::new(("remove-queued", index), "Remove")
                .size(Size::XSmall)
                .variant(Variant::Ghost)
                .on_click(move |_, _, cx| {
                    let _ = removes.update(cx, |this, cx| this.remove_queued(index, cx));
                }),
        )
}

fn note_line(theme: &Theme, status: Option<(SharedString, Hsla)>) -> Div {
    h_flex()
        .gap(px(12.0))
        .text_small()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.text_tertiary)
                .child(PLACEHOLDERS),
        )
        .when_some(status, |line, (text, color)| {
            line.child(
                div()
                    .id("packet-status")
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .flex_none()
                    .max_w(relative(0.6))
                    .truncate()
                    .text_color(color)
                    .child(text),
            )
        })
}

fn tone_text(theme: &Theme, tone: Tone) -> Hsla {
    theme.tone(tone).2
}

fn moved_index(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

fn replay(packet: &Packet) -> (Target, String) {
    match packet.from {
        Sender::Client => (Target::Server, packet::with_room_placeholder(&packet.text)),
        Sender::Server => (Target::Client, packet.text.to_string()),
    }
}

fn target_label(target: Target) -> &'static str {
    match target {
        Target::Server => "Server",
        Target::Client => "Client",
    }
}

fn target_word(target: Target) -> &'static str {
    match target {
        Target::Server => "server",
        Target::Client => "client",
    }
}

fn parse_delay(text: &str) -> Option<Duration> {
    let ms: u64 = text.trim().parse().ok()?;
    Some(Duration::from_millis(
        ms.clamp(spammer::DELAY_MS.0, spammer::DELAY_MS.1),
    ))
}

fn queue_status(
    settings: &spammer::Settings,
    status: Status,
    theme: &Theme,
) -> (SharedString, Hsla) {
    let (text, tone): (SharedString, Tone) = match status {
        Status::Failed(reason) => (format!("Stopped: {reason}").into(), Tone::Danger),
        _ if !settings.enabled => {
            return (
                "Sends each packet in order, then starts over.".into(),
                theme.text_tertiary,
            );
        }
        Status::Sent { index, count } => (
            format!("Sent {} of {count}", index + 1).into(),
            Tone::Accent,
        ),
        Status::NotInGame => ("Not in game".into(), Tone::Warning),
        Status::Idle if settings.packets.is_empty() => ("Nothing to send".into(), Tone::Warning),
        Status::Idle => ("Starting".into(), Tone::Neutral),
    };
    (text, tone_text(theme, tone))
}

fn clock(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%H:%M:%S%.3f")
        .to_string()
}

fn tag(from: Sender) -> &'static str {
    match from {
        Sender::Client => "out",
        Sender::Server => "in",
    }
}

fn describe(line: &Line) -> (SystemTime, &'static str, SharedString) {
    match line {
        Line::Packet(packet) => (packet.at, tag(packet.from), packet.text.clone()),
        Line::Lost { at } => (*at, "lost", LOST.into()),
    }
}

const LOST: &str = "Packets were dropped here because the log fell behind";

fn row(line: &Line, times: bool) -> Row {
    let (at, tag, tone, text) = match line {
        Line::Packet(packet) => (
            packet.at,
            tag(packet.from),
            match packet.from {
                Sender::Client => Tone::Accent,
                Sender::Server => Tone::Neutral,
            },
            packet.preview.clone(),
        ),
        Line::Lost { at } => (*at, "lost", Tone::Warning, LOST.into()),
    };
    Row {
        time: times.then(|| clock(at).into()),
        tag,
        tone,
        text,
        actions: matches!(line, Line::Packet(_)),
    }
}

impl Render for Packets {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = shown_tab::header(
            "Packets",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        );
        let automation = self.automation().cloned();
        let queued = automation
            .as_ref()
            .map_or(0, |automation| automation.spammer().packets.len());
        let labels = [("Log", 0), ("Send", queued)];
        let body = match (&automation, self.page) {
            (None, _) => div()
                .py(px(24.0))
                .text_center()
                .text_small()
                .text_color(theme.text_tertiary)
                .child("No tabs open"),
            (Some(automation), Page::Log) => self.log_page(&theme, automation, window, cx),
            (Some(automation), Page::Send) => self.send_page(&theme, automation, cx),
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
            .child(header)
            .child(
                div().flex_none().px(px(16.0)).child(
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
                                this.page = [Page::Log, Page::Send][index];
                                cx.notify();
                            });
                        }
                    }),
                ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn open(cx: &mut TestAppContext) -> (Entity<Packets>, Automation, &mut VisualTestContext) {
        let folder = std::env::temp_dir().join(format!("mira-packets-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(mira_ui::init);
        let automation = Automation::default();
        automation.update_spammer(|settings| {
            settings.packets = vec!["%xt%zm%a%{room}%".into(), "%xt%zm%b%{room}%".into()];
        });
        let shown = automation.clone();
        let (packets, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let client = Client::empty_for_tests(&folder, cx);
            let mut packets = Packets::new(client, shown_tab::Target::Selected, window, cx);
            packets.shown = Some(ShownTab::fake("Alt 1", shown));
            packets.page = Page::Send;
            packets.revert_delay(cx);
            packets
        });
        cx.run_until_parked();
        (packets, automation, cx)
    }

    fn press(cx: &mut VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    fn typed(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn text(input: &Entity<Input>, cx: &mut VisualTestContext) -> String {
        input.read_with(cx, |input, _| input.text().to_owned())
    }

    #[gpui::test]
    fn the_delay_applies_on_return_or_leaving_the_field_and_escape_reverts(
        cx: &mut TestAppContext,
    ) {
        let (packets, automation, cx) = open(cx);
        let (delay, focus) = packets.read_with(cx, |packets, _| {
            (packets.delay.clone(), packets.focus.clone())
        });
        cx.update(|window, cx| window.focus(&delay.focus_handle(cx), cx));
        press(cx, "cmd-a");
        typed(cx, "250");
        assert_eq!(automation.spammer().delay, Duration::from_secs(1));
        press(cx, "enter");
        assert_eq!(automation.spammer().delay, Duration::from_millis(250));

        press(cx, "cmd-a");
        typed(cx, "5");
        press(cx, "escape");
        assert_eq!(text(&delay, cx), "250");

        press(cx, "cmd-a");
        typed(cx, "5");
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.run_until_parked();
        assert_eq!(automation.spammer().delay, Duration::from_millis(10));
        assert_eq!(text(&delay, cx), "10");
    }

    #[gpui::test]
    fn editing_a_queued_packet_replaces_it_in_place_and_escape_cancels(cx: &mut TestAppContext) {
        let (packets, automation, cx) = open(cx);
        let adding = packets.read_with(cx, |packets, _| packets.adding.clone());
        cx.update(|window, cx| {
            packets.update(cx, |packets, cx| packets.edit_queued(1, window, cx))
        });
        press(cx, "cmd-a");
        typed(cx, "%xt%zm%c%{room}%");
        press(cx, "enter");
        assert_eq!(
            automation.spammer().packets,
            ["%xt%zm%a%{room}%", "%xt%zm%c%{room}%"]
        );

        cx.update(|window, cx| {
            packets.update(cx, |packets, cx| packets.edit_queued(0, window, cx))
        });
        typed(cx, "x");
        press(cx, "escape");
        assert_eq!(text(&adding, cx), "");
        press(cx, "enter");
        assert_eq!(
            automation.spammer().packets,
            ["%xt%zm%a%{room}%", "%xt%zm%c%{room}%"]
        );
    }

    #[gpui::test]
    fn backspace_removes_the_selected_queued_packet(cx: &mut TestAppContext) {
        let (_, automation, cx) = open(cx);
        let row = cx.debug_bounds("row-0").expect("the first packet is shown");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        press(cx, "backspace");
        assert_eq!(automation.spammer().packets, ["%xt%zm%b%{room}%"]);
    }

    #[test]
    fn a_typed_delay_is_whole_milliseconds_held_in_range() {
        assert_eq!(parse_delay(" 250 "), Some(Duration::from_millis(250)));
        assert_eq!(parse_delay("9"), Some(Duration::from_millis(10)));
        assert_eq!(parse_delay("60001"), Some(Duration::from_millis(60_000)));
        assert_eq!(parse_delay(""), None);
    }

    #[test]
    fn moving_a_packet_keeps_the_edited_packet_under_edit() {
        let packets = ["a", "b", "c", "d"];
        for (from, to) in [(0, 3), (3, 0), (1, 2), (2, 1)] {
            let mut moved = packets.to_vec();
            let item = moved.remove(from);
            moved.insert(to, item);
            for (index, packet) in packets.iter().enumerate() {
                assert_eq!(moved[moved_index(index, from, to)], *packet);
            }
        }
    }
}
