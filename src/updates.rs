mod bundle;
mod release;
mod relocate;
mod view;

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::Result;
use gpui::{App, AppContext as _, AsyncApp, Global, SharedString, Task, actions};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSView};
use objc2_foundation::{NSString, ns_string};
use semver::Version;
use serde::{Deserialize, Serialize};

use bundle::Staged;
use release::Update;
pub(crate) use relocate::offer as offer_move;
pub(crate) use view::{button, dot, footer};

use crate::packages::github::{API, GitHub};

actions!(mira, [CheckForUpdates]);

pub(crate) const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
const SPIN: Duration = Duration::from_secs(1);
const COOLDOWN: Duration = Duration::from_secs(30);
static CURRENT: LazyLock<Version> =
    LazyLock::new(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version"));

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Channel {
    #[default]
    Stable,
    Nightly,
}

impl Channel {
    pub(crate) const ALL: [Self; 2] = [Self::Stable, Self::Nightly];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Nightly => "Nightly",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Stable => "Get new versions of Mira when they’re released.",
            Self::Nightly => "Get Mira changes early, up to a few times a day.",
        }
    }

    fn of(version: &Version) -> Option<Self> {
        if !version.build.is_empty() {
            return None;
        }
        if version.pre.is_empty() {
            return Some(Self::Stable);
        }
        let (date, run) = version
            .pre
            .as_str()
            .strip_prefix("nightly.")?
            .split_once('.')?;
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        (date.len() == 8 && digits(date) && digits(run)).then_some(Self::Nightly)
    }
}

#[derive(Clone)]
pub(crate) enum Updates {
    Unavailable(Option<SharedString>),
    Idle,
    Checking,
    Current(Instant),
    Failed(SharedString),
    Available {
        update: Update,
        error: Option<SharedString>,
    },
    Downloading {
        update: Update,
        progress: f32,
    },
    Ready {
        update: Update,
        staged: Staged,
        error: Option<SharedString>,
    },
    Restarting(Version),
}

impl Global for Updates {}

#[derive(Default)]
struct ChannelGeneration(u64);

impl Global for ChannelGeneration {}

#[derive(Default)]
struct InFlight(Option<(Vec<async_channel::Sender<()>>, bool)>);

impl Global for InFlight {}

impl InFlight {
    fn join(manual: bool, cx: &mut App) -> Option<Task<()>> {
        let (done, waiting) = async_channel::bounded::<()>(1);
        let (waiters, asked) = cx.default_global::<Self>().0.as_mut()?;
        waiters.push(done);
        *asked |= manual;
        Some(cx.background_spawn(async move {
            let _ = waiting.recv().await;
        }))
    }

    fn begin(manual: bool, cx: &mut App) {
        cx.default_global::<Self>().0 = Some((Vec::new(), manual));
    }

    fn finish(cx: &mut App) -> bool {
        cx.default_global::<Self>()
            .0
            .take()
            .is_some_and(|(_, manual)| manual)
    }
}

pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &CheckForUpdates, cx| {
        crate::packages::packages(cx)
            .update(cx, |packages, cx| packages.check_when_idle(false, cx));
        let generation = cx.default_global::<ChannelGeneration>().0;
        let checking = Updates::check(true, cx);
        cx.spawn(async move |cx| {
            checking.await;
            if cx.update(|cx| cx.global::<ChannelGeneration>().0 == generation) {
                announce(cx);
            }
        })
        .detach();
    });
    cx.set_global(match bundle::locate() {
        Ok(_) => Updates::Idle,
        Err(reason) => Updates::Unavailable(reason.map(Into::into)),
    });
    cx.on_app_quit(|cx| {
        if let Some(Updates::Ready { staged, .. }) = cx.try_global::<Updates>() {
            staged.discard();
        }
        async {}
    })
    .detach();
}

pub(crate) fn start(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.update(|cx| {
                if crate::settings::check_updates(cx) {
                    Updates::check(false, cx).detach();
                }
            });
            cx.background_executor().timer(EVERY).await;
        }
    })
    .detach();
}

