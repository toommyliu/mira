use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::accounts::AccountId;

const GAP: Duration = Duration::from_secs(1);
const JITTER: Duration = Duration::from_millis(250);

pub(super) struct Launch {
    pub(super) account: AccountId,
    pub(super) script: Option<PathBuf>,
    pub(super) replace: bool,
    pub(super) server: Option<String>,
}

#[derive(Default)]
pub(super) struct Launches {
    waiting: VecDeque<Launch>,
    next: Option<Instant>,
}

impl Launches {
    pub(super) fn add(&mut self, launch: Launch) {
        let Some(waiting) = self
            .waiting
            .iter_mut()
            .find(|waiting| waiting.account == launch.account)
        else {
            self.waiting.push_back(launch);
            return;
        };
        if launch.script.is_some() && launch.script == waiting.script {
            waiting.replace |= launch.replace;
        } else if launch.script.is_some() {
            waiting.script = launch.script;
            waiting.replace = launch.replace;
        }
        if launch.server.is_some() {
            waiting.server = launch.server;
        }
    }

    pub(super) fn remove(&mut self, account: &AccountId) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|launch| &launch.account != account);
        self.waiting.len() != before
    }

    pub(super) fn position(&self, account: &AccountId) -> Option<(usize, usize)> {
        let index = self
            .waiting
            .iter()
            .position(|launch| &launch.account == account)?;
        Some((index + 1, self.waiting.len()))
    }

    pub(super) fn wait(&self, now: Instant) -> Option<Duration> {
        if self.waiting.is_empty() {
            return None;
        }
        Some(
            self.next
                .map_or(Duration::ZERO, |next| next.saturating_duration_since(now)),
        )
    }

    pub(super) fn take_due(&mut self, now: Instant, jitter: Duration) -> Option<Launch> {
        if self.next.is_some_and(|next| now < next) {
            return None;
        }
        let launch = self.waiting.pop_front()?;
        self.next = Some(now + GAP + jitter);
        Some(launch)
    }
}

pub(super) fn jitter() -> Duration {
    crate::jitter::up_to(JITTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> AccountId {
        AccountId::of(name)
    }

    fn names(launches: &mut Launches, at: Instant) -> Option<String> {
        launches
            .take_due(at, Duration::ZERO)
            .map(|launch| launch.account.as_str().to_owned())
    }

    fn queued(name: &str, script: Option<PathBuf>, replace: bool) -> Launch {
        Launch {
            account: id(name),
            script,
            replace,
            server: None,
        }
    }

    #[test]
    fn launches_go_one_at_a_time_in_order() {
        let start = Instant::now();
        let mut launches = Launches::default();
        for name in ["main", "alt1", "alt2"] {
            launches.add(queued(name, None, false));
        }
        assert_eq!(names(&mut launches, start).as_deref(), Some("main"));
        assert_eq!(names(&mut launches, start + GAP / 2), None);
        assert_eq!(launches.wait(start + GAP / 2), Some(GAP / 2));
        assert_eq!(names(&mut launches, start + GAP).as_deref(), Some("alt1"));
        assert_eq!(
            names(&mut launches, start + GAP * 2).as_deref(),
            Some("alt2")
        );
        assert_eq!(launches.wait(start + GAP * 2), None);
    }

    #[test]
    fn jitter_pushes_the_next_launch_back() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.add(queued("main", None, false));
        launches.add(queued("alt1", None, false));
        let jitter = Duration::from_millis(700);
        assert!(launches.take_due(start, jitter).is_some());
        assert!(launches.take_due(start + GAP, Duration::ZERO).is_none());
        assert!(
            launches
                .take_due(start + GAP + jitter, Duration::ZERO)
                .is_some()
        );
    }

    #[test]
    fn a_second_launch_press_keeps_the_spacing() {
        let start = Instant::now();
        let mut launches = Launches::default();
        launches.add(queued("main", None, false));
        assert!(names(&mut launches, start).is_some());
        launches.add(queued("alt1", None, false));
        assert_eq!(names(&mut launches, start + GAP / 4), None);
        assert_eq!(names(&mut launches, start + GAP).as_deref(), Some("alt1"));
    }

    #[test]
    fn launching_a_waiting_account_again_keeps_its_place_and_script() {
        let mut launches = Launches::default();
        launches.add(queued("main", None, false));
        launches.add(queued("alt1", Some("daily.ts".into()), false));
        launches.add(queued("alt1", None, false));
        launches.add(queued("main", Some("hunt.ts".into()), true));
        launches.add(Launch {
            server: Some("Twig".into()),
            ..queued("main", None, false)
        });
        assert_eq!(launches.position(&id("main")), Some((1, 2)));
        assert_eq!(launches.position(&id("alt1")), Some((2, 2)));
        let start = Instant::now();
        let first = launches.take_due(start, Duration::ZERO).unwrap();
        assert_eq!(first.script, Some("hunt.ts".into()));
        assert!(first.replace);
        assert_eq!(first.server.as_deref(), Some("Twig"));
        let second = launches.take_due(start + GAP, Duration::ZERO).unwrap();
        assert_eq!(second.script, Some("daily.ts".into()));
    }

    #[test]
    fn asking_for_the_same_script_again_keeps_a_pending_replace() {
        let mut launches = Launches::default();
        launches.add(queued("main", Some("b.ts".into()), true));
        launches.add(queued("main", Some("b.ts".into()), false));
        launches.add(queued("alt1", Some("a.ts".into()), true));
        launches.add(queued("alt1", Some("b.ts".into()), false));
        let start = Instant::now();
        let first = launches.take_due(start, Duration::ZERO).unwrap();
        assert!(first.replace);
        let second = launches.take_due(start + GAP, Duration::ZERO).unwrap();
        assert_eq!(second.script, Some("b.ts".into()));
        assert!(!second.replace);
    }

    #[test]
    fn removing_a_waiting_account_moves_the_rest_up() {
        let mut launches = Launches::default();
        for name in ["main", "alt1", "alt2"] {
            launches.add(queued(name, None, false));
        }
        assert!(launches.remove(&id("alt1")));
        assert!(!launches.remove(&id("alt1")));
        assert_eq!(launches.position(&id("alt1")), None);
        assert_eq!(launches.position(&id("alt2")), Some((2, 2)));
    }
}
