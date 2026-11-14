mod account;
mod account_card;
mod client;
mod environment;
mod follower;
mod form;
mod game_bar;
mod input;
mod launches;
mod loader_grabber;
mod manager;
mod menu;
mod packet_lines;
mod packet_log;
mod packets;
mod profile_form;
mod profiles;
mod script;
mod script_dialog;
mod script_form;
mod scripts;
mod shown_tab;
mod stats;
mod text_input;

#[path = "macos.rs"]
mod native;

use std::{ops::Range, rc::Rc};

use crate::{
    account_settings::{self, Options},
    accounts::AccountId,
    automation::{auto_attack, auto_relogin, game_settings, settings::Automation},
    ruffle::{
        Msg, Output, Runtime,
        context_menu::MenuSelection,
        extras,
        view::{StageFit, ViewEvent},
    },
};
use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, Entity, FocusHandle,
    Focusable, Hitbox, HitboxBehavior, InteractiveElement as _, IntoElement, KeyDownEvent,
    KeyUpEvent, ModifiersChangedEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, Role, ScrollEvent, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, canvas, div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, Theme,
    primitives::{Button, Spinner, button::Variant},
    theme::{TEXT_BODY, TEXT_SMALL, TEXT_TITLE},
};
use native::NativeView;
use ruffle_core::backend::ui::MouseCursor;

pub(crate) use account::{
    Changed, Details, Glance, Identified, Login, ScriptPhase, ScriptState, Summary, Updated,
};
pub(crate) use client::{Client, CloseSession, bar_title, bind_keys, drag_area, window_bar};
pub(crate) use environment::{OpenEnvironment, open as open_environment};
pub(crate) use follower::{OpenFollower, open as open_follower};
pub(crate) use game_bar::BAR_HEIGHT;
pub(crate) use loader_grabber::{OpenLoaderGrabber, open as open_loader_grabber};
pub(crate) use manager::{OpenAccounts, open as open_accounts};
pub(crate) use packets::{OpenPackets, open as open_packets};
pub(crate) use profiles::{
    OpenCombatProfiles, bind_keys as bind_profile_keys, open as open_combat_profiles,
};
pub(crate) use scripts::{OpenScripts, open as open_scripts};
pub(crate) use shown_tab::Target;

const UNFOCUSED_FRAME: std::time::Duration = std::time::Duration::from_millis(125);
const HIDDEN_TAB_FRAME: std::time::Duration = std::time::Duration::from_millis(500);

pub struct RuffleView {
    session: Runtime,
    error: Option<String>,
    loading: Loading,
    tiled: bool,
    selected: bool,
    stats: Option<crate::ruffle::stats::Report>,
    stats_placement: stats::Placement,
    player_bounds: Bounds<Pixels>,
    stage: Option<StageFit>,
    native: NativeView,
    focus: FocusHandle,
    hovered: bool,
    mouse: input::Mouse,
    cursor: MouseCursor,
    marked: Option<Range<usize>>,
    modifiers: gpui::Modifiers,
    automation: Automation,
    packets: Entity<packet_log::PacketLog>,
    settings_account: Option<AccountId>,
    extras: Rc<extras::Store>,
    auto_relogin: auto_relogin::Phase,
    servers: Vec<String>,
    bar: game_bar::BarState,
    bar_inset: Pixels,
    script: script::Script,
    account: Option<account::Link>,
    status: Option<crate::automation::sign_in::Status>,
    _subscriptions: [Subscription; 4],
}

