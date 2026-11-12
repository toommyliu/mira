use std::{fs, io, path::PathBuf, sync::Arc};

use gpui::{App, Global};
use tokio::sync::watch;

use crate::domain::profiles::Library;

pub(crate) struct Store {
    library: watch::Sender<Arc<Library>>,
}

impl Global for Store {}

impl Store {
    fn open(path: PathBuf) -> Self {
        let (library, issues) = match fs::read(&path) {
            Ok(bytes) => Library::parse(&bytes).unwrap_or_else(|problem| {
                tracing::warn!(%problem, path = %path.display(), "couldn't read combat profiles");
                (Library::default(), Vec::new())
            }),
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, path = %path.display(), "couldn't read combat profiles");
                }
                (Library::default(), Vec::new())
            }
        };
        for issue in &issues {
            tracing::warn!(profile = %issue.name, problem = %issue.problem, "skipped a combat profile");
        }
        Self {
            library: watch::Sender::new(Arc::new(library)),
        }
    }
}

pub(crate) fn init(path: PathBuf, cx: &mut App) {
    cx.set_global(Store::open(path));
}

pub(crate) fn watch(cx: &App) -> watch::Receiver<Arc<Library>> {
    cx.global::<Store>().library.subscribe()
}
