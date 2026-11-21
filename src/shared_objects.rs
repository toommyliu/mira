use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gpui::{App, Global};

use crate::accounts::{AccountId, Accounts};

const SEED: &str = ".seed-";

pub(crate) struct SharedObjects {
    root: PathBuf,
}

impl Global for SharedObjects {}

impl SharedObjects {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn shared(&self) -> PathBuf {
        self.root.join("default")
    }

    fn accounts(&self) -> PathBuf {
        self.root.join("accounts")
    }

    fn folder_of(&self, account: &AccountId) -> io::Result<PathBuf> {
        let name = account.as_str();
        if name.is_empty() || name.starts_with('.') || name.contains(['/', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name:?} can't name a shared object folder"),
            ));
        }
        Ok(self.accounts().join(name))
    }

    fn seed_of(&self, account: &AccountId) -> PathBuf {
        self.accounts().join(format!("{SEED}{}", account.as_str()))
    }

    pub(crate) fn account(&self, account: &AccountId) -> io::Result<PathBuf> {
        let folder = self.folder_of(account)?;
        if folder.try_exists()? {
            return Ok(folder);
        }
        let seed = self.seed_of(account);
        remove(&seed)?;
        copy(&self.shared(), &seed)?;
        fs::rename(&seed, &folder)?;
        Ok(folder)
    }

    pub(crate) fn rename(&self, old: &AccountId, new: &AccountId) -> io::Result<()> {
        let (from, to) = (self.folder_of(old)?, self.folder_of(new)?);
        remove(&to)?;
        match fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            moved => moved,
        }
    }

    pub(crate) fn forget(&self, account: &AccountId) -> io::Result<()> {
        remove(&self.folder_of(account)?)?;
        remove(&self.seed_of(account))
    }

    pub(crate) fn sweep(&self, accounts: &Path) -> io::Result<()> {
        if !accounts.try_exists()? {
            return Ok(());
        }
        let accounts = Accounts::open(accounts.to_path_buf()).map_err(io::Error::other)?;
        let entries = match fs::read_dir(self.accounts()) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            entries => entries?,
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let kept = name.to_str().is_some_and(|name| {
                !name.starts_with(SEED)
                    && accounts
                        .list()
                        .iter()
                        .any(|account| account.id().as_str() == name)
            });
            if !kept {
                remove(&entry.path())?;
            }
        }
        Ok(())
    }
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let entries = match fs::read_dir(from) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy(&entry.path(), &to.join(name))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(name))?;
        }
    }
    Ok(())
}

pub(crate) fn init(root: PathBuf, accounts: &Path, cx: &mut App) {
    let shared_objects = SharedObjects::new(root);
    if let Err(error) = shared_objects.sweep(accounts) {
        tracing::warn!(%error, "couldn't clear shared objects of removed accounts");
    }
    cx.set_global(shared_objects);
}

pub(crate) fn rename(old: &AccountId, new: &AccountId, cx: &App) {
    if let Err(error) = cx.global::<SharedObjects>().rename(old, new) {
        tracing::error!(%error, "couldn't move an account's shared objects");
    }
}

pub(crate) fn forget(account: &AccountId, cx: &App) {
    if let Err(error) = cx.global::<SharedObjects>().forget(account) {
        tracing::error!(%error, "couldn't delete an account's shared objects");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_folder() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "mira-shared-objects-{:x}",
            u64::from_ne_bytes(random)
        ))
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn save_accounts(path: &Path, usernames: &[&str]) {
        let accounts: Vec<_> = usernames
            .iter()
            .map(|username| serde_json::json!({ "name": username, "username": username, "password": "secret" }))
            .collect();
        write(
            path,
            &serde_json::json!({ "accounts": accounts, "setups": [] }).to_string(),
        );
    }

    #[test]
    fn an_account_starts_from_the_shared_settings_once() {
        let root = temporary_folder();
        let shared_objects = SharedObjects::new(root.clone());
        let options = shared_objects.shared().join("localhost/AQLite_Data.sol");
        write(&options, "keybinds");
        write(&shared_objects.shared().join(".DS_Store"), "finder");
        let alice = AccountId::of("Alice");
        write(
            &shared_objects.seed_of(&alice).join("localhost/stale.sol"),
            "interrupted",
        );

        let folder = shared_objects.account(&alice).unwrap();
        assert_eq!(read(&folder.join("localhost/AQLite_Data.sol")), "keybinds");
        assert_eq!(names(&folder), ["localhost"]);
        assert_eq!(names(&folder.join("localhost")), ["AQLite_Data.sol"]);

        write(&options, "changed later");
        write(&folder.join("localhost/AQLite_Data.sol"), "alice's own");
        let again = shared_objects.account(&alice).unwrap();
        assert_eq!(again, folder);
        assert_eq!(
            read(&again.join("localhost/AQLite_Data.sol")),
            "alice's own"
        );
        assert_eq!(names(&root.join("accounts")), ["alice"]);

        fs::remove_dir_all(shared_objects.shared()).unwrap();
        let bob = shared_objects.account(&AccountId::of("bob")).unwrap();
        assert!(names(&bob).is_empty());

        assert!(shared_objects.account(&AccountId::of("..")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folders_follow_renames_and_removals() {
        let root = temporary_folder();
        let shared_objects = SharedObjects::new(root.clone());
        let (alice, ally, bob) = (
            AccountId::of("alice"),
            AccountId::of("ally"),
            AccountId::of("bob"),
        );
        let options = shared_objects
            .account(&alice)
            .unwrap()
            .join("localhost/AQLite_Data.sol");
        write(&options, "alice's own");
        write(
            &root.join("accounts/ally/localhost/AQLite_Data.sol"),
            "left behind",
        );

        shared_objects.rename(&alice, &ally).unwrap();
        assert_eq!(names(&root.join("accounts")), ["ally"]);
        assert_eq!(
            read(&root.join("accounts/ally/localhost/AQLite_Data.sol")),
            "alice's own"
        );

        write(
            &root.join("accounts/bob/localhost/AQLite_Data.sol"),
            "left behind",
        );
        shared_objects.rename(&alice, &bob).unwrap();
        assert_eq!(names(&root.join("accounts")), ["ally"]);

        write(
            &shared_objects.seed_of(&ally).join("localhost/stale.sol"),
            "interrupted",
        );
        shared_objects.forget(&ally).unwrap();
        shared_objects.forget(&ally).unwrap();
        assert!(names(&root.join("accounts")).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_clears_folders_of_removed_accounts_only() {
        let root = temporary_folder();
        let shared_objects = SharedObjects::new(root.clone());
        let accounts = root.join("accounts.json");
        for name in ["alice", "bob", ".seed-alice"] {
            write(&root.join("accounts").join(name).join("data.sol"), name);
        }
        write(&root.join("accounts/.DS_Store"), "finder");
        write(
            &shared_objects.shared().join("localhost/data.sol"),
            "shared",
        );
        let everything = [".DS_Store", ".seed-alice", "alice", "bob"];

        shared_objects.sweep(&accounts).unwrap();
        assert_eq!(names(&root.join("accounts")), everything);

        write(&accounts, "{ not json");
        assert!(shared_objects.sweep(&accounts).is_err());
        assert_eq!(names(&root.join("accounts")), everything);

        save_accounts(&accounts, &["Alice"]);
        shared_objects.sweep(&accounts).unwrap();
        assert_eq!(names(&root.join("accounts")), [".DS_Store", "alice"]);
        assert_eq!(names(&shared_objects.shared()), ["localhost"]);
        fs::remove_dir_all(root).unwrap();
    }
}
