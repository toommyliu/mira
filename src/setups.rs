use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::accounts::AccountId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Setup {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) script: Option<PathBuf>,
    pub(crate) members: Vec<Member>,
    #[serde(default, skip_serializing_if = "Launch::is_default")]
    pub(crate) launch: Launch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Member {
    pub(crate) username: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Launch {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) new_window: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) grid: bool,
}

impl Launch {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Member {
    pub(crate) fn id(&self) -> AccountId {
        AccountId::of(&self.username)
    }
}

impl Setup {
    pub(crate) fn new(name: String, script: Option<PathBuf>, members: Vec<Member>) -> Self {
        Self {
            name,
            script,
            members,
            launch: Launch::default(),
        }
    }

    pub(crate) fn member(&self, account: &AccountId) -> Option<&Member> {
        self.members.iter().find(|member| &member.id() == account)
    }

    pub(crate) fn add(&mut self, usernames: impl IntoIterator<Item = String>) {
        for username in usernames {
            if self.member(&AccountId::of(&username)).is_none() {
                self.members.push(Member { username });
            }
        }
    }

    pub(crate) fn remove(&mut self, accounts: &[AccountId]) {
        self.members
            .retain(|member| !accounts.contains(&member.id()));
    }

    pub(crate) fn move_member(&mut self, from: usize, to: usize) {
        if from < self.members.len() && to < self.members.len() {
            let member = self.members.remove(from);
            self.members.insert(to, member);
        }
    }
}

pub(crate) const NEW: &str = "New setup";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Invalid {
    #[error("Enter a name.")]
    NoName,
    #[error("Another setup is already called {0}.")]
    Taken(String),
}

pub(crate) fn renamed_setup(
    setups: &[Setup],
    old: &str,
    name: &str,
) -> Result<Vec<Setup>, Invalid> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Invalid::NoName);
    }
    let taken = setups
        .iter()
        .any(|other| other.name != old && other.name.eq_ignore_ascii_case(name));
    if taken {
        return Err(Invalid::Taken(name.to_owned()));
    }
    let mut list = setups.to_vec();
    if let Some(setup) = list.iter_mut().find(|setup| setup.name == old) {
        name.clone_into(&mut setup.name);
    }
    Ok(list)
}

pub(crate) fn unused_name(setups: &[Setup], name: &str) -> String {
    (1..)
        .map(|number| match number {
            1 => name.to_owned(),
            number => format!("{name} {number}"),
        })
        .find(|candidate| {
            !setups
                .iter()
                .any(|other| other.name.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or_default()
}

pub(crate) fn renamed(setups: &[Setup], old: &AccountId, username: &str) -> Vec<Setup> {
    let mut list = setups.to_vec();
    for member in list
        .iter_mut()
        .flat_map(|setup| setup.members.iter_mut())
        .filter(|member| &member.id() == old)
    {
        member.username = username.to_owned();
    }
    list
}

pub(crate) fn forgotten(setups: &[Setup], account: &AccountId) -> Vec<Setup> {
    let mut list = setups.to_vec();
    for setup in &mut list {
        setup.members.retain(|member| &member.id() != account);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(username: &str) -> Member {
        Member {
            username: username.into(),
        }
    }

    fn setup(name: &str, members: Vec<Member>) -> Setup {
        Setup::new(name.into(), Some("/scripts/daily.ts".into()), members)
    }

    #[test]
    fn a_renamed_setup_needs_a_name_no_other_setup_has() {
        let list = vec![
            setup("Dailies", vec![member("main")]),
            setup("Bosses", Vec::new()),
        ];
        assert_eq!(renamed_setup(&list, "Dailies", "  "), Err(Invalid::NoName));
        assert_eq!(
            renamed_setup(&list, "Bosses", "DAILIES")
                .unwrap_err()
                .to_string(),
            "Another setup is already called DAILIES."
        );
        assert!(renamed_setup(&list, "Bosses", "bosses").is_ok());
        let list = renamed_setup(&list, "Dailies", " Daily quests ").unwrap();
        let names: Vec<_> = list.iter().map(|setup| setup.name.as_str()).collect();
        assert_eq!(names, ["Daily quests", "Bosses"]);
    }

    #[test]
    fn a_new_name_takes_the_next_free_number() {
        let list = vec![
            setup("New setup", Vec::new()),
            setup("Dailies copy", Vec::new()),
        ];
        assert_eq!(unused_name(&list, "New setup"), "New setup 2");
        assert_eq!(unused_name(&list, "Dailies copy"), "Dailies copy 2");
        assert_eq!(unused_name(&list, "Bosses copy"), "Bosses copy");
    }

    #[test]
    fn members_are_added_once_and_keep_the_order_they_launch_in() {
        let mut dailies = setup("Dailies", vec![member("main")]);
        dailies.add(["Main".to_owned(), "alt1".to_owned(), "alt2".to_owned()]);
        dailies.move_member(2, 0);
        dailies.move_member(0, 9);
        assert_eq!(
            dailies.members,
            [member("alt2"), member("main"), member("alt1")]
        );
        dailies.remove(&[AccountId::of("main")]);
        assert_eq!(dailies.members, [member("alt2"), member("alt1")]);
    }
}