fn announce(cx: &AsyncApp) {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let Some(state) = cx.update(|cx| cx.try_global::<Updates>().cloned()) else {
        return;
    };
    let installed = env!("CARGO_PKG_VERSION");
    let channel = cx.update(|cx| crate::settings::update_channel(cx));
    let (style, message, detail, buttons): (_, String, Option<String>, &[&str]) = match &state {
        Updates::Current(_) if Channel::of(&CURRENT) != Some(channel) => (
            NSAlertStyle::Informational,
            match channel {
                Channel::Stable => "Waiting for the next stable release".into(),
                Channel::Nightly => "Waiting for the next nightly build".into(),
            },
            Some(format!("You’re running Mira {installed}.")),
            &["OK"],
        ),
        Updates::Current(_) => (
            NSAlertStyle::Informational,
            "You’re up to date!".into(),
            Some(format!(
                "Mira {installed} is currently the newest version available."
            )),
            &["OK"],
        ),
        Updates::Available { update, .. } => (
            NSAlertStyle::Informational,
            format!("Mira {} is available.", update.version),
            Some(format!("You’re running Mira {installed}.")),
            &["Download", "Later"],
        ),
        Updates::Downloading { update, .. } => (
            NSAlertStyle::Informational,
            format!("Mira {} is already downloading.", update.version),
            None,
            &["OK"],
        ),
        Updates::Ready { update, .. } => (
            NSAlertStyle::Informational,
            format!("Mira {} is ready to install.", update.version),
            Some(
                "Mira will restart to install it. Any running scripts will be interrupted.".into(),
            ),
            &["Restart", "Later"],
        ),
        Updates::Failed(error) => (
            NSAlertStyle::Warning,
            "Could not check for updates.".into(),
            Some(error.to_string()),
            &["OK"],
        ),
        Updates::Unavailable(reason) => (
            NSAlertStyle::Informational,
            "Automatic updates are not available right now.".into(),
            Some(
                reason
                    .as_ref()
                    .map_or("This is a development build.".into(), ToString::to_string),
            ),
            &["OK"],
        ),
        Updates::Idle => (
            NSAlertStyle::Warning,
            "Could not check for updates.".into(),
            Some("Try again in a moment.".into()),
            &["OK"],
        ),
        Updates::Checking | Updates::Restarting(_) => return,
    };
    if !alert(main, style, &message, detail.as_deref(), buttons) {
        return;
    }
    cx.update(|cx| match state {
        Updates::Available { .. } => Updates::download(cx),
        Updates::Ready { .. } => Updates::restart(cx),
        _ => {}
    });
}

fn alert(
    main: MainThreadMarker,
    style: NSAlertStyle,
    message: &str,
    detail: Option<&str>,
    buttons: &[&str],
) -> bool {
    let alert = NSAlert::new(main);
    alert.setAlertStyle(style);
    // Any accessory view, even an empty one, keeps AppKit from centering alerts with short text.
    alert.setAccessoryView(Some(&NSView::new(main)));
    alert.setMessageText(&NSString::from_str(message));
    if let Some(detail) = detail {
        alert.setInformativeText(&NSString::from_str(detail));
    }
    for (index, title) in buttons.iter().enumerate() {
        let button = alert.addButtonWithTitle(&NSString::from_str(title));
        if index > 0 && index == buttons.len() - 1 {
            button.setKeyEquivalent(ns_string!("\u{1b}"));
        }
    }
    alert.runModal() == NSAlertFirstButtonReturn
}

fn github(cx: &App) -> Result<GitHub> {
    let token = crate::packages::packages(cx)
        .read(cx)
        .token()
        .map(str::to_owned);
    Ok(GitHub::at(API, token)?)
}

fn block_on<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(work)
}

fn message(error: &anyhow::Error) -> SharedString {
    let message = format!("{error:#}");
    tracing::warn!(error = message, "Mira update failed");
    message.into()
}

impl Updates {
    #[cfg(test)]
    pub(crate) fn available_preview(version: Version, error: Option<SharedString>) -> Self {
        Self::Available {
            update: Update::preview(version),
            error,
        }
    }

