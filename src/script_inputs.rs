use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{App, Global};

use crate::scripting::{Supplied, Values};

pub(crate) struct Store {
    path: PathBuf,
    saved: Option<BTreeMap<PathBuf, Values>>,
}

impl Global for Store {}

impl Store {
    fn open(path: PathBuf) -> Self {
        let saved = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .inspect_err(|error| {
                    tracing::error!(%error, path = %path.display(), "couldn't read saved script inputs");
                })
                .ok(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(BTreeMap::new()),
            Err(error) => {
                tracing::error!(%error, path = %path.display(), "couldn't read saved script inputs");
                None
            }
        };
        Self { path, saved }
    }

    fn saved(&self, script: &Path) -> Supplied {
        match &self.saved {
            Some(saved) => {
                Supplied::Values(saved.get(&canonical(script)).cloned().unwrap_or_default())
            }
            None => Supplied::StoreUnreadable,
        }
    }

    fn save(&mut self, script: &Path, values: Values) -> io::Result<()> {
        let Some(saved) = &self.saved else {
            return Err(io::Error::other("saved script inputs couldn't be read"));
        };
        let mut next = saved.clone();
        next.insert(canonical(script), values);
        if let Some(folder) = self.path.parent() {
            fs::create_dir_all(folder)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        let mut bytes = serde_json::to_vec_pretty(&next).map_err(io::Error::other)?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)?;
        self.saved = Some(next);
        Ok(())
    }

    fn start_fresh(&mut self) -> io::Result<()> {
        if self.saved.is_some() {
            return Ok(());
        }
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let stem = self
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("script-inputs");
        let backup = (0..)
            .map(|attempt| {
                let suffix = if attempt == 0 {
                    String::new()
                } else {
                    format!("-{attempt}")
                };
                self.path
                    .with_file_name(format!("{stem}-{seconds}{suffix}.json.bad"))
            })
            .find(|backup| !backup.exists())
            .expect("some backup name is free");
        match fs::rename(&self.path, &backup) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        tracing::warn!(backup = %backup.display(), "moved unreadable script inputs aside");
        self.saved = Some(BTreeMap::new());
        Ok(())
    }
}

fn canonical(script: &Path) -> PathBuf {
    fs::canonicalize(script).unwrap_or_else(|_| script.to_owned())
}

pub(crate) fn init(path: PathBuf, cx: &mut App) {
    cx.set_global(Store::open(path));
}

pub(crate) fn saved(script: &Path, cx: &App) -> Supplied {
    cx.global::<Store>().saved(script)
}

pub(crate) fn values(script: &Path, cx: &App) -> anyhow::Result<Values> {
    match saved(script, cx) {
        Supplied::Values(values) => Ok(values),
        Supplied::StoreUnreadable => Err(anyhow::anyhow!(
            "couldn't queue {}: saved script inputs couldn't be read. Open Scripts to recover them.",
            script.file_name().map_or_else(
                || script.display().to_string(),
                |name| name.display().to_string()
            )
        )),
    }
}

pub(crate) fn unreadable(cx: &App) -> bool {
    cx.global::<Store>().saved.is_none()
}

pub(crate) fn save(script: &Path, values: Values, cx: &mut App) {
    if let Err(error) = cx.global_mut::<Store>().save(script, values) {
        tracing::error!(%error, "couldn't save script inputs");
    }
}

pub(crate) fn start_fresh(cx: &mut App) {
    if let Err(error) = cx.global_mut::<Store>().start_fresh() {
        tracing::error!(%error, "couldn't move unreadable script inputs aside");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn scratch() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder = std::env::temp_dir().join(format!(
            "mira-script-inputs-{:x}",
            u64::from_ne_bytes(random)
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn values(value: serde_json::Value) -> Values {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn saved_inputs_survive_reopening_and_stay_with_their_script() {
        let folder = scratch();
        let path = folder.join("script-inputs.json");
        let hunt = folder.join("hunt.ts");
        let daily = folder.join("daily.ts");
        fs::write(&hunt, "").unwrap();
        fs::write(&daily, "").unwrap();
        let mut store = Store::open(path.clone());
        assert_eq!(store.saved(&hunt), Supplied::Values(Values::new()));

        store.save(&hunt, values(json!({ "kills": 12 }))).unwrap();
        store.save(&daily, values(json!({ "day": "All" }))).unwrap();
        store
            .save(
                &folder.join(".").join("hunt.ts"),
                values(json!({ "kills": 3, "buy": ["A"] })),
            )
            .unwrap();

        let reopened = Store::open(path);
        assert_eq!(
            reopened.saved(&hunt),
            Supplied::Values(values(json!({ "kills": 3, "buy": ["A"] })))
        );
        assert_eq!(
            reopened.saved(&daily),
            Supplied::Values(values(json!({ "day": "All" })))
        );
        assert_eq!(
            reopened.saved(&folder.join("other.ts")),
            Supplied::Values(Values::new())
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_kept_until_the_user_starts_fresh() {
        let folder = scratch();
        let path = folder.join("script-inputs.json");
        let hunt = folder.join("hunt.ts");
        fs::write(&path, "{ \"oops\": ").unwrap();
        let mut store = Store::open(path.clone());
        assert_eq!(store.saved(&hunt), Supplied::StoreUnreadable);
        assert!(store.save(&hunt, values(json!({ "kills": 1 }))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"oops\": ");

        store.start_fresh().unwrap();
        assert_eq!(store.saved(&hunt), Supplied::Values(Values::new()));
        assert!(!path.exists());
        let backups: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".json.bad"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(folder.join(&backups[0])).unwrap(),
            "{ \"oops\": "
        );

        store.save(&hunt, values(json!({ "kills": 1 }))).unwrap();
        assert_eq!(
            Store::open(path).saved(&hunt),
            Supplied::Values(values(json!({ "kills": 1 })))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_failed_backup_leaves_the_unreadable_file_alone() {
        let folder = scratch();
        let path = folder.join("script-inputs.json");
        fs::write(&path, "[]").unwrap();
        let mut store = Store::open(path.clone());
        let writable = fs::metadata(&folder).unwrap().permissions();
        let mut locked = writable.clone();
        locked.set_readonly(true);
        fs::set_permissions(&folder, locked).unwrap();

        assert!(store.start_fresh().is_err());
        assert_eq!(
            store.saved(&folder.join("hunt.ts")),
            Supplied::StoreUnreadable
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");

        fs::set_permissions(&folder, writable).unwrap();
        fs::remove_dir_all(folder).unwrap();
    }
}