impl RuffleView {
    pub fn new(
        data: std::path::PathBuf,
        device: reqwest::header::HeaderValue,
        armies: crate::army::Armies,
        extras: Rc<extras::Store>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<RuffleView>> {
        let mut native = NativeView::attach(window)?;
        let (failed, failures) = async_channel::bounded(1);
        let (outputs_tx, outputs) = async_channel::unbounded();
        let (stats_tx, mut stats) = tokio::sync::watch::channel(None);
        let automation = Automation::default();
        let session = Runtime::spawn(
            native.surface(),
            data,
            device,
            failed,
            outputs_tx,
            stats_tx,
            Automation::clone(&automation),
            extras.watch(),
            crate::combat_profiles::watch(cx),
            armies,
            native.supports_display_frames(),
        )?;
        let inbox = session.inbox();
        native.connect_frames(move || inbox.request_frame());
        let inbox = session.inbox();
        native
            .connect_visibility(move |visible| inbox.post(Msg::View(ViewEvent::Visible(visible))));
        let relogin_changes = automation.watch_auto_relogin();
        let game_changes = automation.watch_game_settings();
        let frame_rate_changes = automation.watch_frame_rate();
        let quest_changes = automation.watch_quests();
        let drop_changes = automation.watch_drops();
        let boost_changes = automation.watch_boosts();
        Ok(cx.new(|cx| {
            notify_on_change(quest_changes, cx);
            notify_on_change(drop_changes, cx);
            notify_on_change(boost_changes, cx);
            notify_on_change(relogin_changes, cx);
            notify_on_change(game_changes, cx);
            notify_on_change(frame_rate_changes, cx);
            cx.spawn(async move |this, cx| {
                while stats.changed().await.is_ok() {
                    let report = *stats.borrow_and_update();
                    if this
                        .update(cx, |this: &mut Self, cx| {
                            this.stats = report;
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn_in(window, async move |this, cx| {
                while let Ok(output) = outputs.recv().await {
                    let menu = match output {
                        Output::Menu(menu) => menu,
                        Output::Beep => {
                            objc2_app_kit::NSBeep();
                            continue;
                        }
                        Output::Packets(batch) => {
                            let Ok(log) = this.read_with(cx, |this: &Self, _| this.packets.clone())
                            else {
                                break;
                            };
                            log.update(cx, |log, cx| {
                                log.extend(batch);
                                cx.notify();
                            });
                            continue;
                        }
                        Output::Link(url) => {
                            if this
                                .update(cx, |_: &mut Self, cx| cx.open_url(url.as_str()))
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::FrameClock(enabled) => {
                            if this
                                .update(cx, |this: &mut Self, _| {
                                    this.native.set_frame_clock(enabled)
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::AutoRelogin(report) => {
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    match report {
                                        auto_relogin::Report::Phase(phase) => {
                                            this.auto_relogin = phase;
                                            cx.emit(Updated);
                                        }
                                        auto_relogin::Report::Servers(servers) => {
                                            this.servers = servers
                                        }
                                    }
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::Loading(_) | Output::Loaded => {
                            let loading = match output {
                                Output::Loading(percent) => Loading::Percent(percent),
                                _ => Loading::Done,
                            };
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    this.loading = loading;
                                    this.changed(cx);
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::Location(location) => {
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    this.bar.located(location);
                                    cx.emit(Updated);
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::Identity(identity) => {
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    let unlinked = this.account.is_none();
                                    if let Some(identity) = &identity {
                                        this.use_account_settings(
                                            AccountId::of(&identity.name),
                                            cx,
                                        );
                                        if unlinked {
                                            cx.emit(Identified(identity.name.clone()));
                                        }
                                    }
                                    this.bar.identified(identity);
                                    if unlinked {
                                        this.changed(cx);
                                    } else {
                                        cx.emit(Updated);
                                        cx.notify();
                                    }
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::SignIn(status) => {
                            if this
                                .update(cx, |this: &mut Self, cx| this.signed_in(status, cx))
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::Cursor(cursor) => {
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    this.cursor = cursor;
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        Output::Stage(stage) => {
                            if this
                                .update(cx, |this: &mut Self, cx| {
                                    this.stage = stage;
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    };
                    let Ok((view, input_view, extras)) = this.update(cx, |this: &mut Self, _| {
                        let (view, input_view) = this.native.menu_views();
                        (view, input_view, this.extras.get())
                    }) else {
                        break;
                    };
                    let selection = menu::show(view, menu, extras).await;
                    menu::sync_modifiers(&input_view);
                    if cx
                        .update(|window, cx| {
                            this.update(cx, |this, _| {
                                if let Some(MenuSelection::Extra(extra)) = selection
                                    && let Err(error) = this.extras.toggle(extra)
                                {
                                    tracing::error!(%error, "couldn't save AS3 extras");
                                }
                                this.session.close_menu(selection);
                                this.restore_modifiers(window);
                            })
                        })
                        .and_then(|updated| updated)
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn(async move |this, cx| {
                if let Ok(error) = failures.recv().await {
                    let _ = this.update(cx, |this: &mut Self, cx| {
                        this.error = Some(error);
                        this.changed(cx);
                    });
                }
            })
            .detach();
            let focus = cx.focus_handle();
            let subscriptions = [
                cx.observe_global::<crate::combat_profiles::Store>(|this: &mut Self, cx| {
                    this.follow_moved_profile(cx);
                    cx.notify();
                }),
                cx.on_focus(&focus, window, |this: &mut Self, window, _| {
                    this.session
                        .send(ViewEvent::Focus(window.is_window_active()));
                    let held = if window.is_window_active() {
                        menu::modifiers()
                    } else {
                        Default::default()
                    };
                    for event in input::modifier_keys(this.modifiers, held) {
                        this.session.send(event);
                    }
                    this.modifiers = held;
                }),
                cx.on_blur(&focus, window, |this: &mut Self, _, _| {
                    this.session.send(ViewEvent::Focus(false));
                    this.session.release_keys();
                    this.modifiers = Default::default();
                }),
                cx.observe_window_activation(window, |this: &mut Self, window, _| {
                    this.session.send(ViewEvent::Focus(
                        window.is_window_active() && this.focus.is_focused(window),
                    ));
                    this.throttle(window);
                    if !window.is_window_active() {
                        this.modifiers = Default::default();
                    }
                }),
            ];
            Self {
                session,
                error: None,
                loading: Loading::Started,
                tiled: false,
                selected: false,
                stats: None,
                stats_placement: stats::Placement::default(),
                player_bounds: Bounds::default(),
                stage: None,
                native,
                focus,
                hovered: false,
                mouse: input::Mouse::default(),
                cursor: MouseCursor::Arrow,
                marked: None,
                modifiers: Default::default(),
                automation,
                packets: cx.new(|_| packet_log::PacketLog::default()),
                settings_account: None,
                extras,
                auto_relogin: auto_relogin::Phase::Off,
                servers: Vec::new(),
                bar: game_bar::BarState::new(window, cx),
                bar_inset: Pixels::ZERO,
                script: script::Script::default(),
                account: None,
                status: None,
                _subscriptions: subscriptions,
            }
        }))
    }

    pub(super) fn bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        game_bar::bar(self, cx).into_any_element()
    }

    fn present(&mut self, tiled: bool, active: bool, window: &Window, cx: &mut Context<Self>) {
        self.native.set_shown(tiled || active);
        self.tiled = tiled;
        self.selected = active;
        self.throttle(window);
        cx.notify();
    }

    fn throttle(&self, window: &Window) {
        self.session
            .send(ViewEvent::Throttle(match (self.selected, self.tiled) {
                (true, _) if window.is_window_active() => None,
                (true, _) | (false, true) => Some(UNFOCUSED_FRAME),
                (false, false) => Some(HIDDEN_TAB_FRAME),
            }));
    }

    fn layout(&mut self, area: Bounds<Pixels>, window: &Window) -> Bounds<Pixels> {
        let scale = window.scale_factor();
        let bounds = input::stage_bounds(area, self.stage, scale);
        self.player_bounds = bounds;
        self.native.set_frame(bounds, scale);
        if let Some(viewport) = input::viewport(bounds, scale) {
            self.session.send(ViewEvent::Resized(viewport));
        }
        bounds
    }

    fn use_account_settings(&mut self, account: AccountId, cx: &mut Context<Self>) {
        if self.settings_account.as_ref() == Some(&account) {
            return;
        }
        account_settings::get(&account, cx)
            .options
            .apply_to(&self.automation);
        self.settings_account = Some(account);
        cx.notify();
    }

    fn save_options(&self, edit: impl FnOnce(&mut Options), cx: &mut Context<Self>) {
        if let Some(account) = &self.settings_account {
            account_settings::change(account, cx, |settings| edit(&mut settings.options));
        }
    }

    fn change_auto_relogin(
        &mut self,
        change: impl FnOnce(&mut auto_relogin::Settings),
        cx: &mut Context<Self>,
    ) {
        let relogin = self.automation.update_auto_relogin(change);
        self.save_options(|options| options.relogin = relogin, cx);
        cx.notify();
    }

    fn restore_modifiers(&mut self, window: &mut Window) {
        self.modifiers = if window.is_window_active() && self.focus.is_focused(window) {
            menu::modifiers()
        } else {
            Default::default()
        };
        for event in input::modifier_keys(Default::default(), self.modifiers) {
            self.session.send(event);
        }
        window.refresh();
    }

    fn change_auto_attack(
        &mut self,
        change: impl FnOnce(&mut auto_attack::Settings),
        cx: &mut Context<Self>,
    ) {
        let attack = self.automation.update_auto_attack(change);
        self.save_options(|options| options.auto_attack_profile = attack.profile, cx);
        cx.notify();
    }

    fn follow_moved_profile(&mut self, cx: &mut Context<Self>) {
        let store = cx.global::<crate::combat_profiles::Store>();
        if let Some(name) = self.automation.auto_attack().profile
            && let Some(to) = store.moved_to(&name)
        {
            self.automation
                .update_auto_attack(|settings| settings.profile = to);
        }
        if let Some(name) = self.automation.follower().profile
            && let Some(to) = store.moved_to(&name)
        {
            self.automation
                .update_follower(|settings| settings.profile = to);
        }
    }

    fn change_game_settings(
        &mut self,
        change: impl FnOnce(&mut game_settings::Settings),
        cx: &mut Context<Self>,
    ) {
        let game = self.automation.update_game_settings(change);
        self.save_options(|options| options.game = game, cx);
        cx.notify();
    }

    fn change_frame_rate(&mut self, fps: u32, cx: &mut Context<Self>) {
        if let Some(fps) = self.automation.set_frame_rate(fps) {
            self.save_options(|options| options.frame_rate = Some(fps), cx);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if let Some(key) = input::key(&event.keystroke) {
            self.session.send(ViewEvent::KeyDown(key));
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if let Some(key) = input::key(&event.keystroke) {
            self.session.send(ViewEvent::KeyUp(key));
        }
    }

    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        for event in input::modifier_keys(self.modifiers, event.modifiers) {
            self.session.send(event);
        }
        self.modifiers = event.modifiers;
    }
}

fn notify_on_change<T: Send + Sync + 'static>(
    mut changes: tokio::sync::watch::Receiver<T>,
    cx: &mut Context<RuffleView>,
) {
    cx.spawn(async move |this, cx| {
        while changes.changed().await.is_ok() {
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
    .detach();
}

fn paint(this: Entity<RuffleView>, hitbox: Hitbox, window: &mut Window, cx: &mut App) {
    let cursor = match this.read(cx).cursor {
        MouseCursor::Arrow => CursorStyle::Arrow,
        MouseCursor::Hand => CursorStyle::PointingHand,
        MouseCursor::IBeam => CursorStyle::IBeam,
        MouseCursor::Grab => CursorStyle::OpenHand,
    };
    window.set_cursor_style(cursor, &hitbox);
    window.handle_input(
        &this.read(cx).focus,
        text_input::TextInput::new(hitbox.bounds, this.clone()),
        cx,
    );
    let down_view = this.clone();
    let down_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && down_hitbox.is_hovered(window) {
            if event.button != gpui::MouseButton::Right {
                window.capture_pointer(down_hitbox.id);
            }
            down_view.update(cx, |this, cx| {
                window.focus(&this.focus, cx);
                let event = this.mouse.down(event, down_hitbox.origin);
                this.session.send(event);
            });
        }
    });
    let up_view = this.clone();
    let up_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            up_view.update(cx, |this, _| {
                let over_game = up_hitbox.is_hovered(window);
                if let Some(event) = this.mouse.up(event, up_hitbox.origin, over_game) {
                    this.session.send(event);
                }
            });
        }
    });
    let move_view = this.clone();
    let move_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let hovered = move_hitbox.is_hovered(window);
        move_view.update(cx, |this, _| {
            let captured = this.mouse.dragging(event.pressed_button);
            if hovered || captured {
                this.session.send(ViewEvent::MouseMove(input::point(
                    event.position,
                    move_hitbox.origin,
                )));
            } else if this.hovered {
                this.session.send(ViewEvent::MouseLeave);
            }
            this.hovered = hovered || captured;
        });
    });
    window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
            this.update(cx, |this, _| {
                this.session
                    .send(ViewEvent::Wheel(input::wheel(event.delta)))
            });
        }
    });
}

#[derive(Clone, Copy)]
enum Loading {
    Started,
    Percent(u8),
    Done,
}

fn loading(theme: &Theme, loading: Loading) -> impl IntoElement {
    let label: gpui::SharedString = match loading {
        Loading::Percent(percent) => format!("Loading {percent}%").into(),
        Loading::Started | Loading::Done => "Loading".into(),
    };
    div()
        .id("loading")
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.0))
        .bg(theme.base)
        .child(
            Spinner::new("loading-spinner")
                .label(label.clone())
                .size(px(24.0)),
        )
        .child(
            div()
                .text_size(TEXT_SMALL)
                .text_color(theme.text_secondary)
                .child(label),
        )
}

fn minimal_rendering(cx: &mut Context<RuffleView>) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .id("minimal-rendering")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .bg(theme.base)
        .text_size(TEXT_BODY)
        .text_color(theme.text)
        .child(
            div()
                .text_size(TEXT_TITLE)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Minimal rendering"),
        )
        .child(
            div()
                .text_color(theme.text_secondary)
                .child("The game and scripts keep running while using fewer resources."),
        )
        .child(
            Button::new("resume-rendering", "Resume rendering")
                .variant(Variant::Outline)
                .mt(px(8.0))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.change_game_settings(|settings| settings.rendering.resume(), cx)
                })),
        )
}

impl Render for RuffleView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let prepaint = {
            let this = this.clone();
            move |bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
                let player = this.update(cx, |this, _| this.layout(bounds, window));
                window.insert_hitbox(player, HitboxBehavior::Normal)
            }
        };
        let player = div()
            .id("game")
            .role(Role::Application)
            .aria_label("Game")
            .relative()
            .flex_1()
            .w_full()
            .when_some(self.error.clone(), |view, error| {
                let theme = cx.theme();
                view.bg(theme.base)
                    .text_color(theme.text)
                    .child(format!("Couldn't start AQW: {error}"))
            })
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .child(
                canvas(prepaint, move |_bounds, hitbox, window, cx| {
                    paint(this, hitbox, window, cx)
                })
                .size_full(),
            )
            .when(
                !matches!(self.loading, Loading::Done) && self.error.is_none(),
                |view| view.child(loading(cx.theme(), self.loading)),
            )
            .when(
                self.automation.game_settings().rendering.minimal(),
                |view| view.child(minimal_rendering(cx)),
            )
            .when_some(self.stats, |view, stats| {
                view.child(stats::overlay(
                    stats,
                    &self.stats_placement,
                    self.player_bounds,
                    cx.entity(),
                ))
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .when(!self.tiled, |root| root.child(self.bar(cx)))
            .child(player)
            .when_some(
                self.script.dialogs().filter(|_| self.selected),
                |root, dialogs| root.child(dialogs),
            )
    }
}

impl Focusable for RuffleView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
