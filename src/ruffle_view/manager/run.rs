use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Instant,
};

use gpui::Global;
use mira_ui::Tone;

use super::super::account_card::{Presence, Status};
use crate::{accounts::AccountId, setups::Setup};

#[derive(Default)]
pub(super) struct Runs(HashMap<String, Instant>);

impl Global for Runs {}

impl Runs {
    pub(super) fn started(&self, setup: &str) -> Option<Instant> {
        self.0.get(setup).copied()
    }

    pub(super) fn start(&mut self, setup: &str, now: Instant) {
        self.0.insert(setup.to_owned(), now);
    }

    pub(super) fn rename(&mut self, old: &str, name: &str) {
        if let Some(started) = self.0.remove(old) {
            self.0.insert(name.to_owned(), started);
        }
    }

    pub(super) fn forget(&mut self, setup: &str) {
        self.0.remove(setup);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Start {
    Continue,
    Fresh,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub(super) start: Vec<AccountId>,
    pub(super) conflicts: Vec<AccountId>,
    pub(super) login: Vec<AccountId>,
}

pub(super) fn plan(
    setup: &Setup,
    presences: &HashMap<AccountId, Presence>,
    done: &HashSet<AccountId>,
    start: Start,
) -> Plan {
    let mut plan = Plan::default();
    let Some(script) = setup.script.as_deref() else {
        return plan;
    };
    for member in &setup.members {
        let id = member.id();
        let Some(presence) = presences.get(&id) else {
            continue;
        };
        if start == Start::Continue && done.contains(&id) {
            continue;
        }
        match presence.running() {
            Some(running) if running == script => {
                if presence.signed_out() {
                    plan.login.push(id);
                }
            }
            Some(_) => plan.conflicts.push(id),
            None => plan.start.push(id),
        }
    }
    plan
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Filter {
    Done,
    Running,
    Starting,
    Needs,
    Rest,
    NotOpen,
}

pub(super) const RUN_FILTERS: [Filter; 5] = [
    Filter::Done,
    Filter::Running,
    Filter::Starting,
    Filter::Needs,
    Filter::Rest,
];

pub(super) const ROSTER_FILTERS: [Filter; 4] = [
    Filter::Running,
    Filter::Starting,
    Filter::Needs,
    Filter::NotOpen,
];

impl Filter {
    pub(super) fn admits(self, status: Status) -> bool {
        match self {
            Filter::Done => status == Status::Done,
            Filter::Running => status == Status::Running,
            Filter::Starting => status == Status::Starting,
            Filter::Needs => status.needs_you(),
            Filter::Rest => matches!(status, Status::Idle | Status::NotOpen),
            Filter::NotOpen => status == Status::NotOpen,
        }
    }

    pub(super) fn label(self, count: usize, total: usize) -> String {
        match self {
            Filter::Done if count == total => format!("All {total} done"),
            Filter::Done => format!("{count} of {total} done"),
            Filter::Running => format!("{count} running"),
            Filter::Starting => format!("{count} starting"),
            Filter::Needs if count == 1 => "1 needs you".to_owned(),
            Filter::Needs => format!("{count} need you"),
            Filter::Rest => format!("{count} not started"),
            Filter::NotOpen => format!("{count} not open"),
        }
    }

    pub(super) fn short(self, count: usize, total: usize) -> String {
        match self {
            Filter::Done => format!("{count}/{total}"),
            _ => count.to_string(),
        }
    }

    pub(super) fn tone(self, failed: bool) -> Option<Tone> {
        match self {
            Filter::Done => Some(Tone::Success),
            Filter::Running | Filter::Starting => Some(Tone::Neutral),
            Filter::Needs if failed => Some(Tone::Danger),
            Filter::Needs => Some(Tone::Attention),
            Filter::Rest | Filter::NotOpen => None,
        }
    }
}

pub(super) fn count(statuses: &[Status], filter: Filter) -> usize {
    statuses
        .iter()
        .filter(|status| filter.admits(**status))
        .count()
}

pub(super) fn any_failed(statuses: &[Status]) -> bool {
    statuses.contains(&Status::Failed)
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Stoppable {
    pub(super) running: Vec<AccountId>,
    pub(super) waiting: Vec<AccountId>,
}

impl Stoppable {
    pub(super) fn is_empty(&self) -> bool {
        self.running.is_empty() && self.waiting.is_empty()
    }

    pub(super) fn all(&self) -> Vec<AccountId> {
        self.running.iter().chain(&self.waiting).cloned().collect()
    }
}

pub(super) fn stoppable<'a>(
    presences: impl IntoIterator<Item = (AccountId, &'a Presence)>,
    script: Option<&Path>,
) -> Stoppable {
    let mut found = Stoppable::default();
    for (id, presence) in presences {
        let matches = |entry: &Path| script.is_none_or(|script| script == entry);
        if presence.running().is_some_and(matches) {
            found.running.push(id);
        } else if presence.queued().is_some_and(matches) {
            found.waiting.push(id);
        }
    }
    found
}

pub(super) fn setup_work(setup: &Setup, presences: &HashMap<AccountId, Presence>) -> Stoppable {
    let Some(script) = setup.script.as_deref() else {
        return Stoppable::default();
    };
    stoppable(
        setup
            .members
            .iter()
            .filter_map(|member| Some((member.id(), presences.get(&member.id())?))),
        Some(script),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ruffle_view::{
            Login, ScriptPhase,
            account_card::tests::{online, queued, signing_in},
        },
        setups::Member,
    };

    fn id(username: &str) -> AccountId {
        AccountId::of(username)
    }

    fn setup(members: &[&str]) -> Setup {
        Setup::new(
            "Daily".into(),
            Some("/s/daily.ts".into()),
            members
                .iter()
                .map(|username| Member {
                    username: (*username).into(),
                })
                .collect(),
        )
    }

    #[test]
    fn continue_skips_accounts_done_this_run_and_a_fresh_start_includes_them() {
        let setup = setup(&["main", "alt1", "alt2", "gone"]);
        let presences = HashMap::from([
            (id("main"), online("/s/daily.ts", ScriptPhase::Finished)),
            (id("alt1"), Presence::Closed(None)),
            (id("alt2"), online("/s/daily.ts", ScriptPhase::Stopped)),
        ]);
        let done = HashSet::from([id("main")]);
        assert_eq!(
            plan(&setup, &presences, &done, Start::Continue).start,
            [id("alt1"), id("alt2")]
        );
        assert_eq!(
            plan(&setup, &presences, &done, Start::Fresh).start,
            [id("main"), id("alt1"), id("alt2")]
        );
    }

    #[test]
    fn starting_leaves_the_same_script_alone_and_asks_about_another() {
        let setup = setup(&["main", "alt1", "alt2", "alt3"]);
        let Presence::Open(mut dropped) = online("/s/daily.ts", ScriptPhase::Running) else {
            unreachable!()
        };
        dropped.login = Login::Disconnected;
        let presences = HashMap::from([
            (id("main"), online("/s/daily.ts", ScriptPhase::Running)),
            (id("alt1"), online("/s/hunt.ts", ScriptPhase::NeedsInput)),
            (
                id("alt2"),
                Presence::Waiting {
                    place: 1,
                    script: None,
                    open: Some(dropped),
                },
            ),
            (id("alt3"), online("/s/hunt.ts", ScriptPhase::Stopping)),
        ]);
        assert_eq!(
            plan(&setup, &presences, &HashSet::new(), Start::Fresh),
            Plan {
                start: vec![id("alt3")],
                conflicts: vec![id("alt1")],
                login: vec![id("alt2")],
            }
        );
    }

    #[test]
    fn stopping_a_setup_targets_its_own_script_including_ones_still_waiting_to_start() {
        let waiting = Presence::Waiting {
            place: 1,
            script: Some(queued("/s/daily.ts")),
            open: None,
        };
        let queued = online("/s/daily.ts", ScriptPhase::Queued);
        let other = online("/s/hunt.ts", ScriptPhase::Running);
        let running = online("/s/daily.ts", ScriptPhase::Running);
        let idle = signing_in(Login::Online);
        let presences = HashMap::from([
            (id("main"), running),
            (id("alt1"), waiting),
            (id("alt2"), queued),
            (id("alt3"), other),
            (id("alt4"), idle),
        ]);
        let mut daily = setup(&["main", "alt1", "alt2", "alt3", "alt4"]);
        assert_eq!(
            setup_work(&daily, &presences),
            Stoppable {
                running: vec![id("main")],
                waiting: vec![id("alt1"), id("alt2")],
            }
        );
        daily.script = None;
        assert!(setup_work(&daily, &presences).is_empty());
        let all = ["main", "alt1", "alt2", "alt3", "alt4"]
            .map(|username| (id(username), &presences[&id(username)]));
        assert_eq!(stoppable(all, None).running, [id("main"), id("alt3")]);
    }

    #[test]
    fn filters_count_the_statuses_they_name() {
        let statuses = [
            Status::Done,
            Status::Running,
            Status::NeedsYou,
            Status::Failed,
            Status::Idle,
            Status::NotOpen,
        ];
        let counts: Vec<_> = RUN_FILTERS
            .iter()
            .map(|filter| count(&statuses, *filter))
            .collect();
        assert_eq!(counts, [1, 1, 0, 2, 2]);
        assert_eq!(count(&statuses, Filter::NotOpen), 1);
        assert_eq!(Filter::Done.label(6, 6), "All 6 done");
        assert_eq!(Filter::Needs.label(1, 6), "1 needs you");
        assert_eq!(
            Filter::Needs.tone(any_failed(&statuses)),
            Some(Tone::Danger)
        );
    }
}
