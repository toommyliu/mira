use std::{
    collections::HashSet,
    fs,
    io::{self, Write as _},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::domain::auth::Credentials;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AccountId(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Account {
    pub(crate) name: String,
    pub(crate) username: String,
    pub(crate) password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) script: Option<PathBuf>,
}

impl AccountId {
    pub(crate) fn of(username: &str) -> Self {
        Self(username.to_lowercase())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Account {
    pub(crate) fn id(&self) -> AccountId {
        AccountId::of(&self.username)
    }

    pub(crate) fn credentials(&self) -> Credentials {
        Credentials {
            username: self.username.clone(),
            password: self.password.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Draft {
    pub(crate) name: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) server: Option<String>,
    pub(crate) script: Option<PathBuf>,
}

impl Draft {
    pub(crate) fn of(account: &Account) -> Self {
        Self {
            name: account.name.clone(),
            username: account.username.clone(),
            password: account.password.clone(),
            server: account.server.clone(),
            script: account.script.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) server: Option<Option<String>>,
    pub(crate) script: Option<Option<PathBuf>>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Invalid {
    #[error("Enter a username.")]
    NoUsername,
    #[error("Enter a password.")]
    NoPassword,
    #[error("{0} is already an account.")]
    Taken(String),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveError {
    #[error(transparent)]
    Invalid(#[from] Invalid),
    #[error("couldn't save the accounts: {0}")]
    Io(#[from] io::Error),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    accounts: Vec<Account>,
}

pub(crate) struct Accounts {
    path: PathBuf,
    list: Vec<Account>,
}

impl Accounts {
    pub(crate) fn open(path: PathBuf) -> Result<Self, String> {
        let list = match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<File>(&bytes)
                    .map_err(|error| format!("{} isn't valid: {error}", path.display()))?
                    .accounts
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("couldn't read {}: {error}", path.display())),
        };
        Ok(Self { path, list })
    }

    pub(crate) fn list(&self) -> &[Account] {
        &self.list
    }

    pub(crate) fn get(&self, id: &AccountId) -> Option<&Account> {
        self.list.iter().find(|account| &account.id() == id)
    }

    pub(crate) fn save(
        &mut self,
        editing: Option<&AccountId>,
        draft: Draft,
    ) -> Result<AccountId, SaveError> {
        let username = draft.username.trim().to_owned();
        if username.is_empty() {
            return Err(Invalid::NoUsername.into());
        }
        if draft.password.is_empty() {
            return Err(Invalid::NoPassword.into());
        }
        let id = AccountId::of(&username);
        if editing != Some(&id) && self.get(&id).is_some() {
            return Err(Invalid::Taken(username).into());
        }
        let name = draft.name.trim();
        let account = Account {
            name: if name.is_empty() {
                username.clone()
            } else {
                name.to_owned()
            },
            username,
            password: draft.password,
            server: server_name(draft.server),
            script: draft.script,
        };
        let mut list = self.list.clone();
        match editing.and_then(|editing| list.iter().position(|account| &account.id() == editing)) {
            Some(index) => list[index] = account,
            None => list.push(account),
        }
        write(&self.path, &list)?;
        self.list = list;
        Ok(id)
    }

    pub(crate) fn change(&mut self, ids: &HashSet<AccountId>, change: Change) -> io::Result<()> {
        let server = change.server.map(server_name);
        let mut list = self.list.clone();
        for account in list
            .iter_mut()
            .filter(|account| ids.contains(&account.id()))
        {
            if let Some(server) = &server {
                account.server = server.clone();
            }
            if let Some(script) = &change.script {
                account.script = script.clone();
            }
        }
        write(&self.path, &list)?;
        self.list = list;
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: &AccountId) -> io::Result<()> {
        let list: Vec<_> = self
            .list
            .iter()
            .filter(|account| &account.id() != id)
            .cloned()
            .collect();
        write(&self.path, &list)?;
        self.list = list;
        Ok(())
    }
}

fn server_name(server: Option<String>) -> Option<String> {
    server
        .map(|server| server.trim().to_owned())
        .filter(|server| !server.is_empty())
}

fn write(path: &Path, accounts: &[Account]) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(&File {
        accounts: accounts.to_vec(),
    })
    .map_err(io::Error::other)?;
    bytes.push(b'\n');
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn temporary_path() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "mira-accounts-{:x}.json",
            u64::from_ne_bytes(random)
        ))
    }

    fn draft(username: &str) -> Draft {
        Draft {
            username: username.into(),
            password: "secret".into(),
            ..Draft::default()
        }
    }

    #[test]
    fn saved_accounts_come_back_after_a_restart_in_a_file_only_the_owner_can_read() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        accounts
            .save(
                None,
                Draft {
                    name: " Farmer ".into(),
                    username: " player1 ".into(),
                    password: " pass word ".into(),
                    server: Some(" Twilly ".into()),
                    script: Some("/scripts/farm.ts".into()),
                },
            )
            .unwrap();
        accounts.save(None, draft("player2")).unwrap();
        let reopened = Accounts::open(path.clone()).unwrap();
        assert_eq!(
            reopened.list(),
            [
                Account {
                    name: "Farmer".into(),
                    username: "player1".into(),
                    password: " pass word ".into(),
                    server: Some("Twilly".into()),
                    script: Some("/scripts/farm.ts".into()),
                },
                Account {
                    name: "player2".into(),
                    username: "player2".into(),
                    password: "secret".into(),
                    server: None,
                    script: None,
                },
            ]
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_username_belongs_to_one_account_whatever_its_case() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        let first = accounts.save(None, draft("Player1")).unwrap();
        let second = accounts.save(None, draft("player2")).unwrap();
        assert_eq!(
            accounts
                .save(None, draft("PLAYER1"))
                .unwrap_err()
                .to_string(),
            "PLAYER1 is already an account."
        );
        assert!(matches!(
            accounts.save(Some(&second), draft("player1")),
            Err(SaveError::Invalid(Invalid::Taken(_)))
        ));
        let renamed = accounts.save(Some(&first), draft("player1")).unwrap();
        assert_eq!(renamed, first);
        assert_eq!(accounts.list().len(), 2);
        assert_eq!(accounts.list()[0].username, "player1");
        accounts.remove(&first).unwrap();
        assert_eq!(Accounts::open(path.clone()).unwrap().list().len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_change_applies_only_to_the_chosen_accounts_and_only_the_fields_it_sets() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        let first = accounts.save(None, draft("player1")).unwrap();
        let second = accounts.save(None, draft("player2")).unwrap();
        accounts.save(None, draft("player3")).unwrap();
        accounts
            .change(
                &HashSet::from([first.clone(), second]),
                Change {
                    server: Some(Some(" Twig ".into())),
                    script: Some(Some("/scripts/farm.ts".into())),
                },
            )
            .unwrap();
        accounts
            .change(
                &HashSet::from([first]),
                Change {
                    server: Some(Some(" ".into())),
                    script: None,
                },
            )
            .unwrap();
        let saved: Vec<_> = Accounts::open(path.clone())
            .unwrap()
            .list()
            .iter()
            .map(|account| (account.server.clone(), account.script.clone()))
            .collect();
        assert_eq!(
            saved,
            [
                (None, Some("/scripts/farm.ts".into())),
                (Some("Twig".into()), Some("/scripts/farm.ts".into())),
                (None, None),
            ]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_account_needs_a_username_and_password() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        assert!(matches!(
            accounts.save(None, draft("  ")),
            Err(SaveError::Invalid(Invalid::NoUsername))
        ));
        let no_password = Draft {
            password: String::new(),
            ..draft("player1")
        };
        assert!(matches!(
            accounts.save(None, no_password),
            Err(SaveError::Invalid(Invalid::NoPassword))
        ));
        assert!(accounts.list().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn a_file_that_cant_be_read_is_reported_and_left_alone() {
        let path = temporary_path();
        fs::write(&path, "{ not json").unwrap();
        assert!(Accounts::open(path.clone()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::remove_file(path).unwrap();
    }
}
