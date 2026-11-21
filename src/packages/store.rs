use std::{
    ffi::CString,
    fs, io,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use super::{
    PackagePath, RECEIPT, Sha,
    archive::{self, Files},
};

const STAGING: &str = ".staging";
const UNPACKED: &str = "unpacked";
const IGNORED: [&str; 1] = [".DS_Store"];
const GENERATED: [&str; 1] = ["tsconfig.json"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) repository: u64,
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) path: PackagePath,
    pub(crate) branch: String,
    pub(crate) commit: Sha,
    pub(crate) tree: Sha,
    pub(crate) files: Files,
}

impl Receipt {
    pub(crate) fn read(package: &Path) -> Result<Option<Self>, String> {
        match fs::read(package.join(RECEIPT)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| format!("Its install record is damaged: {error}.")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Couldn't read its install record: {error}.")),
        }
    }

    fn write(&self, package: &Path) -> io::Result<()> {
        fs::write(package.join(RECEIPT), serde_json::to_vec_pretty(self)?)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Edits {
    pub(crate) changed: Vec<PackagePath>,
    pub(crate) added: Vec<PackagePath>,
    pub(crate) removed: Vec<PackagePath>,
}

impl Edits {
    pub(crate) fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }

    pub(crate) fn find(package: &Path, baseline: &Files) -> io::Result<Self> {
        let mut edits = Self::default();
        let mut seen = Vec::new();
        visit(package, package, &mut |path, file| {
            if path.as_str() == RECEIPT {
                return Ok(());
            }
            match baseline.get(&path) {
                Some(hash) => {
                    if archive::hash(file)? != *hash {
                        edits.changed.push(path.clone());
                    }
                    seen.push(path);
                }
                None if GENERATED.contains(&path.as_str()) => {}
                None => edits.added.push(path),
            }
            Ok(())
        })?;
        edits.removed = baseline
            .keys()
            .filter(|path| !seen.contains(path))
            .cloned()
            .collect();
        edits.changed.sort();
        edits.added.sort();
        Ok(edits)
    }
}

fn visit(
    root: &Path,
    dir: &Path,
    found: &mut impl FnMut(PackagePath, &Path) -> io::Result<()>,
) -> io::Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name();
        if IGNORED.iter().any(|ignored| name == *ignored) {
            continue;
        }
        let path = item.path();
        if item.file_type()?.is_dir() {
            visit(root, &path, found)?;
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let Some(relative) = relative.to_str().and_then(PackagePath::parse) else {
            continue;
        };
        found(relative, &path)?;
    }
    Ok(())
}

pub(crate) type Trash = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    #[error("{0} already exists in the packages folder")]
    Exists(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

#[derive(Clone)]
pub(crate) struct Store {
    root: PathBuf,
    trash: Trash,
}

impl Store {
    pub(crate) fn new(root: PathBuf, trash: Trash) -> Self {
        Self { root, trash }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn package(&self, dir: &str) -> PathBuf {
        self.root.join(dir)
    }

    pub(crate) fn receipts(&self) -> Vec<(String, Receipt)> {
        let Ok(items) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        items
            .flatten()
            .filter_map(|item| {
                let dir = item.file_name().into_string().ok()?;
                let receipt = Receipt::read(&item.path()).ok()??;
                Some((dir, receipt))
            })
            .collect()
    }

    pub(crate) fn sweep(&self) -> io::Result<()> {
        let items = match fs::read_dir(self.root.join(STAGING)) {
            Ok(items) => items,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for item in items {
            (self.trash)(&item?.path())?;
        }
        Ok(())
    }

    pub(crate) fn stage(&self) -> io::Result<Staged> {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let dir = self
            .root
            .join(STAGING)
            .join(format!("{:016x}", u64::from_ne_bytes(random)));
        fs::create_dir_all(dir.join(UNPACKED))?;
        Ok(Staged { dir, kept: false })
    }

    pub(crate) fn publish(
        &self,
        staged: Staged,
        dir: &str,
        receipt: &Receipt,
    ) -> Result<PathBuf, StoreError> {
        receipt.write(&staged.unpacked())?;
        let target = self.package(dir);
        match rename(&staged.unpacked(), &target, libc::RENAME_EXCL) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(StoreError::Exists(dir.to_owned()))
            }
            result => Ok(result.map(|()| target)?),
        }
    }

    pub(crate) fn replace(
        &self,
        mut staged: Staged,
        dir: &str,
        receipt: &Receipt,
    ) -> io::Result<()> {
        receipt.write(&staged.unpacked())?;
        rename(&staged.unpacked(), &self.package(dir), libc::RENAME_SWAP)?;
        if let Err(error) = (self.trash)(&staged.unpacked()) {
            tracing::warn!(%error, path = %staged.dir.display(), "couldn't move the previous copy to the Trash");
            staged.kept = true;
        }
        Ok(())
    }

    pub(crate) fn uninstall(&self, dir: &str) -> io::Result<()> {
        if matches!(dir, "" | "." | "..") || dir.contains(['/', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid package folder name",
            ));
        }
        let package = self.package(dir);
        if !fs::metadata(&package)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "The package path is not a folder",
            ));
        }
        (self.trash)(&package)
    }
}

