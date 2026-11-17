use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::IDLE_LAP_PAUSE;
use crate::domain::rotation::normalized;

const OFFER_TIMEOUT: Duration = Duration::from_secs(3);
const FOCUS_GRACE: Duration = Duration::from_secs(3);
const FOCUS_SLACK: Duration = Duration::from_secs(1);
const MESSAGE_COPIES: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Assignment {
    pub(crate) group: usize,
    pub(crate) mon_map_id: i32,
    pub(crate) players: Vec<usize>,
    pub(crate) on: On,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum On {
    Focus,
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sighting {
    pub(crate) alive: bool,
    pub(crate) focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    Died,
    Respawned,
    FocusAdded(Option<Duration>),
    FocusRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Lease(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) lease: Lease,
    pub(crate) assignment: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Cooling(Duration),
    Passed,
    Landed,
}

pub(crate) struct Orchestrator {
    targets: Vec<Target>,
    here: Vec<bool>,
    ready_at: Vec<Option<Instant>>,
    leases: u64,
}

struct Target {
    group: usize,
    mon_map_id: i32,
    players: Vec<usize>,
    trigger: Trigger,
    life: Life,
    lives: Vec<Life>,
    next: usize,
    tried: Vec<bool>,
    turn: Turn,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Life {
    number: u32,
    over: bool,
}

enum Trigger {
    Focus {
        up: bool,
        casts: u32,
        seen: Vec<u32>,
        landed_at: Option<Instant>,
        ends_at: Option<Instant>,
    },
    Message {
        text: String,
        pending: bool,
        counted_at: Option<Instant>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    Idle,
    Offered {
        lease: Lease,
        position: usize,
        at: Instant,
    },
    Casting {
        lease: Lease,
        position: usize,
    },
    Paused {
        until: Instant,
    },
}

impl Orchestrator {
    pub(crate) fn new(
        assignments: Vec<Assignment>,
        sightings: &[Vec<Sighting>],
        now: Instant,
    ) -> Self {
        let size = sightings.len();
        let targets = assignments
            .into_iter()
            .enumerate()
            .map(|(index, assignment)| {
                let lives: Vec<Life> = sightings
                    .iter()
                    .map(|sighted| Life {
                        number: 0,
                        over: !sighted[index].alive,
                    })
                    .collect();
                let trigger = match assignment.on {
                    On::Focus => Trigger::Focus {
                        up: sightings.iter().any(|sighted| sighted[index].focus),
                        casts: 0,
                        seen: vec![0; size],
                        landed_at: None,
                        ends_at: None,
                    },
                    On::Message(text) => Trigger::Message {
                        text: normalized(&text),
                        pending: false,
                        counted_at: None,
                    },
                };
                Target {
                    group: assignment.group,
                    mon_map_id: assignment.mon_map_id,
                    tried: vec![false; assignment.players.len()],
                    players: assignment.players,
                    trigger,
                    life: lives.iter().copied().max().unwrap_or_default(),
                    lives,
                    next: 0,
                    turn: Turn::Idle,
                }
            })
            .collect();
        let mut orchestrator = Self {
            targets,
            here: vec![true; size],
            ready_at: vec![None; size],
            leases: 0,
        };
        orchestrator.settle(now);
        orchestrator
    }

    pub(crate) fn saw(&mut self, player: usize, mon_map_id: i32, seen: Seen, now: Instant) {
        for target in &mut self.targets {
            if target.mon_map_id == mon_map_id {
                target.saw(player, seen, now);
            }
        }
        self.settle(now);
    }

    pub(crate) fn heard(
        &mut self,
        player: usize,
        mon_map_id: Option<i32>,
        text: &str,
        now: Instant,
    ) {
        let text = normalized(text);
        for target in &mut self.targets {
            if mon_map_id.is_none_or(|id| id == target.mon_map_id)
                && target.lives[player] == target.life
                && let Trigger::Message {
                    text: wanted,
                    pending,
                    counted_at,
                } = &mut target.trigger
                && text.contains(wanted.as_str())
                && counted_at.is_none_or(|at| now >= at + MESSAGE_COPIES)
            {
                *pending = true;
                *counted_at = Some(now);
            }
        }
        self.settle(now);
    }

    pub(crate) fn offer(&self, player: usize) -> Option<Offer> {
        self.targets
            .iter()
            .enumerate()
            .find_map(|(assignment, target)| match target.turn {
                Turn::Offered {
                    lease, position, ..
                } if target.players[position] == player => Some(Offer { lease, assignment }),
                _ => None,
            })
    }

    pub(crate) fn cast(&mut self, player: usize, lease: Lease) -> Option<i32> {
        let (target, position) = self
            .targets
            .iter_mut()
            .find_map(|target| match target.turn {
                Turn::Offered {
                    lease: held,
                    position,
                    ..
                } if held == lease && target.players[position] == player => {
                    Some((target, position))
                }
                _ => None,
            })?;
        target.turn = Turn::Casting { lease, position };
        Some(target.mon_map_id)
    }

    pub(crate) fn answer(&mut self, player: usize, lease: Lease, answer: Answer, now: Instant) {
        let Some((target, position)) = self.targets.iter_mut().find_map(|target| {
            let position = target.position(lease)?;
            (target.players[position] == player).then_some((target, position))
        }) else {
            return;
        };
        match answer {
            Answer::Cooling(wait) => self.ready_at[player] = Some(now + wait),
            Answer::Passed => target.tried[position] = true,
            Answer::Landed => {
                target.next = (position + 1) % target.players.len();
                target.tried.fill(false);
                target.trigger.landed(now);
            }
        }
        target.turn = Turn::Idle;
        self.settle(now);
    }

    pub(crate) fn leave(&mut self, player: usize, now: Instant) {
        self.here[player] = false;
        for target in &mut self.targets {
            if let Some(position) = target.holding()
                && target.players[position] == player
            {
                target.tried[position] = true;
                target.turn = Turn::Idle;
            }
        }
        self.settle(now);
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        self.settle(now);
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.targets
            .iter()
            .flat_map(|target| [target.turn.deadline(), target.trigger.deadline()])
            .flatten()
            .min()
    }

    pub(super) fn schedule(&self) -> (u64, Option<Instant>) {
        (self.leases, self.deadline())
    }

    pub(super) fn deserted(&self) -> bool {
        !self.here.contains(&true)
    }

    fn settle(&mut self, now: Instant) {
        let active = self
            .targets
            .iter()
            .filter(|target| !target.life.over)
            .map(|target| target.group)
            .min();
        for target in &mut self.targets {
            target.trigger.expire(now);
            let live = active == Some(target.group) && !target.life.over;
            if !live {
                target.trigger.absorb();
            }
            let wanted = live && target.trigger.armed();
            match target.turn {
                Turn::Casting { .. } => continue,
                Turn::Idle if !wanted => continue,
                Turn::Offered { .. } | Turn::Paused { .. } if !wanted => {
                    target.turn = Turn::Idle;
                    target.tried.fill(false);
                    continue;
                }
                Turn::Offered { position, at, .. } if now >= at + OFFER_TIMEOUT => {
                    target.tried[position] = true;
                }
                Turn::Offered { .. } => continue,
                Turn::Paused { until } if now < until => continue,
                Turn::Paused { .. } => target.tried.fill(false),
                Turn::Idle => {}
            }
            target.turn = target.next_turn(&self.here, &self.ready_at, &mut self.leases, now);
        }
    }
}

impl Target {
    fn saw(&mut self, player: usize, seen: Seen, now: Instant) {
        match seen {
            Seen::Died => self.lives[player].over = true,
            Seen::Respawned if self.lives[player].over => {
                self.lives[player] = Life {
                    number: self.lives[player].number + 1,
                    over: false,
                };
            }
            Seen::Respawned => {}
            Seen::FocusAdded(_) | Seen::FocusRemoved => {
                if self.lives[player] == self.life {
                    self.trigger.focus(player, seen, now);
                }
                return;
            }
        }
        if self.lives[player] > self.life {
            let renewed = self.lives[player].number > self.life.number;
            self.life = self.lives[player];
            if renewed {
                self.next = 0;
                self.tried.fill(false);
                self.turn = Turn::Idle;
                self.trigger.renew();
            }
        }
    }

    fn position(&self, lease: Lease) -> Option<usize> {
        match self.turn {
            Turn::Offered {
                lease: held,
                position,
                ..
            }
            | Turn::Casting {
                lease: held,
                position,
            } if held == lease => Some(position),
            _ => None,
        }
    }

    fn holding(&self) -> Option<usize> {
        match self.turn {
            Turn::Offered { position, .. } | Turn::Casting { position, .. } => Some(position),
            Turn::Idle | Turn::Paused { .. } => None,
        }
    }

    fn next_turn(
        &self,
        here: &[bool],
        ready_at: &[Option<Instant>],
        leases: &mut u64,
        now: Instant,
    ) -> Turn {
        let mut cooling: Option<Instant> = None;
        let mut anyone = false;
        for step in 0..self.players.len() {
            let position = (self.next + step) % self.players.len();
            let player = self.players[position];
            if !here[player] {
                continue;
            }
            anyone = true;
            if self.tried[position] {
                continue;
            }
            match ready_at[player] {
                Some(at) if at > now => {
                    cooling = Some(cooling.map_or(at, |soonest| soonest.min(at)))
                }
                _ => {
                    *leases += 1;
                    return Turn::Offered {
                        lease: Lease(*leases),
                        position,
                        at: now,
                    };
                }
            }
        }
        match cooling {
            Some(until) => Turn::Paused { until },
            None if anyone => Turn::Paused {
                until: now + IDLE_LAP_PAUSE,
            },
            None => Turn::Idle,
        }
    }
}

impl Turn {
    fn deadline(self) -> Option<Instant> {
        match self {
            Turn::Offered { at, .. } => Some(at + OFFER_TIMEOUT),
            Turn::Paused { until } => Some(until),
            Turn::Idle | Turn::Casting { .. } => None,
        }
    }
}

impl Trigger {
    fn armed(&self) -> bool {
        match self {
            Trigger::Focus { up, .. } => !up,
            Trigger::Message { pending, .. } => *pending,
        }
    }

    fn focus(&mut self, player: usize, seen: Seen, now: Instant) {
        let Trigger::Focus {
            up,
            casts,
            seen: seen_at,
            landed_at,
            ends_at,
        } = self
        else {
            return;
        };
        match seen {
            Seen::FocusAdded(lasts) => {
                seen_at[player] = *casts;
                *up = true;
                *landed_at = None;
                *ends_at = (*ends_at).max(lasts.map(|lasts| now + lasts + FOCUS_SLACK));
            }
            Seen::FocusRemoved if seen_at[player] == *casts => {
                *up = false;
                *ends_at = None;
            }
            _ => {}
        }
    }

    fn landed(&mut self, now: Instant) {
        match self {
            Trigger::Focus {
                up,
                casts,
                landed_at,
                ..
            } => {
                *up = true;
                *casts += 1;
                *landed_at = Some(now);
            }
            Trigger::Message { .. } => self.absorb(),
        }
    }

    fn absorb(&mut self) {
        if let Trigger::Message { pending, .. } = self {
            *pending = false;
        }
    }

    fn renew(&mut self) {
        match self {
            Trigger::Focus {
                up,
                landed_at,
                ends_at,
                ..
            } => {
                *up = false;
                *landed_at = None;
                *ends_at = None;
            }
            Trigger::Message { .. } => self.absorb(),
        }
    }

    fn expire(&mut self, now: Instant) {
        if let Trigger::Focus {
            up,
            landed_at,
            ends_at,
            ..
        } = self
            && (landed_at.is_some_and(|at| now >= at + FOCUS_GRACE)
                || ends_at.is_some_and(|at| now >= at))
        {
            *up = false;
            *landed_at = None;
            *ends_at = None;
        }
    }

    fn deadline(&self) -> Option<Instant> {
        match self {
            Trigger::Focus {
                landed_at, ends_at, ..
            } => [landed_at.map(|at| at + FOCUS_GRACE), *ends_at]
                .into_iter()
                .flatten()
                .min(),
            Trigger::Message { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOSS: i32 = 7;
    const ADD: i32 = 8;
    const NOBODY: [usize; 0] = [];

    fn on(group: usize, mon_map_id: i32, players: &[usize], on: On) -> Assignment {
        Assignment {
            group,
            mon_map_id,
            players: players.to_vec(),
            on,
        }
    }

    fn start(assignments: Vec<Assignment>, size: usize, focus: bool, now: Instant) -> Orchestrator {
        let sighted = vec![Sighting { alive: true, focus }; assignments.len()];
        Orchestrator::new(assignments, &vec![sighted; size], now)
    }

    fn offered(orchestrator: &Orchestrator, size: usize) -> Vec<usize> {
        (0..size)
            .filter(|player| orchestrator.offer(*player).is_some())
            .collect()
    }

    fn land(orchestrator: &mut Orchestrator, player: usize, now: Instant) {
        let lease = orchestrator
            .offer(player)
            .expect("the player was offered a cast")
            .lease;
        assert!(orchestrator.cast(player, lease).is_some());
        orchestrator.answer(player, lease, Answer::Landed, now);
    }

    fn pass(orchestrator: &mut Orchestrator, player: usize, answer: Answer, now: Instant) {
        let lease = orchestrator
            .offer(player)
            .expect("the player was offered a cast")
            .lease;
        orchestrator.answer(player, lease, answer, now);
    }

    #[test]
    fn focus_taunts_go_around_the_players_each_time_focus_wears_off() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[1, 0], On::Focus)], 3, false, now);
        assert_eq!(
            offered(&taunt, 3),
            [1],
            "the first listed player goes first"
        );
        assert_eq!(taunt.cast(1, taunt.offer(1).unwrap().lease), Some(BOSS));
        let lease = taunt.offer(1).map(|offer| offer.lease);
        assert_eq!(lease, None, "a cast in flight is no longer on offer");
        taunt.answer(1, Lease(1), Answer::Landed, now);
        assert_eq!(offered(&taunt, 3), NOBODY);
        for player in 0..3 {
            taunt.saw(player, BOSS, Seen::FocusAdded(None), now);
        }
        assert_eq!(offered(&taunt, 3), NOBODY);
        taunt.saw(2, BOSS, Seen::FocusRemoved, now);
        assert_eq!(offered(&taunt, 3), [0]);
        land(&mut taunt, 0, now);
        taunt.saw(0, BOSS, Seen::FocusAdded(None), now);
        taunt.saw(0, BOSS, Seen::FocusRemoved, now);
        assert_eq!(offered(&taunt, 3), [1]);
    }

    #[test]
    fn a_removal_from_before_the_latest_cast_does_not_trigger_another() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, true, now);
        assert_eq!(offered(&taunt, 2), NOBODY, "focus is already up");
        taunt.saw(0, BOSS, Seen::FocusRemoved, now);
        land(&mut taunt, 0, now);
        taunt.saw(1, BOSS, Seen::FocusRemoved, now);
        assert_eq!(
            offered(&taunt, 2),
            NOBODY,
            "player 2 was still catching up on the old focus"
        );
        taunt.saw(1, BOSS, Seen::FocusAdded(None), now);
        taunt.saw(1, BOSS, Seen::FocusRemoved, now);
        assert_eq!(offered(&taunt, 2), [1]);
    }

    #[test]
    fn a_landed_cast_that_nobody_sees_focus_from_tries_again_after_a_grace_period() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        land(&mut taunt, 0, now);
        assert_eq!(taunt.deadline(), Some(now + FOCUS_GRACE));
        taunt.tick(now + FOCUS_GRACE - Duration::from_millis(1));
        assert_eq!(offered(&taunt, 2), NOBODY);
        taunt.tick(now + FOCUS_GRACE);
        assert_eq!(offered(&taunt, 2), [1]);
    }

    #[test]
    fn focus_that_disappears_without_a_removal_ends_when_its_duration_runs_out() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        land(&mut taunt, 0, now);
        let lasts = Duration::from_secs(6);
        taunt.saw(1, BOSS, Seen::FocusAdded(Some(lasts)), now);
        taunt.tick(now + FOCUS_GRACE);
        assert_eq!(
            offered(&taunt, 2),
            NOBODY,
            "the focus was seen, so it is still up"
        );
        assert_eq!(taunt.deadline(), Some(now + lasts + FOCUS_SLACK));
        taunt.tick(now + lasts + FOCUS_SLACK - Duration::from_millis(1));
        assert_eq!(offered(&taunt, 2), NOBODY);
        taunt.tick(now + lasts + FOCUS_SLACK);
        assert_eq!(offered(&taunt, 2), [1]);
    }

    #[test]
    fn players_that_cannot_cast_pass_and_cooling_players_are_waited_for() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1, 2], On::Focus)], 3, false, now);
        pass(&mut taunt, 0, Answer::Passed, now);
        pass(&mut taunt, 1, Answer::Cooling(Duration::from_secs(2)), now);
        pass(&mut taunt, 2, Answer::Passed, now);
        assert_eq!(offered(&taunt, 3), NOBODY);
        assert_eq!(taunt.deadline(), Some(now + Duration::from_secs(2)));
        taunt.tick(now + Duration::from_secs(2));
        assert_eq!(
            offered(&taunt, 3),
            [0],
            "the lap starts over once the cooldown ends"
        );
    }

    #[test]
    fn a_lap_where_nobody_can_cast_pauses_before_the_next() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        pass(&mut taunt, 0, Answer::Passed, now);
        let lease = taunt.offer(1).unwrap().lease;
        assert_eq!(taunt.cast(1, lease), Some(BOSS));
        taunt.answer(1, lease, Answer::Passed, now);
        assert_eq!(offered(&taunt, 2), NOBODY);
        taunt.tick(now + IDLE_LAP_PAUSE - Duration::from_millis(1));
        assert_eq!(offered(&taunt, 2), NOBODY);
        taunt.tick(now + IDLE_LAP_PAUSE);
        assert_eq!(offered(&taunt, 2), [0]);
    }

    #[test]
    fn an_unanswered_offer_moves_to_the_next_player() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        let stale = taunt.offer(0).unwrap().lease;
        taunt.tick(now + OFFER_TIMEOUT);
        assert_eq!(offered(&taunt, 2), [1]);
        assert_eq!(taunt.cast(0, stale), None);
        taunt.answer(0, stale, Answer::Landed, now + OFFER_TIMEOUT);
        assert_eq!(
            offered(&taunt, 2),
            [1],
            "an answer to an old offer changes nothing"
        );
    }

    #[test]
    fn an_offer_is_withdrawn_when_someone_else_taunts_first() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        let lease = taunt.offer(0).unwrap().lease;
        taunt.saw(1, BOSS, Seen::FocusAdded(None), now);
        assert_eq!(taunt.cast(0, lease), None);
        assert_eq!(offered(&taunt, 2), NOBODY);
    }

    #[test]
    fn a_leaving_player_passes_the_turn() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        taunt.leave(0, now);
        assert_eq!(offered(&taunt, 2), [1]);
        assert!(!taunt.deserted());
        taunt.leave(1, now);
        assert!(taunt.deserted());
    }

    #[test]
    fn one_cast_answers_a_message_every_player_heard() {
        let now = Instant::now();
        let message = On::Message("Prepares a  HEAVY attack".into());
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], message)], 3, false, now);
        assert_eq!(offered(&taunt, 3), NOBODY);
        taunt.heard(2, Some(ADD), "The boss prepares a heavy attack!", now);
        taunt.heard(2, None, "The boss gathers energy", now);
        assert_eq!(offered(&taunt, 3), NOBODY);
        taunt.heard(2, Some(BOSS), "The boss prepares   a heavy attack!", now);
        taunt.heard(0, None, "the boss prepares a heavy attack!", now);
        assert_eq!(offered(&taunt, 3), [0]);
        land(&mut taunt, 0, now);
        let late = now + MESSAGE_COPIES - Duration::from_millis(1);
        taunt.heard(1, None, "The boss prepares a heavy attack!", late);
        assert_eq!(
            offered(&taunt, 3),
            NOBODY,
            "player 2 heard the message late"
        );
        let next = now + MESSAGE_COPIES;
        taunt.heard(0, None, "The boss prepares a heavy attack!", next);
        assert_eq!(offered(&taunt, 3), [1]);
        land(&mut taunt, 1, next);
        taunt.leave(0, next);
        taunt.heard(1, None, "The boss prepares a heavy attack!", next);
        assert_eq!(
            offered(&taunt, 3),
            NOBODY,
            "player 2's copy of a message that player 1 answered before leaving"
        );
        taunt.heard(
            1,
            None,
            "The boss prepares a heavy attack!",
            next + MESSAGE_COPIES,
        );
        assert_eq!(offered(&taunt, 3), [1]);
    }

    #[test]
    fn lower_groups_wait_for_higher_ones_and_a_respawn_takes_back_over() {
        let now = Instant::now();
        let mut taunt = start(
            vec![
                on(0, BOSS, &[0, 1], On::Focus),
                on(1, ADD, &[2, 0], On::Focus),
            ],
            3,
            false,
            now,
        );
        assert_eq!(offered(&taunt, 3), [0]);
        land(&mut taunt, 0, now);
        taunt.saw(1, BOSS, Seen::Died, now);
        assert_eq!(offered(&taunt, 3), [2]);
        land(&mut taunt, 2, now);
        taunt.saw(2, ADD, Seen::FocusAdded(None), now);
        taunt.saw(2, ADD, Seen::FocusRemoved, now);
        assert_eq!(offered(&taunt, 3), [0]);
        taunt.saw(1, BOSS, Seen::Respawned, now);
        assert_eq!(
            offered(&taunt, 3),
            [0],
            "the boss is back, and its rotation starts over with player 1"
        );
        assert_eq!(taunt.offer(0).unwrap().assignment, 0);
    }

    #[test]
    fn a_late_death_report_does_not_end_the_next_life() {
        let now = Instant::now();
        let mut taunt = start(vec![on(0, BOSS, &[0, 1], On::Focus)], 2, false, now);
        land(&mut taunt, 0, now);
        taunt.saw(0, BOSS, Seen::Died, now);
        taunt.saw(0, BOSS, Seen::Respawned, now);
        assert_eq!(offered(&taunt, 2), [0], "a new life starts with player 1");
        taunt.saw(1, BOSS, Seen::Died, now);
        taunt.saw(1, BOSS, Seen::FocusAdded(None), now);
        assert_eq!(offered(&taunt, 2), [0]);
    }
}
