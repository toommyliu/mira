mod backends;
pub(crate) mod context_menu;
pub(crate) mod extras;
mod net;
mod pacing;
mod preferences;
pub(crate) mod stats;
mod surface;
pub(crate) mod view;

use std::{
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use ruffle_core::{
    FloatDuration, Player, PlayerBuilder, StageScaleMode, backend::ui::MouseCursor,
    config::Letterbox, tag_utils::SwfMovie,
};
use ruffle_frontend_utils::backends::storage::DiskStorageBackend;

use crate::{
    army::Armies,
    automation::{
        anti_afk, anti_counter, auto_attack, auto_relogin, auto_zone, boosts, drops, follower,
        game_settings,
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
        rotation,
        world::{self, Location},
    },
    scripting::{self, Calls, ToPlayer},
};
use backends::{Navigator, Tasks, Ui, spawn, spawn_local};
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
    WatchSignIn,
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
}

#[derive(Clone)]
pub(crate) struct Inbox {
    messages: mpsc::Sender<Msg>,
    frame_pending: Arc<AtomicBool>,
}

impl Inbox {
    pub(crate) fn post(&self, msg: Msg) {
        let _ = self.messages.send(msg);
    }

    pub(crate) fn request_frame(&self) {
        if !self.frame_pending.swap(true, Ordering::Relaxed) {
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
        data: PathBuf,
        device: reqwest::header::HeaderValue,
        failures: async_channel::Sender<String>,
        outputs: async_channel::Sender<Output>,
        stats: tokio::sync::watch::Sender<Option<stats::Report>>,
        automation: Automation,
        extras: tokio::sync::watch::Receiver<extras::Extras>,
        profiles: tokio::sync::watch::Receiver<Arc<Library>>,
        armies: Armies,
        display_frames: bool,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let inbox = Inbox {
            messages: tx,
            frame_pending: Arc::new(AtomicBool::new(false)),
        };
        let outbox = inbox.clone();
        let thread = thread::Builder::new()
            .name("mira-player".into())
            .spawn(move || {
                if let Err(error) = run(
                    surface,
                    data,
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

    pub(crate) fn watch_sign_in(&self) {
        self.inbox.post(Msg::WatchSignIn);
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
    data: PathBuf,
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
        .with_boxed_audio(backends::audio::audio())
        .with_external_interface(Box::new(bridge.clone()))
        .with_ui(Ui::new(arboard::Clipboard::new()?, outputs.clone()))
        .with_storage(Box::new(DiskStorageBackend::new(
            data.join("shared-objects"),
        )))
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
    let following = Automation::clone(&automation);
    spawn_local(
        &inbox,
        &tasks,
        follower::run(
            bridge.clone(),
            move |wants| subscriber.subscribe(wants),
            automation.watch_follower(),
            counter_attacks.clone(),
            profiles.clone(),
            move |status| following.report_follower(status),
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
    spawn_local(
        &inbox,
        &tasks,
        game_settings::log_hidden_ads(bridge.subscribe(game_settings::wants)),
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
    let smooth_motion = preferences::BoolPreference::new(data.join("smooth-motion"));
    match smooth_motion.load() {
        Ok(enabled) => player.lock().unwrap().set_frame_interpolation(enabled),
        Err(error) => tracing::warn!(%error, "couldn't load smooth motion preference"),
    }
    let mut open_menu: Option<OpenMenu> = None;
    let mut scripts = Calls::default();
    let scripts_bridge = bridge.clone();
    let commands_bridge = bridge.clone();
    let mut input = PlayerInput::new(bridge);
    let show_stats = preferences::BoolPreference::new(data.join("show-stats"));
    let stats_enabled = show_stats.load().unwrap_or_else(|error| {
        tracing::warn!(%error, "couldn't load stats preference");
        false
    });
    let mut stats = stats::Collector::new(stats);
    stats.set_enabled(&mut player.lock().unwrap(), stats_enabled);
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
        let mut frame_due = false;
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
                    let statuses = outputs.clone();
                    let bridge = commands_bridge.clone();
                    let connections = bridge.subscribe(auto_relogin::wants);
                    let logins = logins.clone();
                    let settings = automation.auto_relogin();
                    drop(signing_in.replace(spawn(&inbox, async move {
                        bridge.ready().await;
                        sign_in::run(
                            bridge,
                            logins,
                            connections,
                            credentials,
                            server,
                            settings,
                            move |status| {
                                let _ = statuses.try_send(Output::SignIn(status));
                            },
                        )
                        .await
                    })));
                }
                Some(Msg::WatchSignIn) => {
                    let statuses = outputs.clone();
                    let bridge = commands_bridge.clone();
                    let connections = bridge.subscribe(auto_relogin::wants);
                    let identified = bridge.clone();
                    drop(signing_in.replace(spawn(&inbox, async move {
                        bridge.ready().await;
                        sign_in::watch(bridge, connections, move |status| {
                            if matches!(status, sign_in::Status::Online(_)) {
                                match player::identity(&identified) {
                                    Ok(identity) => {
                                        let _ = statuses.try_send(Output::Identity(identity));
                                    }
                                    Err(error) => tracing::debug!(
                                        %error,
                                        "couldn't read the player's name and guild"
                                    ),
                                }
                            }
                            let _ = statuses.try_send(Output::SignIn(status));
                        })
                        .await
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
                Some(Msg::Frame) => {
                    inbox.frame_pending.store(false, Ordering::Relaxed);
                    frame_due = true;
                }
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
                            if let Err(error) = show_stats.save(enabled) {
                                tracing::error!(%error, "couldn't save stats preference");
                            }
                        }
                        if matches!(selection, Some(MenuSelection::SmoothMotion))
                            && let Err(error) = smooth_motion.save(player.frame_interpolation())
                        {
                            tracing::error!(%error, "couldn't save smooth motion preference");
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
        player.tick(FloatDuration::from_std(now - last_tick));
        last_tick = now;
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
        if pacing.draw(now, player.needs_render(), interpolating, frame_due) {
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
            frame_pending: Arc::new(AtomicBool::new(false)),
        };
        thread::scope(|scope| {
            for _ in 0..8 {
                let inbox = &inbox;
                scope.spawn(move || {
                    for _ in 0..1000 {
                        inbox.request_frame();
                    }
                });
            }
        });
        inbox.post(Msg::View(ViewEvent::Focus(false)));
        assert!(matches!(received.try_recv(), Ok(Msg::Frame)));
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
