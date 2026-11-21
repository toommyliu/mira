use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use ruffle_core::backend::storage::StorageBackend;

static WRITES: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Storage {
    folder: PathBuf,
}

impl Storage {
    pub(crate) fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn path_of(&self, name: &str) -> Option<PathBuf> {
        Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
            .then(|| self.folder.join(format!("{name}.sol")))
    }
}

impl StorageBackend for Storage {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.path_of(name)?;
        match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, path = %path.display(), "couldn't read a shared object");
                }
                None
            }
        }
    }

    fn put(&mut self, name: &str, value: &[u8]) -> bool {
        let Some(path) = self.path_of(name) else {
            return false;
        };
        match replace(&path, value) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "couldn't save a shared object");
                false
            }
        }
    }

    fn remove_key(&mut self, name: &str) {
        if let Some(path) = self.path_of(name) {
            let _ = fs::remove_file(path);
        }
    }
}

fn replace(path: &Path, value: &[u8]) -> io::Result<()> {
    let (Some(folder), Some(file)) = (path.parent(), path.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    fs::create_dir_all(folder)?;
    let temporary = folder.join(format!(
        ".{}.{}.{}.tmp",
        file.to_string_lossy(),
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, value)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_folder() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!("mira-storage-{:x}", u64::from_ne_bytes(random)))
    }

    #[test]
    fn players_sharing_a_folder_never_read_a_partial_save() {
        let folder = temporary_folder();
        let old = vec![1; 256 * 1024];
        let new = vec![2; 128 * 1024];
        let mut writer = Storage::new(folder.clone());
        assert!(writer.put("localhost/AQLite_Data", &old));
        let reader = Storage::new(folder.clone());
        std::thread::scope(|scope| {
            let (old, new) = (&old, &new);
            scope.spawn(move || {
                for round in 0..200 {
                    let value = if round % 2 == 0 { new } else { old };
                    assert!(writer.put("localhost/AQLite_Data", value));
                }
            });
            for _ in 0..2000 {
                let read = reader.get("localhost/AQLite_Data").unwrap();
                assert!(read == *old || read == *new, "read {} bytes", read.len());
            }
        });
        let names: Vec<_> = fs::read_dir(folder.join("localhost"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["AQLite_Data.sol"]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn names_that_leave_the_folder_are_refused() {
        let folder = temporary_folder();
        let mut storage = Storage::new(folder.join("default"));
        for name in ["../escaped", "/escaped", "localhost/../../escaped"] {
            assert!(!storage.put(name, b"data"), "{name}");
            assert_eq!(storage.get(name), None, "{name}");
        }
        assert!(!folder.join("escaped.sol").exists());
        assert!(!folder.exists());
    }
}
