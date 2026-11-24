mod backends;
pub(crate) mod context_menu;
pub(crate) mod extras;
mod net;
mod pacing;
pub(crate) mod stats;
mod surface;
pub(crate) mod view;

use std::{
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use ruffle_core::{
    FloatDuration, Player, PlayerBuilder, StageScaleMode, backend::ui::MouseCursor,
    config::Letterbox, tag_utils::SwfMovie,
};

use crate::{
    army::Armies,
    automation::{
        anti_afk, anti_counter, auto_attack, auto_relogin, auto_zone, boosts, drops, game_settings,
        packets::{self as packet_capture, Captured},
        quests, register_items,
        settings::Automation,
        sign_in, spammer,
    },
    bridge::{Bridge, CallError, Event, Packets},
    domain::{
        CombatCooldown,
        auth::{Credentials, LOGIN_GATE, Logins},
        bank,
        counter_attack::{self, CounterAttacks},
        grabber, packets, player,
        profiles::Library,
        retreat, rotation,
        world::{self, Location},
    },
    scripting::{self, Calls, ToPlayer},
};
use backends::{Navigator, Storage, Tasks, Ui, spawn, spawn_local};
use context_menu::{ContextMenu, MenuSelection, OpenMenu, PlayerInput};
pub(crate) use surface::Surface;
use view::{Effect, ViewEvent};

pub(crate) enum Msg {
    View(ViewEvent),
    MenuClosed(Option<MenuSelection>),
    ReleaseKeys,
    Frame,
    Poll(async_task::Runnable),
    Script(ToPlayer),
    Command(Command),
    SignIn {
        credentials: Credentials,
        server: Option<String>,
    },
    Wake,
    Stop,
}

pub(crate) enum Output {
    Menu(ContextMenu),
    AutoRelogin(auto_relogin::Report),
    FrameClock(bool),
    Cursor(MouseCursor),
    Loading(u8),
    Loaded,
    Location(Option<Location>),
    Identity(Option<player::Identity>),
    SignIn(sign_in::Status),
    Stage(Option<view::StageFit>),
    Beep,
    Packets(Vec<Captured>),
    Link(url::Url),
}

#[derive(Clone)]
pub(crate) enum Command {
    Jump {
        cell: String,
        pad: String,
    },
    SendPacket {
        target: packets::Target,
        packet: String,
        sent: async_channel::Sender<Result<packets::Delivery, CallError>>,
    },
    OpenBank,
    ReloadMap,
    SetSpawnPoint,
    FetchBoosts(async_channel::Sender<Result<Vec<boosts::Owned>, CallError>>),
    TakeBoosts {
        picked: Vec<boosts::Owned>,
        missed: async_channel::Sender<Result<Vec<String>, CallError>>,
    },
    QuestPickups {
        after: quests::State,
        waiting: async_channel::Sender<Result<Vec<drops::Waiting>, CallError>>,
    },
    Grabber(grabber::Job),
    Retreat(async_channel::Sender<Result<bool, CallError>>),
}

#[derive(Clone)]
pub(crate) struct Inbox {
    messages: mpsc::Sender<Msg>,
    shown_at: Arc<Mutex<Option<Instant>>>,
}

impl Inbox {
    pub(crate) fn post(&self, msg: Msg) {
        let _ = self.messages.send(msg);
    }

    pub(crate) fn request_frame(&self, shown_at: Instant) {
        if self.shown_at.lock().unwrap().replace(shown_at).is_none() {
            self.post(Msg::Frame);
        }
    }
}

pub(crate) struct Runtime {
    inbox: Inbox,
    thread: Option<thread::JoinHandle<()>>,
}

impl Runtime {
    pub(crate) fn spawn(
        surface: Surface,
        preferences: crate::settings::Player,
        storage: PathBuf,
        device: reqwest::header::HeaderValue,
        failures: async_channel::Sender<String>,
        outputs: async_channel::Sender<Output>,
        stats: tokio::sync::watch::Sender<Option<stats::Report>>,
        automation: Automation,
        extras: tokio::sync::watch::Receiver<extras::Extras>,
        profiles: tokio::sync::watch::Receiver<Arc<Library>>,
        armies: Armies,
        display_frames: bool,
        span: tracing::Span,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let inbox = Inbox {
            messages: tx,
            shown_at: Arc::default(),
        };
        let outbox = inbox.clone();
        let thread = thread::Builder::new()
            .name("mira-player".into())
            .spawn(move || {
                let _session = span.entered();
                if let Err(error) = run(
                    surface,
                    preferences,
                    storage,
                    device,
                    rx,
                    outbox,
                    outputs,
                    stats,
                    automation,
                    extras,
                    profiles,
                    armies,
                    display_frames,
                ) {
                    tracing::error!(%error, "player failed");
                    let _ = failures.try_send(error.to_string());
                }
            })?;
        Ok(Self {
            inbox,
            thread: Some(thread),
        })
    }

    pub(crate) fn send(&self, event: ViewEvent) {
        self.inbox.post(Msg::View(event));
    }

    pub(crate) fn inbox(&self) -> Inbox {
        self.inbox.clone()
    }

    pub(crate) fn close_menu(&self, selection: Option<MenuSelection>) {
        self.inbox.post(Msg::MenuClosed(selection));
    }

    pub(crate) fn release_keys(&self) {
        self.inbox.post(Msg::ReleaseKeys);
    }

    pub(crate) fn command(&self, command: Command) {
        self.inbox.post(Msg::Command(command));
    }

    pub(crate) fn sign_in(&self, credentials: Credentials, server: Option<String>) {
        self.inbox.post(Msg::SignIn {
            credentials,
            server,
        });
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.inbox.post(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    surface: Surface,
    preferences: crate::settings::Player,
    storage: PathBuf,
    device: reqwest::header::HeaderValue,
    rx: mpsc::Receiver<Msg>,
    inbox: Inbox,
    outputs: async_channel::Sender<Output>,
    stats: tokio::sync::watch::Sender<Option<stats::Report>>,
    automation: Automation,
    extras: tokio::sync::watch::Receiver<extras::Extras>,
    profiles: tokio::sync::watch::Receiver<Arc<Library>>,
    armies: Armies,
    display_frames: bool,
) -> anyhow::Result<()> {
    let mut pacing = pacing::Pacing::new(display_frames, Instant::now());
    let mut early = Vec::new();
    let mut viewport = loop {
        match rx.recv()? {
            Msg::View(ViewEvent::Resized(viewport)) => break viewport,
            Msg::View(ViewEvent::Visible(visible)) => pacing.visible = visible,
            Msg::View(ViewEvent::Throttle(throttle)) => {
                pacing.set_throttle(throttle, Instant::now())
            }
            Msg::Stop => return Ok(()),
            message @ Msg::SignIn { .. } => early.push(message),
            _ => {}
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _timers = runtime.enter();
    let net = net::Net::new(runtime.handle().clone(), device)?;
    let tasks = Tasks::default();
    let movie = SwfMovie::from_data(
        include_bytes!("../../assets/loader.swf"),
        "file:///mira/loader.swf".into(),
        None,
        None,
    )?;
    let bridge = Bridge::default();
    let player = PlayerBuilder::new()
        .with_boxed_renderer(surface.renderer(viewport)?)
        .with_navigator(Navigator::new(
            net,
            inbox.clone(),
            tasks.clone(),
            outputs.clone(),
        ))
        .with_boxed_audio(Box::new(backends::audio::audio()))
        .with_external_interface(Box::new(bridge.clone()))
        .with_ui(Ui::new(arboard::Clipboard::new()?, outputs.clone()))
        .with_storage(Box::new(Storage::new(storage)))
        .with_viewport_dimensions(viewport.width(), viewport.height(), viewport.scale())
        .with_autoplay(true)
        .with_letterbox(Letterbox::On)
        .with_movie(movie)
        .build();
    // Ruffle clears to white until it reads the movie's background tag; a preset color replaces that tag.
    player
        .lock()
        .unwrap()
        .set_background_color(Some(ruffle_core::Color::BLACK));
    bridge.attach(&player);
    let phases = outputs.clone();
    let logins = Rc::new(Logins::gated(&LOGIN_GATE));
    let counter_attacks = CounterAttacks::default();
    let cooldown = CombatCooldown::default();
    spawn_local(
        &inbox,
        &tasks,
        cooldown
            .clone()
            .track(bridge.clone(), bridge.subscribe(CombatCooldown::wants)),
    );
    spawn_local(
        &inbox,
        &tasks,
        anti_counter::run(
            bridge.clone(),
            bridge.subscribe(counter_attack::wants),
            automation.watch_game_settings(),
            counter_attacks.clone(),
        ),
    );
    spawn_local(
        &inbox,
        &tasks,
        anti_afk::run(
            bridge.clone(),
            bridge.subscribe(anti_afk::wants),
            automation.watch_game_settings(),
        ),
    );
    spawn_local(
        &inbox,
        &tasks,
        auto_relogin::run(
            bridge.clone(),
            logins.clone(),
            bridge.subscribe(auto_relogin::wants),
            automation.watch_auto_relogin(),
            move |phase| {
                let _ = phases.try_send(Output::AutoRelogin(phase));
            },
        ),
    );
    let reported = outputs.clone();
    let identified = bridge.clone();
    let statuses = Rc::new(sign_in::Statuses::new(move |status| {
        if matches!(status, sign_in::Status::Online(_)) {
            match player::identity(&identified) {
                Ok(identity) => {
                    let _ = reported.try_send(Output::Identity(identity));
                }
                Err(error) => tracing::debug!(%error, "couldn't read the player's name and guild"),
            }
        }
        let _ = reported.try_send(Output::SignIn(status));
    }));
    let watched = bridge.clone();
    let watch = sign_in::watch(
        bridge.clone(),
        logins.clone(),
        bridge.subscribe(auto_relogin::wants),
        statuses.clone(),
    );
    spawn_local(&inbox, &tasks, async move {
        watched.ready().await;
        watch.await
    });
    spawn_local(
        &inbox,
        &tasks,
        auto_zone::run(
            bridge.clone(),
            Packets::subscribe(|wants| bridge.subscribe(wants)),
            automation.watch_game_settings(),
            auto_zone::random_point,
        ),
    );
    spawn_local(
        &inbox,
        &tasks,
        auto_attack::run(
            bridge.clone(),
            automation.watch_auto_attack(),
            counter_attacks.clone(),
            bridge.subscribe(rotation::wants),
            profiles.clone(),
        ),
    );
    let subscriber = bridge.clone();
    spawn_local(
        &inbox,
        &tasks,
        quests::run(
            bridge.clone(),
            move |wants| subscriber.subscribe(wants),
            automation.watch_quests(),
        ),
    );
    let subscriber = bridge.clone();
    let beeps = outputs.clone();
    spawn_local(
        &inbox,
        &tasks,
        drops::run(
            bridge.clone(),
            move |wants| subscriber.subscribe(wants),
            automation.watch_drops(),
            automation.watch_quests(),
            move || {
                let _ = beeps.try_send(Output::Beep);
            },
            {
                let automation = automation.clone();
                move |released: &[i32]| {
                    automation.update_drops(|state| {
                        state.held.retain(|id| !released.contains(id));
                    });
                }
            },
            {
                let automation = automation.clone();
                move |names: &[String]| automation.report_quest_items(names.to_vec())
            },
        ),
    );
    let subscriber = bridge.clone();
    spawn_local(
        &inbox,
        &tasks,
        boosts::run(
            bridge.clone(),
            move |wants| subscriber.subscribe(wants),
            automation.watch_boosts(),
        ),
    );
    let located = outputs.clone();
    let identified = bridge.clone();
    spawn_local(
        &inbox,
        &tasks,
        world::track_location(
            bridge.clone(),
            bridge.subscribe(world::moves),
            move |location| {
                let identity = match location {
                    Some(_) => player::identity(&identified).unwrap_or_else(|error| {
                        tracing::debug!(%error, "couldn't read the player's name and guild");
                        None
                    }),
                    None => None,
                };
                let _ = located.try_send(Output::Location(location));
                let _ = located.try_send(Output::Identity(identity));
            },
        ),
    );
    spawn_local(
        &inbox,
        &tasks,
        game_settings::run(
            bridge.clone(),
            automation.watch_game_settings(),
            automation.watch_frame_rate(),
        ),
    );
    spawn_local(
        &inbox,
        &tasks,
        extras::run(bridge.clone(), bridge.subscribe(extras::wants), extras),
    );
    let spammer_reports = automation.clone();
    let spammer_stops = automation.clone();
    spawn_local(
        &inbox,
        &tasks,
        spammer::run(
            bridge.clone(),
            automation.watch_spammer(),
            move |status| spammer_reports.report_spammer(status),
            move || {
                spammer_stops.update_spammer(|settings| settings.enabled = false);
            },
        ),
    );
    let capture_bridge = bridge.clone();
    let captured = outputs.clone();
    spawn_local(
        &inbox,
        &tasks,
        packet_capture::capture(
            move || capture_bridge.subscribe(packet_capture::wants),
            automation.watch_capture(),
            move |batch| {
                let _ = captured.try_send(Output::Packets(batch));
            },
        ),
    );
    let loading = bridge.subscribe(|event| matches!(event, Event::Progress { .. } | Event::Loaded));
    let loading_bridge = bridge.clone();
    let loading_outputs = outputs.clone();
    spawn_local(&inbox, &tasks, async move {
        loop {
            match loading.next().await {
                Ok(Event::Progress { percent }) => {
                    let _ = loading_outputs.try_send(Output::Loading(percent.clamp(0, 100) as u8));
                }
                Ok(_) => break,
                Err(_) if loading_bridge.loaded() => break,
                Err(_) => {}
            }
        }
        let _ = loading_outputs.try_send(Output::Loaded);
    });
    player
        .lock()
        .unwrap()
        .set_frame_interpolation(preferences.smooth_motion);
    let mut open_menu: Option<OpenMenu> = None;
    let mut scripts = Calls::default();
    let scripts_bridge = bridge.clone();
    let commands_bridge = bridge.clone();
    let mut input = PlayerInput::new(bridge);
    let mut stats = stats::Collector::new(stats);
    stats.set_enabled(&mut player.lock().unwrap(), preferences.performance_stats);
    let mut last_tick = Instant::now();
    let mut frame_clock_enabled = false;
    let mut stage = None;
    let mut frame_rate = f64::NAN;
    let mut rendering = automation.watch_game_settings();
    rendering.mark_changed();
    let mut wait = Duration::ZERO;
    let mut effects = Vec::new();
    for message in early {
        inbox.post(message);
    }
    let mut signing_in = None;
    'running: loop {
        let mut frame_shown_at = None;
        let deadline = Instant::now() + wait;
        let message = match rx.recv_timeout(wait) {
            Ok(msg) => Some(msg),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let mut message = message;
        loop {
            match message {
                Some(Msg::Stop) => break 'running,
                Some(Msg::Poll(task)) => {
                    task.run();
                }
                Some(Msg::Script(message)) => scripts.handle(message, |call, state| {
                    spawn(
                        &inbox,
                        scripting::answer(
                            scripts_bridge.clone(),
                            logins.clone(),
                            automation.clone(),
                            armies.clone(),
                            counter_attacks.clone(),
                            cooldown.clone(),
                            profiles.clone(),
                            state,
                            call,
                        ),
                    )
                }),
                Some(Msg::SignIn {
                    credentials,
                    server,
                }) => {
                    let bridge = commands_bridge.clone();
                    let attempt = sign_in::run(
                        bridge.clone(),
                        logins.clone(),
                        credentials,
                        server,
                        automation.auto_relogin(),
                        statuses.clone(),
                    );
                    drop(signing_in.replace(spawn(&inbox, async move {
                        bridge.ready().await;
                        attempt.await
                    })));
                }
                Some(Msg::Command(command)) => spawn_local(
                    &inbox,
                    &tasks,
                    run_command(
                        commands_bridge.clone(),
                        automation.clone(),
                        cooldown.clone(),
                        command,
                    ),
                ),
                Some(Msg::Frame) => frame_shown_at = inbox.shown_at.lock().unwrap().take(),
                Some(Msg::View(event))
                    if open_menu.is_some()
                        && !matches!(
                            event,
                            ViewEvent::Resized(_)
                                | ViewEvent::Focus(_)
                                | ViewEvent::Visible(_)
                                | ViewEvent::Throttle(_)
                        ) => {}
                Some(Msg::View(event)) => {
                    view::translate(event, viewport, &mut effects);
                    let mut player = player.lock().unwrap();
                    for effect in effects.drain(..) {
                        match effect {
                            Effect::Event(event) => {
                                if let Some(menu) = input.handle_event(&mut player, event) {
                                    let context_menu = ContextMenu {
                                        items: menu.items.clone(),
                                        at: menu.position(viewport.scale()),
                                        smooth_motion: player.frame_interpolation(),
                                        stats: stats.enabled(),
                                    };
                                    if outputs.try_send(Output::Menu(context_menu)).is_ok() {
                                        open_menu = Some(menu);
                                    } else {
                                        menu.close(&mut player, None);
                                    }
                                }
                            }
                            Effect::MouseInStage(inside) => player.set_mouse_in_stage(inside),
                            Effect::Resize(next) if next != viewport => {
                                player.set_viewport_dimensions(next.dimensions());
                                viewport = next;
                                pacing.resized = true;
                            }
                            Effect::Resize(_) => {}
                            Effect::Visible(visible) => pacing.visible = visible,
                            Effect::Throttle(throttle) => {
                                pacing.set_throttle(throttle, Instant::now())
                            }
                        }
                    }
                }
                Some(Msg::ReleaseKeys) => input.release_keys(&mut player.lock().unwrap()),
                Some(Msg::MenuClosed(selection)) => {
                    if let Some(menu) = open_menu.take() {
                        let mut player = player.lock().unwrap();
                        menu.close(&mut player, selection);
                        if matches!(selection, Some(MenuSelection::Stats)) {
                            let enabled = !stats.enabled();
                            stats.set_enabled(&mut player, enabled);
                        }
                    }
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                break;
            }
            message = rx.try_recv().ok();
            if message.is_none() {
                break;
            }
        }
        let now = Instant::now();
        let mut player = player.lock().unwrap();
        let clock = frame_shown_at.unwrap_or(now).max(last_tick);
        player.tick(FloatDuration::from_std(clock - last_tick));
        backends::audio::follow_game_volume(&mut player);
        last_tick = clock;
        if player.frame_rate() != frame_rate {
            frame_rate = player.frame_rate();
            automation.observe_frame_rate(frame_rate);
        }
        if rendering.has_changed().unwrap_or(false) {
            pacing.minimal = rendering.borrow_and_update().rendering.minimal();
        }
        let interpolating = player.interpolating();
        let frame_clock = pacing.frame_clock(interpolating);
        if frame_clock != frame_clock_enabled {
            let _ = outputs.try_send(Output::FrameClock(frame_clock));
            frame_clock_enabled = frame_clock;
        }
        if pacing.draw(
            now,
            player.needs_render(),
            interpolating,
            frame_shown_at.is_some(),
        ) {
            let fit = stage_fit(&mut player);
            if fit != stage && outputs.try_send(Output::Stage(fit)).is_ok() {
                stage = fit;
            }
            player.render();
        }
        stats.sample(&mut player, viewport.dimensions());
        wait = pacing.wait(Instant::now(), interpolating, player.time_til_next_frame());
    }
    drop(scripts);
    tasks.borrow_mut().clear();
    while let Ok(message) = rx.try_recv() {
        if let Msg::Poll(task) = message {
            task.run();
        }
    }
    Ok(())
}

async fn run_command(
    bridge: Bridge,
    automation: Automation,
    cooldown: CombatCooldown,
    command: Command,
) {
    bridge.ready().await;
    let done = match command {
        Command::FetchBoosts(found) => {
            let _ = found.try_send(boosts::fetch(&bridge).await);
            Ok(())
        }
        Command::QuestPickups { after, waiting } => {
            let before = automation.quests();
            let found = drops::newly_taken(&bridge, &automation.drops(), &before, &after).await;
            let _ = waiting.try_send(found);
            Ok(())
        }
        Command::TakeBoosts { picked, missed } => {
            let taken = boosts::take(&bridge, |wants| bridge.subscribe(wants), picked).await;
            let _ = missed.try_send(taken.map(|taken| {
                automation.update_boosts(|state| register_items(&mut state.items, taken.usable));
                taken.missed
            }));
            Ok(())
        }
        Command::Jump { cell, pad } => player::jump(&bridge, &cell, Some(&pad)).await.map(drop),
        Command::OpenBank => bank::open(&bridge).await.map(drop),
        Command::SendPacket {
            target,
            packet,
            sent,
        } => {
            let _ = sent.try_send(packets::send(&bridge, target, &packet));
            Ok(())
        }
        Command::ReloadMap => world::reload(&bridge).await.map(drop),
        Command::SetSpawnPoint => world::set_spawn_point(&bridge, None, None).await.map(drop),
        Command::Grabber(job) => {
            grabber::run(&bridge, |wants| bridge.subscribe(wants), &cooldown, job).await;
            Ok(())
        }
        Command::Retreat(moved) => {
            let subscribe = |wants| bridge.subscribe(wants);
            tokio::select! {
                biased;
                () = moved.closed() => {}
                result = retreat::retreat(&bridge, &subscribe, automation.scripts().rooms) => {
                    let _ = moved.try_send(result);
                }
            }
            Ok(())
        }
    };
    if let Err(error) = done {
        tracing::warn!(%error, "a game bar command failed");
    }
}

fn stage_fit(player: &mut Player) -> Option<view::StageFit> {
    player.mutate_with_update_context(|context| {
        let stage = context.stage;
        if stage.scale_mode() != StageScaleMode::ShowAll {
            return None;
        }
        let (width, height) = stage.movie_size();
        Some(view::StageFit {
            width: NonZeroU32::new(width)?,
            height: NonZeroU32::new(height)?,
            align: stage.align(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_frames_coalesce_while_input_still_arrives() {
        let (messages, received) = mpsc::channel();
        let inbox = Inbox {
            messages,
            shown_at: Arc::default(),
        };
        thread::scope(|scope| {
            for _ in 0..8 {
                let inbox = &inbox;
                scope.spawn(move || {
                    for _ in 0..1000 {
                        inbox.request_frame(Instant::now());
                    }
                });
            }
        });
        let latest = Instant::now();
        inbox.request_frame(latest);
        inbox.post(Msg::View(ViewEvent::Focus(false)));
        assert!(matches!(received.try_recv(), Ok(Msg::Frame)));
        assert_eq!(inbox.shown_at.lock().unwrap().take(), Some(latest));
        assert!(matches!(
            received.try_recv(),
            Ok(Msg::View(ViewEvent::Focus(false)))
        ));
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}
