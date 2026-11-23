use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, de::DeserializeOwned};

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    read_json(path)
        .unwrap_or_else(|error| {
            set_aside(path, &error);
            None
        })
        .unwrap_or_default()
}

pub(crate) fn set_aside(path: &Path, problem: &dyn fmt::Display) {
    match move_aside(path) {
        Ok(backup) => {
            tracing::warn!(%problem, path = %path.display(), backup = %backup.display(), "moved an unreadable file aside");
        }
        Err(error) => {
            tracing::error!(%problem, %error, path = %path.display(), "couldn't move an unreadable file aside");
        }
    }
}

pub(crate) fn move_aside(path: &Path) -> io::Result<PathBuf> {
    let backup = backup_of(path);
    fs::rename(path, &backup)?;
    Ok(backup)
}

pub(crate) fn copy_aside(path: &Path) -> io::Result<PathBuf> {
    let backup = backup_of(path);
    fs::copy(path, &backup)?;
    Ok(backup)
}

fn backup_of(path: &Path) -> PathBuf {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map_or_else(String::new, |extension| format!(".{}", extension.display()));
    (0..)
        .map(|attempt| {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!("-{attempt}")
            };
            path.with_file_name(format!("{stem}-{seconds}{suffix}{extension}.bad"))
        })
        .find(|backup| !backup.exists())
        .expect("some backup name is free")
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    write(path, &bytes)
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace(path, bytes, &OpenOptions::new(), false)
}

pub(crate) fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace(path, bytes, OpenOptions::new().mode(0o600), true)
}

fn replace(path: &Path, bytes: &[u8], options: &OpenOptions, sync: bool) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut file = options
        .clone()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    if sync {
        file.sync_all()?;
    }
    fs::rename(temporary, path)
}

#[cfg(test)]
pub(crate) fn backed_up(folder: &Path, contents: &str) -> bool {
    fs::read_dir(folder).unwrap().any(|entry| {
        let path = entry.unwrap().path();
        path.extension().is_some_and(|extension| extension == "bad")
            && fs::read_to_string(path).is_ok_and(|text| text == contents)
    })
}