pub(crate) struct Staged {
    dir: PathBuf,
    kept: bool,
}

impl Staged {
    pub(crate) fn unpacked(&self) -> PathBuf {
        self.dir.join(UNPACKED)
    }

    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.kept
            && let Err(error) = fs::remove_dir_all(&self.dir)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %self.dir.display(), "couldn't clean up a package download");
        }
    }
}

fn rename(from: &Path, to: &Path, flags: libc::c_uint) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both arguments are valid NUL-terminated paths that outlive the call.
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn move_to_trash(path: &Path) -> io::Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| io::Error::other(error.localizedDescription().to_string()))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::packages::archive::tests::scratch;

    pub(crate) struct Bin {
        pub(crate) dir: PathBuf,
        pub(crate) broken: Arc<Mutex<bool>>,
    }

    impl Bin {
        pub(crate) fn new(root: &Path) -> (Self, Trash) {
            let dir = root.join("trash");
            fs::create_dir_all(&dir).unwrap();
            let broken = Arc::new(Mutex::new(false));
            let into = dir.clone();
            let failing = broken.clone();
            let trash: Trash = Arc::new(move |path: &Path| {
                if *failing.lock().unwrap() {
                    return Err(io::Error::other("the Trash is full"));
                }
                let mut random = [0; 8];
                getrandom::getrandom(&mut random).unwrap();
                fs::rename(path, into.join(format!("{:x}", u64::from_ne_bytes(random))))
            });
            (Self { dir, broken }, trash)
        }

        pub(crate) fn contents(&self) -> Vec<Vec<u8>> {
            let mut found = Vec::new();
            for item in fs::read_dir(&self.dir).unwrap() {
                let root = item.unwrap().path();
                visit(&root, &root, &mut |path, file| {
                    if !path.as_str().ends_with(RECEIPT) {
                        found.push(fs::read(file)?);
                    }
                    Ok(())
                })
                .unwrap();
            }
            found.sort();
            found
        }
    }

    fn sha(digit: char) -> Sha {
        Sha::parse(&digit.to_string().repeat(40)).unwrap()
    }

    pub(crate) fn receipt(files: Files) -> Receipt {
        Receipt {
            repository: 7,
            owner: "o".into(),
            repo: "r".into(),
            path: PackagePath::default(),
            branch: "main".into(),
            commit: sha('a'),
            tree: sha('b'),
            files,
        }
    }

    pub(crate) fn stage_files(store: &Store, files: &[(&str, &str)]) -> (Staged, Files) {
        let staged = store.stage().unwrap();
        let mut hashes = Files::new();
        for (path, text) in files {
            let file = staged.unpacked().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            hashes.insert(
                PackagePath::parse(path).unwrap(),
                archive::hash(&file).unwrap(),
            );
        }
        (staged, hashes)
    }

    fn staging_is_empty(store: &Store) -> bool {
        fs::read_dir(store.root().join(STAGING))
            .map(|mut items| items.next().is_none())
            .unwrap_or(true)
    }

    #[test]
    fn installing_publishes_the_package_with_its_receipt_and_never_overwrites() {
        let root = scratch("store");
        let (_bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        let (staged, files) = stage_files(&store, &[("package.json", "{}"), ("scripts/a.ts", "a")]);
        let installed = store
            .publish(staged, "o-r", &receipt(files.clone()))
            .unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("scripts/a.ts")).unwrap(),
            "a"
        );
        assert_eq!(Receipt::read(&installed), Ok(Some(receipt(files))));
        assert!(staging_is_empty(&store));

        let (again, files) = stage_files(&store, &[("scripts/a.ts", "different")]);
        assert!(matches!(
            store.publish(again, "o-r", &receipt(files)),
            Err(StoreError::Exists(dir)) if dir == "o-r"
        ));
        assert_eq!(
            fs::read_to_string(installed.join("scripts/a.ts")).unwrap(),
            "a"
        );
        assert!(staging_is_empty(&store));

        assert_eq!(Receipt::read(&root), Ok(None));
        fs::write(installed.join(RECEIPT), "{").unwrap();
        assert!(
            Receipt::read(&installed)
                .unwrap_err()
                .starts_with("Its install record is damaged")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edits_since_install_are_found_and_kept_in_the_trash_when_replaced() {
        let root = scratch("store");
        let (bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        let (staged, files) = stage_files(
            &store,
            &[
                ("scripts/a.ts", "a"),
                ("scripts/b.ts", "b"),
                ("lib/c.ts", "c"),
            ],
        );
        let installed = store
            .publish(staged, "o-r", &receipt(files.clone()))
            .unwrap();
        assert_eq!(Edits::find(&installed, &files).unwrap(), Edits::default());

        fs::write(installed.join("scripts/a.ts"), "my edit").unwrap();
        fs::remove_file(installed.join("lib/c.ts")).unwrap();
        fs::write(installed.join("scripts/mine.ts"), "mine").unwrap();
        fs::write(installed.join(".DS_Store"), "finder").unwrap();
        fs::write(installed.join("tsconfig.json"), "{}").unwrap();
        let path = |text: &str| PackagePath::parse(text).unwrap();
        assert_eq!(
            Edits::find(&installed, &files).unwrap(),
            Edits {
                changed: vec![path("scripts/a.ts")],
                added: vec![path("scripts/mine.ts")],
                removed: vec![path("lib/c.ts")],
            }
        );

        let (staged, next) = stage_files(&store, &[("scripts/a.ts", "new"), ("scripts/b.ts", "b")]);
        store
            .replace(staged, "o-r", &receipt(next.clone()))
            .unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("scripts/a.ts")).unwrap(),
            "new"
        );
        assert!(!installed.join("scripts/mine.ts").exists());
        assert_eq!(Receipt::read(&installed), Ok(Some(receipt(next.clone()))));
        assert_eq!(Edits::find(&installed, &next).unwrap(), Edits::default());
        let trashed = bin.contents();
        assert!(trashed.contains(&b"my edit".to_vec()));
        assert!(trashed.contains(&b"mine".to_vec()));
        assert!(staging_is_empty(&store));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_previous_copy_that_couldnt_be_trashed_is_trashed_on_the_next_sweep() {
        let root = scratch("store");
        let (bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        store.sweep().unwrap();
        let (staged, files) = stage_files(&store, &[("scripts/a.ts", "old")]);
        let installed = store.publish(staged, "o-r", &receipt(files)).unwrap();

        *bin.broken.lock().unwrap() = true;
        let (staged, next) = stage_files(&store, &[("scripts/a.ts", "new")]);
        store.replace(staged, "o-r", &receipt(next)).unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("scripts/a.ts")).unwrap(),
            "new"
        );
        assert!(!staging_is_empty(&store));
        let (abandoned, _) = stage_files(&store, &[("scripts/a.ts", "half")]);
        std::mem::forget(abandoned);

        *bin.broken.lock().unwrap() = false;
        store.sweep().unwrap();
        store.sweep().unwrap();
        assert!(staging_is_empty(&store));
        assert_eq!(bin.contents(), [b"half".to_vec(), b"old".to_vec()]);
        assert_eq!(
            fs::read_to_string(installed.join("scripts/a.ts")).unwrap(),
            "new"
        );

        store.uninstall("o-r").unwrap();
        assert!(!installed.exists());
        assert_eq!(
            bin.contents(),
            [b"half".to_vec(), b"new".to_vec(), b"old".to_vec()]
        );
        assert!(store.receipts().is_empty());
        assert_eq!(
            store.uninstall("o-r").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(root).unwrap();
    }
}