    #[cfg(test)]
    pub(crate) fn ready_preview(version: Version) -> Self {
        Self::Ready {
            update: Update::preview(version.clone()),
            staged: Staged::preview(version),
            error: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn downloading_preview(version: Version, progress: f32) -> Self {
        Self::Downloading {
            update: Update::preview(version),
            progress,
        }
    }

    pub(crate) fn update(&self) -> Option<&Update> {
        match self {
            Self::Available { update, .. }
            | Self::Downloading { update, .. }
            | Self::Ready { update, .. } => Some(update),
            _ => None,
        }
    }

    pub(crate) fn version(&self) -> Option<&Version> {
        match self {
            Self::Available { update, .. }
            | Self::Downloading { update, .. }
            | Self::Ready { update, .. } => Some(&update.version),
            Self::Restarting(version) => Some(version),
            _ => None,
        }
    }

    pub(crate) fn channel_changed(cx: &mut App) {
        match cx.try_global::<Self>() {
            Some(Self::Unavailable(_) | Self::Restarting(_)) | None => return,
            Some(Self::Ready { staged, .. }) => staged.discard(),
            _ => {}
        }
        cx.default_global::<ChannelGeneration>().0 += 1;
        InFlight::finish(cx);
        cx.set_global(Self::Idle);
        Self::check(false, cx).detach();
    }

    pub(crate) fn check(manual: bool, cx: &mut App) -> Task<()> {
        if let Some(running) = InFlight::join(manual, cx) {
            return running;
        }
        let started = cx.background_executor().now();
        let generation = cx.default_global::<ChannelGeneration>().0;
        let offered = match cx.try_global::<Self>() {
            Some(&Self::Current(at)) if started.duration_since(at) < COOLDOWN => {
                cx.set_global(Self::Checking);
                InFlight::begin(manual, cx);
                return cx.spawn(async move |cx| {
                    cx.background_executor().timer(SPIN).await;
                    cx.update(|cx| {
                        if cx.global::<ChannelGeneration>().0 != generation {
                            return;
                        }
                        InFlight::finish(cx);
                        if matches!(cx.try_global::<Self>(), Some(Self::Checking)) {
                            cx.set_global(Self::Current(at));
                        }
                    });
                });
            }
            Some(Self::Idle | Self::Current(_) | Self::Failed(_)) => false,
            Some(Self::Available { .. }) => true,
            _ => return Task::ready(()),
        };
        let github = match github(cx) {
            Ok(github) => github,
            Err(error) => {
                cx.set_global(Self::Failed(message(&error)));
                return Task::ready(());
            }
        };
        let previous = cx.try_global::<Self>().cloned();
        if !offered {
            cx.set_global(Self::Checking);
        }
        InFlight::begin(manual, cx);
        let channel = crate::settings::update_channel(cx);
        let task = cx
            .background_spawn(async move { block_on(release::latest(&github, &CURRENT, channel)) });
        cx.spawn(async move |cx| {
            let result = task.await;
            let executor = cx.background_executor().clone();
            if let Some(left) = (started + SPIN).checked_duration_since(executor.now()) {
                executor.timer(left).await;
            }
            cx.update(|cx| {
                if cx.global::<ChannelGeneration>().0 != generation {
                    return;
                }
                let manual = InFlight::finish(cx);
                if !matches!(
                    cx.try_global::<Self>(),
                    Some(Self::Checking | Self::Available { .. })
                ) {
                    return;
                }
                cx.set_global(match result {
                    Ok(Some(update)) => Self::Available {
                        update,
                        error: None,
                    },
                    Ok(None) => Self::Current(cx.background_executor().now()),
                    Err(error) if manual => Self::Failed(message(&error)),
                    Err(error) => {
                        tracing::info!(
                            error = format!("{error:#}"),
                            "couldn't check for a Mira update"
                        );
                        if offered {
                            return;
                        }
                        previous.unwrap_or(Self::Idle)
                    }
                })
            });
        })
    }

    pub(crate) fn download(cx: &mut App) {
        let Some(Self::Available { update, .. }) = cx.try_global::<Self>() else {
            return;
        };
        let update = update.clone();
        let generation = cx.default_global::<ChannelGeneration>().0;
        let github = github(cx);
        cx.set_global(Self::Downloading {
            update: update.clone(),
            progress: 0.0,
        });
        let (report, progress) = async_channel::unbounded();
        let fetching = update.clone();
        let task = cx.background_spawn(async move {
            let mut shown = 0.0;
            block_on(fetch(github?, fetching, move |fraction| {
                if fraction - shown >= 0.01 || fraction >= 1.0 {
                    shown = fraction;
                    let _ = report.try_send(fraction);
                }
            }))
        });
        cx.spawn(async move |cx| {
            while let Ok(progress) = progress.recv().await {
                cx.update(|cx| {
                    if cx.global::<ChannelGeneration>().0 != generation {
                        return;
                    }
                    cx.set_global(Self::Downloading {
                        update: update.clone(),
                        progress,
                    })
                });
            }
            let result = task.await;
            cx.update(|cx| {
                if cx.global::<ChannelGeneration>().0 != generation {
                    if let Ok(staged) = result {
                        staged.discard();
                    }
                    return;
                }
                cx.set_global(match result {
                    Ok(staged) => Self::Ready {
                        update,
                        staged,
                        error: None,
                    },
                    Err(error) => Self::Available {
                        update,
                        error: Some(message(&error)),
                    },
                })
            });
        })
        .detach();
    }

    pub(crate) fn restart(cx: &mut App) {
        let Some(Self::Ready { update, staged, .. }) = cx.try_global::<Self>() else {
            return;
        };
        let (update, staged) = (update.clone(), staged.clone());
        cx.set_global(Self::Restarting(staged.version.clone()));
        let installing = staged.clone();
        let task = cx.background_spawn(async move {
            installing.install()?;
            if let Err(error) = bundle::reopen(&installing.target) {
                tracing::warn!(%error, "couldn't reopen Mira after installing the update");
            }
            anyhow::Ok(())
        });
        cx.spawn(async move |cx| {
            let result = task.await;
            cx.update(|cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => cx.set_global(Self::Ready {
                    update,
                    staged,
                    error: Some(message(&error)),
                }),
            });
        })
        .detach();
    }
}

async fn fetch(github: GitHub, update: Update, progress: impl FnMut(f32)) -> Result<Staged> {
    let target = bundle::locate().map_err(|reason| {
        anyhow::anyhow!(reason.unwrap_or_else(|| "Mira isn’t packaged".into()))
    })?;
    let dir = Staged::workspace(&target)?;
    let zip = dir.join(release::ZIP);
    let staged = async {
        release::download(&github, &update, &zip, progress).await?;
        Staged::unpack(target, dir.clone(), &zip, update.version)
    }
    .await;
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    staged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn a_check_soon_after_one_spins_without_asking_github(cx: &mut gpui::TestAppContext) {
        let checked = cx.update(|cx| {
            let now = cx.background_executor().now();
            cx.set_global(Updates::Current(now));
            now
        });
        cx.executor().advance_clock(COOLDOWN / 2);
        cx.update(|cx| Updates::check(true, cx).detach());
        assert!(cx.update(|cx| matches!(cx.global::<Updates>(), Updates::Checking)));
        cx.executor().advance_clock(SPIN);
        cx.run_until_parked();
        assert!(
            cx.update(
                |cx| matches!(cx.global::<Updates>(), &Updates::Current(at) if at == checked)
            )
        );
    }

    #[gpui::test]
    fn a_second_check_waits_for_the_one_already_running(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_global(Updates::Current(cx.background_executor().now())));
        cx.update(|cx| Updates::check(true, cx).detach());
        let second = cx.update(|cx| Updates::check(true, cx));
        let answered = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = answered.clone();
        cx.update(|cx| {
            cx.spawn(async move |_| {
                second.await;
                flag.set(true);
            })
            .detach()
        });
        cx.run_until_parked();
        assert!(!answered.get());
        cx.executor().advance_clock(SPIN);
        cx.run_until_parked();
        assert!(answered.get());
        assert!(cx.update(|cx| matches!(cx.global::<Updates>(), Updates::Current(_))));
    }
}
