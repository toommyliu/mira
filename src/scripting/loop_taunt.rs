use std::{future::pending, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{Instant, sleep_until};

use crate::{
    army::{
        Answer, ArmyError, Assignment, Lease, Member, Offer, On, Orchestrator, Seen, Sighting,
        Taunter,
    },
    bridge::{CallError, Drained, Event, Loader, Packets, Subscription},
    domain::{
        events::{GameEvent, Kind, Me, TargetType},
        taunt::{self, Readiness, Sighted},
        world::MonsterQuery,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Target {
    target: MonsterQuery,
    players: Vec<usize>,
    on: Trigger,
    skips: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Trigger {
    Focus,
    Message { includes: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Answered {
    lease: Lease,
    pub(super) cast: bool,
}

pub(crate) struct Taunting {
    taunter: Taunter,
    monsters: Vec<i32>,
    skips: Vec<bool>,
    events: Subscription,
}

const WATCHED: [Kind; 7] = [
    Kind::MonsterDeath,
    Kind::MonsterRespawn,
    Kind::AuraAdded,
    Kind::AuraRemoved,
    Kind::Message,
    Kind::MapJoined,
    Kind::Logout,
];

fn watched(event: &Event) -> bool {
    WATCHED.iter().any(|kind| kind.wants(event))
}

struct Anyone;

impl Me for Anyone {
    fn username(&self) -> Option<String> {
        None
    }

    fn user_id(&self) -> Option<i32> {
        None
    }
}

pub(super) async fn join(
    member: &Member,
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    plan: Vec<Vec<Target>>,
    timeout: Option<Duration>,
) -> Result<Taunting, CallError> {
    check(&plan, member.size())?;
    member.sync("loopTaunt", timeout).await?;
    let events = subscribe(watched);
    let targets: Vec<&Target> = plan.iter().flatten().collect();
    let sighted = targets
        .iter()
        .map(|target| taunt::sight(loader, &target.target))
        .collect::<Result<Vec<_>, _>>()?;
    let wire = serde_json::to_value(&plan).expect("a plan is plain data");
    let sighting = serde_json::to_value(&sighted).expect("sightings are plain data");
    let mut monsters = Vec::new();
    let taunter = member
        .loop_taunt(wire, sighting, timeout, |sightings| {
            let (assignments, sightings) = agree(&plan, sightings)?;
            monsters = assignments
                .iter()
                .map(|assignment| assignment.mon_map_id)
                .collect();
            Ok(Orchestrator::new(assignments, &sightings, Instant::now()))
        })
        .await?;
    Ok(Taunting {
        taunter,
        monsters,
        skips: targets.iter().map(|target| target.skips).collect(),
        events,
    })
}

fn check(plan: &[Vec<Target>], size: usize) -> Result<(), ArmyError> {
    if plan.is_empty() || plan.iter().any(Vec::is_empty) {
        return Err(refuse(
            "loopTaunt needs at least one group, and a target in each group",
        ));
    }
    for (group, targets) in plan.iter().enumerate() {
        let mut taken = vec![false; size];
        for target in targets {
            let players = &target.players;
            let valid = !players.is_empty()
                && players.iter().all(|number| (1..=size).contains(number))
                && players
                    .iter()
                    .enumerate()
                    .all(|(position, number)| !players[..position].contains(number));
            if !valid {
                return Err(refuse(&format!(
                    "loopTaunt needs different player numbers from 1 to {size} for each target"
                )));
            }
            if let Some(number) = players.iter().find(|number| taken[*number - 1]) {
                return Err(refuse(&format!(
                    "Player {number} has more than one target in loopTaunt group {}.",
                    group + 1
                )));
            }
            for number in players {
                taken[number - 1] = true;
            }
            if let Trigger::Message { includes } = &target.on
                && includes.trim().is_empty()
            {
                return Err(refuse(
                    "loopTaunt needs text to look for in each message trigger",
                ));
            }
        }
    }
    Ok(())
}

fn agree(
    plan: &[Vec<Target>],
    sightings: Vec<Value>,
) -> Result<(Vec<Assignment>, Vec<Vec<Sighting>>), ArmyError> {
    let sightings: Vec<Vec<Option<Sighted>>> = sightings
        .into_iter()
        .map(|sighting| serde_json::from_value(sighting).expect("players send their own sightings"))
        .collect();
    let mut assignments: Vec<Assignment> = Vec::new();
    let targets = plan
        .iter()
        .enumerate()
        .flat_map(|(group, targets)| targets.iter().map(move |target| (group, target)));
    for (index, (group, target)) in targets.enumerate() {
        let name = describe(&target.target);
        let mut found = None;
        for (player, sighted) in sightings.iter().enumerate() {
            let Some(sighted) = sighted[index] else {
                return Err(refuse(&format!(
                    "Player {} can't see {name} in their cell.",
                    player + 1
                )));
            };
            match found {
                None => found = Some(sighted.mon_map_id),
                Some(first) if first != sighted.mon_map_id => {
                    return Err(refuse(&format!(
                        "Players 1 and {} found different monsters for {name}.",
                        player + 1
                    )));
                }
                Some(_) => {}
            }
        }
        let mon_map_id = found.expect("an army has at least one player");
        if let Some(other) = assignments
            .iter()
            .position(|assignment| assignment.mon_map_id == mon_map_id)
        {
            return Err(refuse(&format!(
                "loopTaunt needs a different monster for each target, but {name} and {} are the same one.",
                describe(
                    &plan
                        .iter()
                        .flatten()
                        .nth(other)
                        .expect("assignments follow the plan")
                        .target
                )
            )));
        }
        assignments.push(Assignment {
            group,
            mon_map_id,
            players: target.players.iter().map(|number| number - 1).collect(),
            on: match &target.on {
                Trigger::Focus => On::Focus,
                Trigger::Message { includes } => On::Message(includes.clone()),
            },
        });
    }
    let sightings = sightings
        .into_iter()
        .map(|sighted| {
            sighted
                .into_iter()
                .flatten()
                .map(|sighted| Sighting {
                    alive: sighted.alive,
                    focus: sighted.focus,
                })
                .collect()
        })
        .collect();
    Ok((assignments, sightings))
}

fn describe(query: &MonsterQuery) -> String {
    match query {
        MonsterQuery::Name(name) => format!("\"{}\"", name.trim()),
        MonsterQuery::MonMapId { mon_map_id } => format!("monster {mon_map_id}"),
    }
}

fn refuse(message: &str) -> ArmyError {
    ArmyError::from(message.to_owned())
}

impl Taunting {
    pub(super) async fn turn(
        &self,
        loader: &impl Loader,
        subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
        mut answered: Option<Answered>,
    ) -> Result<Option<Offer>, CallError> {
        let player = self.taunter.player();
        let mut changes = self.taunter.changes();
        loop {
            if let Some(drained) = self.events.try_drain()
                && !self.observe(loader, drained)?
            {
                return Ok(None);
            }
            if let Some(Answered { lease, cast }) = answered.take() {
                if cast {
                    self.cast(loader, subscribe, lease).await?;
                } else {
                    self.answer(lease, Answer::Passed)?;
                }
                continue;
            }
            let now = Instant::now();
            let (offer, deadline) = self.taunter.with(|taunt| {
                taunt.tick(now);
                (taunt.offer(player), taunt.deadline())
            })?;
            if let Some(offer) = offer {
                match taunt::readiness(loader)? {
                    Readiness::Ready if self.skips[offer.assignment] => return Ok(Some(offer)),
                    Readiness::Ready => self.cast(loader, subscribe, offer.lease).await?,
                    Readiness::Cooling(wait) => self.answer(offer.lease, Answer::Cooling(wait))?,
                    Readiness::Unable => self.answer(offer.lease, Answer::Passed)?,
                }
                continue;
            }
            let deadline = async {
                match deadline {
                    Some(at) => sleep_until(at).await,
                    None => pending().await,
                }
            };
            tokio::select! {
                drained = self.events.drain() => {
                    if !self.observe(loader, drained)? {
                        return Ok(None);
                    }
                }
                _ = changes.changed() => {}
                () = deadline => {}
            }
        }
    }

    async fn cast(
        &self,
        loader: &impl Loader,
        subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
        lease: Lease,
    ) -> Result<(), CallError> {
        let player = self.taunter.player();
        let Some(mon_map_id) = self.taunter.with(|taunt| taunt.cast(player, lease))? else {
            return Ok(());
        };
        let landed = taunt::cast(loader, Packets::subscribe(subscribe), mon_map_id).await;
        let answer = if matches!(landed, Ok(true)) {
            Answer::Landed
        } else {
            Answer::Passed
        };
        self.answer(lease, answer)?;
        landed.map(drop)
    }

    fn answer(&self, lease: Lease, answer: Answer) -> Result<(), CallError> {
        let player = self.taunter.player();
        let now = Instant::now();
        Ok(self
            .taunter
            .with(|taunt| taunt.answer(player, lease, answer, now))?)
    }

    fn observe(&self, loader: &impl Loader, drained: Drained) -> Result<bool, CallError> {
        let mut seen = Vec::new();
        for event in &drained.events {
            for kind in WATCHED.iter().filter(|kind| kind.wants(event)) {
                kind.read(event, &Anyone, &mut seen);
            }
        }
        if seen
            .iter()
            .any(|event| matches!(event, GameEvent::MapJoined { .. } | GameEvent::Logout))
        {
            return Ok(false);
        }
        let resighted = if drained.lagged {
            self.monsters
                .iter()
                .map(|id| {
                    taunt::sight(
                        loader,
                        &MonsterQuery::MonMapId {
                            mon_map_id: f64::from(*id),
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let player = self.taunter.player();
        let now = Instant::now();
        self.taunter.with(|taunt| {
            for event in seen {
                match event {
                    GameEvent::MonsterDeath { mon_map_id } => {
                        taunt.saw(player, mon_map_id, Seen::Died, now);
                    }
                    GameEvent::MonsterRespawn { mon_map_id } => {
                        taunt.saw(player, mon_map_id, Seen::Respawned, now);
                    }
                    GameEvent::AuraAdded {
                        name,
                        target_type: TargetType::Monster,
                        target_id,
                        duration,
                        icon,
                        ..
                    } if taunt::is_focus(&name, icon.as_deref()) => {
                        let lasts =
                            duration.and_then(|secs| Duration::try_from_secs_f64(secs).ok());
                        taunt.saw(player, target_id, Seen::FocusAdded(lasts), now);
                    }
                    GameEvent::AuraRemoved {
                        name,
                        target_type: TargetType::Monster,
                        target_id,
                        ..
                    } if taunt::is_focus(&name, None) => {
                        taunt.saw(player, target_id, Seen::FocusRemoved, now);
                    }
                    GameEvent::Message { text, mon_map_id } => {
                        taunt.heard(player, mon_map_id, &text, now);
                    }
                    _ => {}
                }
            }
            for sighted in resighted.into_iter().flatten() {
                let life = if sighted.alive {
                    Seen::Respawned
                } else {
                    Seen::Died
                };
                let focus = if sighted.focus {
                    Seen::FocusAdded(None)
                } else {
                    Seen::FocusRemoved
                };
                taunt.saw(player, sighted.mon_map_id, life, now);
                taunt.saw(player, sighted.mon_map_id, focus, now);
            }
        })?;
        Ok(true)
    }
}
