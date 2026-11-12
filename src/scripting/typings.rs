use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use tracing::warn;

include!(concat!(env!("OUT_DIR"), "/script_types.rs"));

const STAMP: &str = "version";

pub(crate) fn install_types(dir: &Path) -> io::Result<()> {
    let partial = dir.with_extension("partial");
    let replaced = dir.with_extension("old");
    remove(&partial)?;
    remove(&replaced)?;
    if fs::read_to_string(dir.join(STAMP)).is_ok_and(|stamp| stamp == VERSION) {
        return Ok(());
    }
    for (path, text) in FILES {
        let path = partial.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    fs::write(partial.join(STAMP), VERSION)?;
    match fs::rename(dir, &replaced) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    fs::rename(&partial, dir)?;
    remove(&replaced)
}

pub(crate) fn link_types<'a>(folders: impl IntoIterator<Item = &'a Path>, types: &Path) {
    for folder in folders {
        if let Err(error) = link(folder, types) {
            warn!(%error, path = %folder.display(), "couldn't add a tsconfig.json");
        }
    }
}

fn link(folder: &Path, types: &Path) -> io::Result<()> {
    let config = serde_json::json!({ "extends": types.join("tsconfig.json") });
    let created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join("tsconfig.json"));
    let mut file = match created {
        Ok(file) => file,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    writeln!(file, "{}", serde_json::to_string_pretty(&config)?)
}

fn remove(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let dir =
            std::env::temp_dir().join(format!("mira-typings-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_writes_once_and_replaces_stale_types() {
        let root = scratch();
        let types = root.join("types");
        fs::create_dir_all(root.join("types.partial/leftover")).unwrap();
        fs::create_dir_all(root.join("types.old/leftover")).unwrap();

        install_types(&types).unwrap();
        assert_eq!(fs::read_to_string(types.join(STAMP)).unwrap(), VERSION);
        for (path, text) in FILES {
            assert_eq!(fs::read_to_string(types.join(path)).unwrap(), *text);
        }
        assert!(!root.join("types.partial").exists());
        assert!(!root.join("types.old").exists());

        let config = types.join("tsconfig.json");
        fs::write(&config, "edited").unwrap();
        install_types(&types).unwrap();
        assert_eq!(fs::read_to_string(&config).unwrap(), "edited");

        fs::write(types.join(STAMP), "stale").unwrap();
        fs::write(types.join("unknown.d.ts"), "").unwrap();
        install_types(&types).unwrap();
        assert_ne!(fs::read_to_string(&config).unwrap(), "edited");
        assert_eq!(fs::read_to_string(types.join(STAMP)).unwrap(), VERSION);
        assert!(!types.join("unknown.d.ts").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn link_creates_a_tsconfig_only_where_none_exists() {
        let root = scratch();
        let types = root.join("types");
        let empty = root.join("empty");
        let owned = root.join("owned");
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("tsconfig.json"), "{ \"mine\": true }").unwrap();

        for _ in 0..2 {
            link(&empty, &types).unwrap();
            link(&owned, &types).unwrap();
            link(&root.join("missing"), &types).unwrap();
        }

        let linked: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(empty.join("tsconfig.json")).unwrap())
                .unwrap();
        assert_eq!(
            linked,
            serde_json::json!({ "extends": types.join("tsconfig.json") })
        );
        assert_eq!(
            fs::read_to_string(owned.join("tsconfig.json")).unwrap(),
            "{ \"mine\": true }"
        );
        assert!(!root.join("missing").exists());

        fs::remove_dir_all(root).unwrap();
    }
}
