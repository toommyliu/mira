mod bundle;
mod release;
mod view;

use std::{sync::LazyLock, time::Duration};

use anyhow::Result;
use gpui::{App, AppContext as _, Global, SharedString};
use semver::Version;

use bundle::Staged;
use release::Update;
pub(crate) use view::{footer, sidebar_row};

use crate::packages::github::{API, GitHub};

const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
static CURRENT: LazyLock<Version> =
    LazyLock::new(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version"));

pub(crate) enum Updates {
    Unavailable(Option<SharedString>),
    Idle,
    Checking,
    Current,
    Failed(SharedString),
    Available {
        update: Update,
        error: Option<SharedString>,
    },
    Downloading {
        version: Version,
        progress: f32,
    },
    Ready {
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
            staged: Staged::preview(version),
            error: None,
        }
    }

    pub(crate) fn version(&self) -> Option<&Version> {
        match self {
            Self::Available { update, .. } => Some(&update.version),
            Self::Ready { staged, .. } => Some(&staged.version),
            Self::Downloading { version, .. } | Self::Restarting(version) => Some(version),
            _ => None,
        }
    }

    pub(crate) fn check(manual: bool, cx: &mut App) {
        let offered = match cx.try_global::<Self>() {
            Some(Self::Idle | Self::Current | Self::Failed(_)) => false,
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
                    Ok(None) => Self::Current,
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
        let version = update.version.clone();
        cx.set_global(Self::Downloading {
            version: version.clone(),
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
                        version: version.clone(),
                        progress,
                    })
                });
            }
            let result = task.await;
            cx.update(|cx| {
                cx.set_global(match result {
                    Ok(staged) => Self::Ready {
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
        let Some(Self::Ready { staged, .. }) = cx.try_global::<Self>() else {
            return;
        };
        let staged = staged.clone();
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
