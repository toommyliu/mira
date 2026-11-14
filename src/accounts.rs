use std::{
    collections::HashSet,
    fs,
    io::{self, Write as _},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::auth::Credentials,
    setups::{self, Member, Setup},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AccountId(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub(crate) name: String,
    pub(crate) username: String,
    pub(crate) password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
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
}

impl Draft {
    pub(crate) fn of(account: &Account) -> Self {
        Self {
            name: account.name.clone(),
            username: account.username.clone(),
            password: account.password.clone(),
            server: account.server.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) server: Option<Option<String>>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Invalid {
    #[error("Enter a username.")]
    NoUsername,
    #[error("Enter a password.")]
    NoPassword,
    #[error("You've already added {0}. Edit that account instead.")]
    Taken(String),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveError {
    #[error(transparent)]
    Invalid(#[from] Invalid),
    #[error("couldn't save the accounts: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SetupError {
    #[error(transparent)]
    Invalid(#[from] setups::Invalid),
    #[error("couldn't save the setups: {0}")]
    Io(#[from] io::Error),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    accounts: Vec<Account>,
    #[serde(default)]
    setups: Option<Vec<Setup>>,
}

pub(crate) struct Accounts {
    path: PathBuf,
    list: Vec<Account>,
    setups: Vec<Setup>,
}

impl Accounts {
    pub(crate) fn open(path: PathBuf) -> Result<Self, String> {
        let (list, setups) = match fs::read(&path) {
            Ok(bytes) => {
                let file = serde_json::from_slice::<File>(&bytes)
                    .map_err(|error| format!("{} isn't valid: {error}", path.display()))?;
                let setups = file.setups.unwrap_or_else(|| {
                    setups::seeded(legacy_scripts(&bytes)).into_iter().collect()
                });
                (file.accounts, setups)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
            Err(error) => return Err(format!("couldn't read {}: {error}", path.display())),
        };
        Ok(Self { path, list, setups })
    }

    pub(crate) fn list(&self) -> &[Account] {
        &self.list
    }

    pub(crate) fn setups(&self) -> &[Setup] {
        &self.setups
    }

    pub(crate) fn setup(&self, name: &str) -> Option<&Setup> {
        self.setups.iter().find(|setup| setup.name == name)
    }

    pub(crate) fn create_setup(
        &mut self,
        script: Option<PathBuf>,
        members: Vec<Member>,
    ) -> io::Result<String> {
        let name = setups::unused_name(&self.setups, setups::NEW);
        let mut setups = self.setups.clone();
        setups.push(Setup {
            name: name.clone(),
            script,
            server: None,
            members,
        });
        self.commit(self.list.clone(), setups)?;
        Ok(name)
    }

    pub(crate) fn rename_setup(&mut self, old: &str, name: &str) -> Result<String, SetupError> {
        let setups = setups::renamed_setup(&self.setups, old, name)?;
        self.commit(self.list.clone(), setups)?;
        Ok(name.trim().to_owned())
    }

    pub(crate) fn edit_setup(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut Setup),
    ) -> io::Result<()> {
        let mut setups = self.setups.clone();
        if let Some(setup) = setups.iter_mut().find(|setup| setup.name == name) {
            edit(setup);
        }
        self.commit(self.list.clone(), setups)
    }

    pub(crate) fn duplicate_setup(&mut self, name: &str) -> io::Result<Option<String>> {
        let Some(original) = self.setup(name).cloned() else {
            return Ok(None);
        };
        let copy = setups::unused_name(&self.setups, &format!("{name} copy"));
        let mut setups = self.setups.clone();
        setups.push(Setup {
            name: copy.clone(),
            ..original
        });
        self.commit(self.list.clone(), setups)?;
        Ok(Some(copy))
    }

    pub(crate) fn remove_setup(&mut self, name: &str) -> io::Result<()> {
        let setups = self
            .setups
            .iter()
            .filter(|setup| setup.name != name)
            .cloned()
            .collect();
        self.commit(self.list.clone(), setups)
    }

    fn commit(&mut self, list: Vec<Account>, setups: Vec<Setup>) -> io::Result<()> {
        write(&self.path, &list, &setups)?;
        self.list = list;
        self.setups = setups;
        Ok(())
    }

    pub(crate) fn get(&self, id: &AccountId) -> Option<&Account> {
        self.list.iter().find(|account| &account.id() == id)
    }

    pub(crate) fn check(&self, editing: Option<&AccountId>, draft: &Draft) -> Result<(), Invalid> {
        let username = draft.username.trim();
        if username.is_empty() {
            return Err(Invalid::NoUsername);
        }
        if draft.password.is_empty() {
            return Err(Invalid::NoPassword);
        }
        let id = AccountId::of(username);
        if editing != Some(&id) && self.get(&id).is_some() {
            return Err(Invalid::Taken(username.to_owned()));
        }
        Ok(())
    }

    pub(crate) fn save(
        &mut self,
        editing: Option<&AccountId>,
        draft: Draft,
    ) -> Result<AccountId, SaveError> {
        self.check(editing, &draft)?;
        let username = draft.username.trim().to_owned();
        let id = AccountId::of(&username);
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
        };
        let setups = match editing.filter(|editing| **editing != id) {
            Some(old) => setups::renamed(&self.setups, old, &account.username),
            None => self.setups.clone(),
        };
        let mut list = self.list.clone();
        match editing.and_then(|editing| list.iter().position(|account| &account.id() == editing)) {
            Some(index) => list[index] = account,
            None => list.push(account),
        }
        self.commit(list, setups)?;
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
        }
        self.commit(list, self.setups.clone())
    }

    pub(crate) fn remove(&mut self, id: &AccountId) -> io::Result<()> {
        let list: Vec<_> = self
            .list
            .iter()
            .filter(|account| &account.id() != id)
            .cloned()
            .collect();
        self.commit(list, setups::forgotten(&self.setups, id))
    }
}

fn legacy_scripts(bytes: &[u8]) -> Vec<(String, PathBuf)> {
    #[derive(Deserialize)]
    struct Legacy {
        accounts: Vec<LegacyAccount>,
    }
    #[derive(Deserialize)]
    struct LegacyAccount {
        username: String,
        #[serde(default)]
        script: serde_json::Value,
    }
    serde_json::from_slice::<Legacy>(bytes)
        .map(|file| {
            file.accounts
                .into_iter()
                .filter_map(|account| {
                    let script = account.script.as_str()?;
                    Some((account.username, PathBuf::from(script)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn server_name(server: Option<String>) -> Option<String> {
    server
        .map(|server| server.trim().to_owned())
        .filter(|server| !server.is_empty())
}

fn write(path: &Path, accounts: &[Account], setups: &[Setup]) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(&File {
        accounts: accounts.to_vec(),
        setups: Some(setups.to_vec()),
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
                },
                Account {
                    name: "player2".into(),
                    username: "player2".into(),
                    password: "secret".into(),
                    server: None,
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
            "You've already added PLAYER1. Edit that account instead."
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
    fn a_change_applies_only_to_the_chosen_accounts() {
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
                },
            )
            .unwrap();
        accounts
            .change(
                &HashSet::from([first]),
                Change {
                    server: Some(Some(" ".into())),
                },
            )
            .unwrap();
        let saved: Vec<_> = Accounts::open(path.clone())
            .unwrap()
            .list()
            .iter()
            .map(|account| account.server.clone())
            .collect();
        assert_eq!(saved, [None, Some("Twig".into()), None]);
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

    fn setup(name: &str, members: &[(&str, Option<&str>)]) -> Setup {
        Setup {
            name: name.into(),
            script: Some("/scripts/daily.ts".into()),
            server: None,
            members: members
                .iter()
                .map(|(username, script)| crate::setups::Member {
                    username: (*username).into(),
                    script: script.map(Into::into),
                })
                .collect(),
        }
    }

    fn add_setup(accounts: &mut Accounts, setup: Setup) {
        let name = accounts.create_setup(setup.script, setup.members).unwrap();
        accounts.rename_setup(&name, &setup.name).unwrap();
    }

    #[test]
    fn scripts_saved_on_accounts_become_a_setup_that_outlives_the_old_fields() {
        let path = temporary_path();
        fs::write(
            &path,
            r#"{ "accounts": [
                { "name": "Main", "username": "main", "password": "x", "script": "/scripts/farm.ts" },
                { "name": "Alt", "username": "alt", "password": "y" },
                { "name": "Odd", "username": "odd", "password": "z", "script": 123 }
            ] }"#,
        )
        .unwrap();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        let first = setup("My scripts", &[("main", None)]);
        let first = Setup {
            script: Some("/scripts/farm.ts".into()),
            ..first
        };
        assert_eq!(accounts.setups(), std::slice::from_ref(&first));
        accounts.save(None, draft("alt2")).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            saved["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|account| account.get("script").is_none())
        );
        assert_eq!(Accounts::open(path.clone()).unwrap().setups(), [first]);
        accounts.remove_setup("My scripts").unwrap();
        assert!(Accounts::open(path.clone()).unwrap().setups().is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn renaming_or_removing_an_account_updates_its_setups_in_the_same_save() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        let main = accounts.save(None, draft("main")).unwrap();
        let alt = accounts.save(None, draft("alt1")).unwrap();
        add_setup(
            &mut accounts,
            setup("Dailies", &[("main", None), ("alt1", None)]),
        );
        add_setup(&mut accounts, setup("Bosses", &[("Alt1", Some("/b.ts"))]));
        accounts.save(Some(&alt), draft("alt-one")).unwrap();
        accounts.remove(&main).unwrap();
        let reopened = Accounts::open(path.clone()).unwrap();
        assert_eq!(
            reopened.setups(),
            [
                setup("Dailies", &[("alt-one", None)]),
                setup("Bosses", &[("alt-one", Some("/b.ts"))]),
            ]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn setups_are_created_renamed_edited_duplicated_and_removed() {
        let path = temporary_path();
        let mut accounts = Accounts::open(path.clone()).unwrap();
        accounts.save(None, draft("main")).unwrap();
        assert_eq!(
            accounts.create_setup(None, Vec::new()).unwrap(),
            "New setup"
        );
        assert_eq!(
            accounts.create_setup(None, Vec::new()).unwrap(),
            "New setup 2"
        );
        assert!(matches!(
            accounts.rename_setup("New setup", " "),
            Err(SetupError::Invalid(crate::setups::Invalid::NoName))
        ));
        assert_eq!(
            accounts.rename_setup("New setup", " Dailies ").unwrap(),
            "Dailies"
        );
        accounts
            .edit_setup("Dailies", |setup| {
                setup.add(["main".to_owned()]);
                setup.server = Some("Twig".into());
            })
            .unwrap();
        assert_eq!(
            accounts.duplicate_setup("Dailies").unwrap().as_deref(),
            Some("Dailies copy")
        );
        accounts.remove_setup("Dailies").unwrap();
        let reopened = Accounts::open(path.clone()).unwrap();
        let names: Vec<_> = reopened
            .setups()
            .iter()
            .map(|setup| setup.name.clone())
            .collect();
        assert_eq!(names, ["New setup 2", "Dailies copy"]);
        let copy = reopened.setup("Dailies copy").unwrap();
        assert_eq!(
            copy.members,
            [Member {
                username: "main".into(),
                script: None,
            }]
        );
        assert_eq!(copy.server.as_deref(), Some("Twig"));
        fs::remove_file(path).unwrap();
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
