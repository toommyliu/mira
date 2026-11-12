use std::{sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::json;
use tokio::sync::watch;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Packets, Subscription},
    domain::{
        self,
        combat::{self, Skill},
        counter_attack::CounterAttacks,
        profiles::{Library, Profile, ProfileChoice, ProfileError},
        rotation::{self, Fighter},
        world::MonsterQuery,
    },
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Target,
    HasTarget,
    Attack {
        query: MonsterQuery,
    },
    UseSkill {
        skill: Skill,
        target: Option<MonsterQuery>,
        force: Option<bool>,
        wait_until_ready: Option<bool>,
    },
    CanUseSkill {
        skill: Skill,
    },
    SkillCooldown {
        skill: Skill,
    },
    CancelTarget,
    CancelAutoAttack,
    Exit,
    Kill {
        query: MonsterQuery,
        skills: Option<Vec<Skill>>,
        skill_interval_ms: Option<u64>,
        target_priority: Option<Vec<MonsterQuery>>,
        profile: Option<ProfileChoice>,
    },
    Hunt {
        query: MonsterQuery,
        prefer_most_matches: Option<bool>,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Target
            | Request::CancelTarget
            | Request::CancelAutoAttack
            | Request::Hunt { .. } => serde_json::Value::Null,
            Request::SkillCooldown { .. } => json!(0),
            Request::HasTarget
            | Request::Attack { .. }
            | Request::UseSkill { .. }
            | Request::CanUseSkill { .. }
            | Request::Exit
            | Request::Kill { .. } => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    counter_attacks: &CounterAttacks,
    profiles: &watch::Receiver<Arc<Library>>,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::UseSkill {
            skill,
            target,
            force,
            wait_until_ready,
        } => json!(
            combat::use_skill(
                loader,
                Packets::subscribe(&subscribe),
                counter_attacks,
                skill,
                target.as_ref(),
                force.unwrap_or(false),
                wait_until_ready.unwrap_or(false)
            )
            .await?
        ),
        Request::Kill {
            query,
            skills,
            skill_interval_ms,
            target_priority,
            profile,
        } => json!(
            combat::kill(
                loader,
                Packets::subscribe(&subscribe),
                counter_attacks,
                &query,
                fighter(skills, profile, profiles)?.listening(subscribe(rotation::wants)),
                skill_interval_ms.map_or(combat::TICK, |ms| {
                    Duration::from_millis(ms).max(combat::MIN_SKILL_INTERVAL)
                }),
                target_priority.as_deref().unwrap_or_default(),
            )
            .await?
        ),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Hunt {
            query,
            prefer_most_matches,
        } => json!(combat::hunt(loader, &query, prefer_most_matches.unwrap_or(false)).await?),
        Request::Exit => json!(combat::exit(loader).await?),
        Request::Target => json!(combat::target(loader)?),
        Request::HasTarget => json!(bridge::combat::has_target(loader)?),
        Request::Attack { query } => json!(combat::attack(loader, counter_attacks, &query)?),
        Request::CanUseSkill { skill } => {
            json!(bridge::combat::can_use_skill(loader, skill.index())?)
        }
        Request::SkillCooldown { skill } => {
            json!(bridge::combat::get_skill_cooldown_remaining(loader, skill.index())?.max(0))
        }
        Request::CancelTarget => json!(bridge::combat::cancel_target(loader)?),
        Request::CancelAutoAttack => json!(bridge::combat::cancel_auto_attack(loader)?),
    })
}

fn fighter(
    skills: Option<Vec<Skill>>,
    profile: Option<ProfileChoice>,
    profiles: &watch::Receiver<Arc<Library>>,
) -> Result<Fighter, ProfileError> {
    Ok(match (skills, profile) {
        (Some(_), Some(_)) => return Err(ProfileError::Conflict),
        (Some(skills), None) => Fighter::fixed(Profile::from_skills(&skills)),
        (None, None) => Fighter::fixed(Profile::generic()),
        (None, Some(ProfileChoice::Inline(profile))) => Fighter::fixed(profile),
        (None, Some(ProfileChoice::Named(name))) => {
            Fighter::named(name.clone(), profiles.clone()).ok_or(ProfileError::Unknown(name))?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    #[tokio::test]
    async fn out_of_game_reads_are_empty_and_actions_do_nothing() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while out of game"),
        });
        let events = Events::default();
        let frogzard = || MonsterQuery::Name("Frogzard".into());
        let skill = || serde_json::from_value::<Skill>(json!(1)).unwrap();
        for (request, expected) in [
            (Request::Target, json!(null)),
            (Request::HasTarget, json!(false)),
            (Request::Attack { query: frogzard() }, json!(false)),
            (
                Request::UseSkill {
                    skill: skill(),
                    target: None,
                    force: None,
                    wait_until_ready: Some(true),
                },
                json!(false),
            ),
            (Request::CanUseSkill { skill: skill() }, json!(false)),
            (Request::SkillCooldown { skill: skill() }, json!(0)),
            (Request::CancelTarget, json!(null)),
            (Request::CancelAutoAttack, json!(null)),
            (Request::Exit, json!(false)),
            (
                Request::Kill {
                    query: frogzard(),
                    skills: None,
                    skill_interval_ms: None,
                    target_priority: None,
                    profile: None,
                },
                json!(false),
            ),
            (
                Request::Hunt {
                    query: frogzard(),
                    prefer_most_matches: Some(true),
                },
                json!(null),
            ),
        ] {
            assert_eq!(
                serve(
                    &loader,
                    |wants| events.subscribe(wants),
                    &CounterAttacks::default(),
                    &watch::channel(Arc::new(Library::default())).1,
                    request.clone()
                )
                .await,
                Ok(expected),
                "{request:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_kill_names_a_saved_profile_or_passes_one_but_not_both() {
        let loader = FakeLoader::new(|callback, _| panic!("{callback} was called"));
        let events = Events::default();
        let kill = |skills: Option<Vec<Skill>>, profile: serde_json::Value| Request::Kill {
            query: MonsterQuery::Name("Frogzard".into()),
            skills,
            skill_interval_ms: None,
            target_priority: None,
            profile: serde_json::from_value(profile).unwrap(),
        };
        for (request, error) in [
            (
                kill(None, json!("Missing")),
                ProfileError::Unknown("Missing".into()),
            ),
            (
                kill(
                    Some(Vec::new()),
                    json!({ "onCooldown": "skip", "steps": [] }),
                ),
                ProfileError::Conflict,
            ),
        ] {
            assert_eq!(
                serve(
                    &loader,
                    |wants| events.subscribe(wants),
                    &CounterAttacks::default(),
                    &watch::channel(Arc::new(Library::default())).1,
                    request
                )
                .await,
                Err(error.into())
            );
        }
    }
}
