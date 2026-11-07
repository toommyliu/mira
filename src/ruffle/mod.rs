mod backends;
pub(crate) mod context_menu;
mod net;
mod pacing;
mod preferences;
pub(crate) mod stats;
mod surface;
pub(crate) mod view;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use ruffle_core::{
    FloatDuration, PlayerBuilder, backend::ui::MouseCursor, config::Letterbox, tag_utils::SwfMovie,
};
use ruffle_frontend_utils::backends::storage::DiskStorageBackend;

use crate::automation::{
    auth::auto_relogin, bridge::Bridge, combat::auto_attack, world::auto_zone,
};
use backends::{Navigator, Tasks, Ui, spawn_local};
use context_menu::{ContextMenu, MenuSelection, OpenMenu, PlayerInput};
pub(crate) use surface::Surface;
use view::{Effect, ViewEvent};

pub(crate) enum Msg {
    View(ViewEvent),
    MenuClosed(Option<MenuSelection>),
    ReleaseKeys,
    Frame,
    Poll(async_task::Runnable),
    Wake,
    Stop,
}

pub(crate) enum Output {
    Menu(ContextMenu),
    AutoRelogin(auto_relogin::Report),
    FrameClock(bool),
    Cursor(MouseCursor),
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
        relogin: tokio::sync::watch::Receiver<auto_relogin::Settings>,
        zone: tokio::sync::watch::Receiver<auto_zone::Settings>,
        attack: tokio::sync::watch::Receiver<auto_attack::Settings>,
        display_frames: bool,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let inbox = Inbox {
            messages: tx,
            frame_pending: Arc::new(AtomicBool::new(false)),
        };
        let outbox = inbox.clone();
        let thread = thread::Builder::new()
            .name("lyra-player".into())
            .spawn(move || {
                if let Err(error) = run(
                    surface,
                    data,
                    device,
                    rx,
                    outbox,
                    outputs,
                    stats,
                    relogin,
                    zone,
                    attack,
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
    relogin: tokio::sync::watch::Receiver<auto_relogin::Settings>,
    zone: tokio::sync::watch::Receiver<auto_zone::Settings>,
    attack: tokio::sync::watch::Receiver<auto_attack::Settings>,
    display_frames: bool,
) -> anyhow::Result<()> {
    let mut pacing = pacing::Pacing::new(display_frames, Instant::now());
    let mut viewport = loop {
        match rx.recv()? {
            Msg::View(ViewEvent::Resized(viewport)) => break viewport,
            Msg::View(ViewEvent::Visible(visible)) => pacing.visible = visible,
            Msg::Stop => return Ok(()),
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
        "file:///lyra/loader.swf".into(),
        None,
        None,
    )?;
    let bridge = Bridge::default();
    let player = PlayerBuilder::new()
        .with_boxed_renderer(surface.renderer(viewport)?)
        .with_navigator(Navigator::new(net, inbox.clone(), tasks.clone()))
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
    bridge.attach(&player);
    let phases = outputs.clone();
    spawn_local(
        &inbox,
        &tasks,
        auto_relogin::run(
            bridge.clone(),
            bridge.subscribe(auto_relogin::wants),
            relogin,
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
            bridge.subscribe(auto_zone::wants),
            zone,
            auto_zone::random_point,
        ),
    );
    spawn_local(&inbox, &tasks, auto_attack::run(bridge.clone(), attack));
    let smooth_motion = preferences::BoolPreference::new(data.join("smooth-motion"));
    match smooth_motion.load() {
        Ok(enabled) => player.lock().unwrap().set_frame_interpolation(enabled),
        Err(error) => tracing::warn!(%error, "couldn't load smooth motion preference"),
    }
    let mut open_menu: Option<OpenMenu> = None;
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
    let mut wait = Duration::ZERO;
    let mut effects = Vec::new();
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
                Some(Msg::Frame) => {
                    inbox.frame_pending.store(false, Ordering::Relaxed);
                    frame_due = true;
                }
                Some(Msg::View(event))
                    if open_menu.is_some()
                        && !matches!(
                            event,
                            ViewEvent::Resized(_) | ViewEvent::Focus(_) | ViewEvent::Visible(_)
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
                            }
                            Effect::Resize(_) => {}
                            Effect::Visible(visible) => pacing.visible = visible,
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
        let interpolating = player.interpolating();
        let frame_clock = pacing.frame_clock(interpolating);
        if frame_clock != frame_clock_enabled {
            let _ = outputs.try_send(Output::FrameClock(frame_clock));
            frame_clock_enabled = frame_clock;
        }
        if pacing.draw(now, player.needs_render(), interpolating, frame_due) {
            player.render();
        }
        stats.sample(&mut player, viewport.dimensions());
        wait = pacing.wait(Instant::now(), interpolating, player.time_til_next_frame());
    }
    tasks.borrow_mut().clear();
    while let Ok(message) = rx.try_recv() {
        if let Msg::Poll(task) = message {
            task.run();
        }
    }
    Ok(())
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
