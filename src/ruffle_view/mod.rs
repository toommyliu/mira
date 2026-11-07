mod game_bar;
mod input;
mod menu;
mod script;
mod stats;
mod text_input;
mod ui;

#[path = "macos.rs"]
mod native;

use std::ops::Range;

use crate::{
    automation::{auto_attack, auto_relogin, auto_zone, settings::Store},
    ruffle::{Msg, Output, Runtime, view::ViewEvent},
};
use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, Entity, FocusHandle,
    Focusable, Hitbox, HitboxBehavior, InteractiveElement as _, IntoElement, KeyDownEvent,
    KeyUpEvent, ModifiersChangedEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, ScrollWheelEvent, Styled as _, Subscription, Window,
    canvas, div, prelude::FluentBuilder as _,
};
use native::NativeView;
use ruffle_core::backend::ui::MouseCursor;

pub(crate) use ui::BAR_HEIGHT;

pub struct RuffleView {
    session: Runtime,
    error: Option<String>,
    stats: Option<crate::ruffle::stats::Report>,
    stats_placement: stats::Placement,
    player_bounds: Bounds<Pixels>,
    native: NativeView,
    focus: FocusHandle,
    hovered: bool,
    mouse: input::Mouse,
    cursor: MouseCursor,
    marked: Option<Range<usize>>,
    modifiers: gpui::Modifiers,
    automation: Store,
    auto_relogin: auto_relogin::Phase,
    servers: Vec<String>,
    bar: game_bar::BarState,
    script: script::Script,
    _subscriptions: [Subscription; 3],
}

impl RuffleView {
    pub fn new(
        data: std::path::PathBuf,
        device: reqwest::header::HeaderValue,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<RuffleView>> {
        let mut native = NativeView::attach(window)?;
        let (failed, failures) = async_channel::bounded(1);
        let (outputs_tx, outputs) = async_channel::unbounded();
        let (stats_tx, mut stats) = tokio::sync::watch::channel(None);
        let automation = Store::open(data.join("automation.json"));
        let session = Runtime::spawn(
            native.surface(),
            data,
            device,
            failed,
            outputs_tx,
            stats_tx,
            automation.watch_auto_relogin(),
            automation.watch_auto_zone(),
            automation.watch_auto_attack(),
            native.supports_display_frames(),
        )?;
        let inbox = session.inbox();
        native.connect_frames(move || inbox.request_frame());
        let inbox = session.inbox();
        native
            .connect_visibility(move |visible| inbox.post(Msg::View(ViewEvent::Visible(visible))));
        Ok(cx.new(|cx| {
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
                                            this.auto_relogin = phase
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
                    };
                    let Ok((view, input_view)) =
                        this.update(cx, |this: &mut Self, _| this.native.menu_views())
                    else {
                        break;
                    };
                    let selection = menu::show(view, menu).await;
                    menu::sync_modifiers(&input_view);
                    if this
                        .update_in(cx, |this, window, _| {
                            this.session.close_menu(selection);
                            this.restore_modifiers(window);
                        })
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
                        cx.notify();
                    });
                }
            })
            .detach();
            let focus = cx.focus_handle();
            let subscriptions = [
                cx.on_focus(&focus, window, |this: &mut Self, window, _| {
                    this.session
                        .send(ViewEvent::Focus(window.is_window_active()))
                }),
                cx.on_blur(&focus, window, |this: &mut Self, _, _| {
                    this.session.send(ViewEvent::Focus(false))
                }),
                cx.observe_window_activation(window, |this: &mut Self, window, _| {
                    this.session.send(ViewEvent::Focus(
                        window.is_window_active() && this.focus.is_focused(window),
                    ));
                    if !window.is_window_active() {
                        this.modifiers = Default::default();
                    }
                }),
            ];
            Self {
                session,
                error: None,
                stats: None,
                stats_placement: stats::Placement::default(),
                player_bounds: Bounds::default(),
                native,
                focus,
                hovered: false,
                mouse: input::Mouse::default(),
                cursor: MouseCursor::Arrow,
                marked: None,
                modifiers: Default::default(),
                automation,
                auto_relogin: auto_relogin::Phase::Off,
                servers: Vec::new(),
                bar: game_bar::BarState::new(cx),
                script: script::Script::Idle,
                _subscriptions: subscriptions,
            }
        }))
    }

    fn layout(&mut self, bounds: Bounds<Pixels>, window: &Window) {
        self.player_bounds = bounds;
        let scale = window.scale_factor();
        self.native.set_frame(bounds, scale);
        if let Some(viewport) = input::viewport(bounds, scale) {
            self.session.send(ViewEvent::Resized(viewport));
        }
    }

    fn change_auto_relogin(
        &mut self,
        change: impl FnOnce(&mut auto_relogin::Settings),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.automation.auto_relogin().clone();
        change(&mut settings);
        if let Err(error) = self.automation.set_auto_relogin(settings) {
            tracing::error!(%error, "couldn't save automation settings");
        }
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

    fn change_auto_zone(
        &mut self,
        change: impl FnOnce(&mut auto_zone::Settings),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.automation.auto_zone();
        change(&mut settings);
        if let Err(error) = self.automation.set_auto_zone(settings) {
            tracing::error!(%error, "couldn't save automation settings");
        }
        cx.notify();
    }

    fn change_auto_attack(
        &mut self,
        change: impl FnOnce(&mut auto_attack::Settings),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.automation.auto_attack();
        change(&mut settings);
        if let Err(error) = self.automation.set_auto_attack(settings) {
            tracing::error!(%error, "couldn't save automation settings");
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
    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
            this.update(cx, |this, _| {
                this.session
                    .send(ViewEvent::Wheel(input::wheel(event.delta)))
            });
        }
    });
}

impl Render for RuffleView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let prepaint = {
            let this = this.clone();
            move |bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
                this.update(cx, |this, _| this.layout(bounds, window));
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            }
        };
        let player = div()
            .relative()
            .flex_1()
            .w_full()
            .when_some(self.error.clone(), |view, error| {
                view.bg(gpui::rgb(0x181818))
                    .text_color(gpui::rgb(0xffffff))
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
            .child(game_bar::bar(self, cx))
            .child(player)
            .when_some(self.bar.open(), |root, menu| {
                root.child(game_bar::overlay(self, menu, cx))
            })
    }
}

impl Focusable for RuffleView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
