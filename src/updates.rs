mod bundle;
mod release;
mod view;

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::Result;
use gpui::{App, AppContext as _, Global, SharedString};
use semver::Version;

use bundle::Staged;
use release::Update;
pub(crate) use view::{button, dot, footer};

use crate::packages::github::{API, GitHub};

const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
const SPIN: Duration = Duration::from_secs(1);
const COOLDOWN: Duration = Duration::from_secs(30);
static CURRENT: LazyLock<Version> =
    LazyLock::new(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version"));

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

pub(crate) fn init(cx: &mut App) {
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
                    Updates::check(false, cx);
                }
            });
            cx.background_executor().timer(EVERY).await;
        }
    })
    .detach();
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

    pub(crate) fn check(manual: bool, cx: &mut App) {
        let started = cx.background_executor().now();
        let offered = match cx.try_global::<Self>() {
            Some(&Self::Current(at)) if started.duration_since(at) < COOLDOWN => {
                cx.set_global(Self::Checking);
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(SPIN).await;
                    cx.update(|cx| {
                        if matches!(cx.try_global::<Self>(), Some(Self::Checking)) {
                            cx.set_global(Self::Current(at));
                        }
                    });
                })
                .detach();
                return;
            }
            Some(Self::Idle | Self::Current(_) | Self::Failed(_)) => false,
            Some(Self::Available { .. }) => true,
            _ => return,
        };
        let github = match github(cx) {
            Ok(github) => github,
            Err(error) => return cx.set_global(Self::Failed(message(&error))),
        };
        if !offered {
            cx.set_global(Self::Checking);
        }
        let task = cx.background_spawn(async move { block_on(release::latest(&github, &CURRENT)) });
        cx.spawn(async move |cx| {
            let result = task.await;
            let executor = cx.background_executor().clone();
            if let Some(left) = (started + SPIN).checked_duration_since(executor.now()) {
                executor.timer(left).await;
            }
            cx.update(|cx| {
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
                        Self::Idle
                    }
                })
            });
        })
        .detach();
    }

    pub(crate) fn download(cx: &mut App) {
        let Some(Self::Available { update, .. }) = cx.try_global::<Self>() else {
            return;
        };
        let update = update.clone();
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
                    cx.set_global(Self::Downloading {
                        update: update.clone(),
                        progress,
                    })
                });
            }
            let result = task.await;
            cx.update(|cx| {
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
            if let Err(error) = installing.relaunch() {
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
        cx.update(|cx| Updates::check(true, cx));
        assert!(cx.update(|cx| matches!(cx.global::<Updates>(), Updates::Checking)));
        cx.executor().advance_clock(SPIN);
        cx.run_until_parked();
        assert!(
            cx.update(
                |cx| matches!(cx.global::<Updates>(), &Updates::Current(at) if at == checked)
            )
        );
    }
}
