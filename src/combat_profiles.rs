use std::{fs, io, path::PathBuf, sync::Arc};

use gpui::{App, BorrowAppContext as _, Global};
use tokio::sync::watch;

use crate::{
    characters, data_file,
    domain::profiles::{Issue, Library, Profile, same_name},
};

pub(crate) struct Store {
    path: PathBuf,
    library: watch::Sender<Arc<Library>>,
    issues: Vec<Issue>,
    moved: Vec<(String, Option<String>)>,
}

impl Global for Store {}

impl Store {
    fn open(path: PathBuf) -> Self {
        let (library, issues) = match fs::read(&path) {
            Ok(bytes) => Library::parse(&bytes).unwrap_or_else(|problem| {
                data_file::set_aside(&path, &problem);
                (Library::default(), Vec::new())
            }),
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    data_file::set_aside(&path, &error);
                }
                (Library::default(), Vec::new())
            }
        };
        for issue in &issues {
            tracing::warn!(profile = %issue.name, problem = %issue.problem, "skipped a combat profile");
        }
        if !issues.is_empty() {
            match data_file::copy_aside(&path) {
                Ok(backup) => {
                    tracing::warn!(backup = %backup.display(), "kept a copy of the combat profiles that couldn't all be read");
                    if let Err(error) = data_file::write(&path, &library.to_json()) {
                        tracing::warn!(%error, path = %path.display(), "couldn't save combat profiles");
                    }
                }
                Err(error) => {
                    tracing::error!(%error, path = %path.display(), "couldn't keep a copy of the combat profiles");
                }
            }
        }
        Self {
            path,
            library: watch::Sender::new(Arc::new(library)),
            issues,
            moved: Vec::new(),
        }
    }

    fn write(&self, library: Library) -> io::Result<()> {
        data_file::write(&self.path, &library.to_json())?;
        self.library.send_replace(Arc::new(library));
        Ok(())
    }

    pub(crate) fn moved_to(&self, name: &str) -> Option<Option<String>> {
        let mut current = Some(name.to_owned());
        let mut moved = false;
        for (from, to) in &self.moved {
            if current
                .as_deref()
                .is_some_and(|current| same_name(current, from))
            {
                current.clone_from(to);
                moved = true;
            }
        }
        moved.then_some(current)
    }
}

pub(crate) fn init(path: PathBuf, cx: &mut App) {
    cx.set_global(Store::open(path));
}

pub(crate) fn watch(cx: &App) -> watch::Receiver<Arc<Library>> {
    cx.global::<Store>().library.subscribe()
}

pub(crate) fn library(cx: &App) -> Arc<Library> {
    cx.global::<Store>().library.borrow().clone()
}

pub(crate) fn issues(cx: &App) -> &[Issue] {
    &cx.global::<Store>().issues
}

pub(crate) fn save(
    previous: Option<&str>,
    name: String,
    profile: Profile,
    cx: &mut App,
) -> io::Result<()> {
    let mut library = Library::clone(&cx.global::<Store>().library.borrow());
    library.put(previous, name.clone(), profile);
    let renamed = previous
        .filter(|previous| !same_name(previous, &name))
        .map(|previous| (previous.to_owned(), Some(name)));
    commit(library, renamed, cx)
}

pub(crate) fn delete(name: &str, cx: &mut App) -> io::Result<()> {
    let mut library = Library::clone(&cx.global::<Store>().library.borrow());
    library.remove(name);
    commit(library, Some((name.to_owned(), None)), cx)
}

fn commit(
    library: Library,
    moved: Option<(String, Option<String>)>,
    cx: &mut App,
) -> io::Result<()> {
    cx.update_global::<Store, _>(|store, _| store.write(library))?;
    if let Some((from, to)) = moved {
        characters::move_auto_attack_profile(&from, to.as_deref(), cx);
        cx.global_mut::<Store>().moved.push((from, to));
        cx.defer(|cx| cx.global_mut::<Store>().moved.clear());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data_file, storyboard::scratch};

    #[test]
    fn saving_keeps_a_file_that_couldnt_be_read_in_full() {
        let folder = scratch("combat-profiles");
        let path = folder.join("combat-profiles.json");
        let generic = serde_json::to_value(Profile::generic()).unwrap();
        let partly = serde_json::json!({
            "version": 1,
            "profiles": { "Kept": generic, "Broken": 5 },
        })
        .to_string();
        for (damaged, readable) in [("{ not json", &["New"][..]), (&partly, &["Kept", "New"])] {
            fs::write(&path, damaged).unwrap();
            let store = Store::open(path.clone());
            assert!(data_file::backed_up(&folder, damaged));
            if let Ok(bytes) = fs::read(&path) {
                assert!(Library::parse(&bytes).unwrap().1.is_empty());
            }
            let mut library = Library::clone(&store.library.borrow());
            library.put(None, "New".into(), Profile::generic());
            store.write(library).unwrap();
            assert!(data_file::backed_up(&folder, damaged));
            let reopened = Store::open(path.clone());
            assert!(
                reopened
                    .library
                    .borrow()
                    .names()
                    .eq(readable.iter().copied())
            );
        }
        fs::remove_dir_all(folder).unwrap();
    }
}
