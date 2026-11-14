use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::accounts::AccountId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Setup {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) script: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    pub(crate) members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Member {
    pub(crate) username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) script: Option<PathBuf>,
}

impl Member {
    pub(crate) fn id(&self) -> AccountId {
        AccountId::of(&self.username)
    }
}

impl Setup {
    pub(crate) fn member(&self, account: &AccountId) -> Option<&Member> {
        self.members.iter().find(|member| &member.id() == account)
    }

    pub(crate) fn script_for(&self, account: &AccountId) -> Option<&Path> {
        let member = self.member(account)?;
        member.script.as_deref().or(self.script.as_deref())
    }

    pub(crate) fn add(&mut self, usernames: impl IntoIterator<Item = String>) {
        for username in usernames {
            if self.member(&AccountId::of(&username)).is_none() {
                self.members.push(Member {
                    username,
                    script: None,
                });
            }
        }
    }

    pub(crate) fn remove(&mut self, accounts: &[AccountId]) {
        self.members
            .retain(|member| !accounts.contains(&member.id()));
    }

    pub(crate) fn pick(&mut self, accounts: &[AccountId], script: Option<PathBuf>) {
        for member in self
            .members
            .iter_mut()
            .filter(|member| accounts.contains(&member.id()))
        {
            member.script.clone_from(&script);
        }
    }
}

const FIRST: &str = "My scripts";
pub(crate) const NEW: &str = "New setup";

pub(crate) fn seeded(scripts: Vec<(String, PathBuf)>) -> Option<Setup> {
    let mut counts: HashMap<&Path, usize> = HashMap::new();
    for (_, script) in &scripts {
        *counts.entry(script).or_default() += 1;
    }
    let common = scripts
        .iter()
        .map(|(_, script)| script.as_path())
        .fold(None, |best: Option<&Path>, script| match best {
            Some(best) if counts[best] >= counts[script] => Some(best),
            _ => Some(script),
        })?
        .to_path_buf();
    let members = scripts
        .into_iter()
        .map(|(username, script)| Member {
            username,
            script: (script != common).then_some(script),
        })
        .collect();
    Some(Setup {
        name: FIRST.into(),
        script: Some(common),
        server: None,
        members,
    })
}

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

    fn member(username: &str, script: Option<&str>) -> Member {
        Member {
            username: username.into(),
            script: script.map(Into::into),
        }
    }

    fn setup(name: &str, members: Vec<Member>) -> Setup {
        Setup {
            name: name.into(),
            script: Some("/scripts/daily.ts".into()),
            server: None,
            members,
        }
    }

    #[test]
    fn an_account_runs_the_setup_script_unless_it_picks_another() {
        let warlord = Setup {
            name: "Warlord farm".into(),
            script: Some("/scripts/warlord.ts".into()),
            server: None,
            members: vec![
                member("Main", None),
                member("alt2", Some("/scripts/follower.ts")),
            ],
        };
        assert_eq!(
            warlord.script_for(&AccountId::of("main")),
            Some(Path::new("/scripts/warlord.ts"))
        );
        assert_eq!(
            warlord.script_for(&AccountId::of("alt2")),
            Some(Path::new("/scripts/follower.ts"))
        );
        assert_eq!(warlord.script_for(&AccountId::of("alt3")), None);
    }

    #[test]
    fn a_renamed_setup_needs_a_name_no_other_setup_has() {
        let list = vec![
            setup("Dailies", vec![member("main", None)]),
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
    fn members_are_added_once_and_only_picked_ones_change_script() {
        let mut dailies = setup("Dailies", vec![member("main", None)]);
        dailies.add(["Main".to_owned(), "alt1".to_owned(), "alt2".to_owned()]);
        dailies.pick(
            &[AccountId::of("alt1"), AccountId::of("alt9")],
            Some("/scripts/follower.ts".into()),
        );
        dailies.remove(&[AccountId::of("alt2")]);
        assert_eq!(
            dailies.members,
            [
                member("main", None),
                member("alt1", Some("/scripts/follower.ts")),
            ]
        );
    }

    #[test]
    fn scripts_saved_on_accounts_become_one_setup() {
        let seeded = seeded(vec![
            ("main".to_owned(), PathBuf::from("/scripts/warlord.ts")),
            ("alt1".to_owned(), PathBuf::from("/scripts/warlord.ts")),
            ("alt2".to_owned(), PathBuf::from("/scripts/follower.ts")),
        ]);
        assert_eq!(
            seeded,
            Some(Setup {
                name: "My scripts".into(),
                script: Some("/scripts/warlord.ts".into()),
                server: None,
                members: vec![
                    member("main", None),
                    member("alt1", None),
                    member("alt2", Some("/scripts/follower.ts")),
                ],
            })
        );
        assert_eq!(super::seeded(Vec::new()), None);
    }
}
