mod config;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;
use tokio::{sync::watch, time::Instant};

use crate::scripting::RunId;
use config::Config;
pub(crate) use config::Gear;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub(crate) struct ArmyError(String);

impl From<String> for ArmyError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<config::ConfigError> for ArmyError {
    fn from(error: config::ConfigError) -> Self {
        Self(error.to_string())
    }
}

#[derive(Clone)]
pub(crate) struct Armies(Arc<Registry>);

struct Registry {
    folder: PathBuf,
    armies: Mutex<HashMap<String, Army>>,
}

impl Armies {
    pub(crate) fn new(folder: PathBuf) -> Self {
        Self(Arc::new(Registry {
            folder,
            armies: Mutex::default(),
        }))
    }

    pub(crate) async fn start(
        &self,
        name: &str,
        run: RunId,
        username: &str,
        timeout: Option<Duration>,
    ) -> Result<Option<Member>, ArmyError> {
        config::check_name(name)?;
        let key = name.to_lowercase();
        let seated = match self.seat(&key, run, username, None)? {
            Some(seated) => seated,
            None => {
                let folder = self.0.folder.clone();
                let owned = name.to_owned();
                let config = tokio::task::spawn_blocking(move || Config::load(&folder, &owned))
                    .await
                    .map_err(|_| ArmyError(format!("couldn't read {name}.yaml")))??;
                self.seat(&key, run, username, Some((name, config)))?
                    .expect("a fresh army seats its player")
            }
        };
        Seating {
            army: seated.0,
            player: seated.1,
            run,
            armed: true,
        }
        .wait(timeout)
        .await
    }

    fn seat(
        &self,
        key: &str,
        run: RunId,
        username: &str,
        fresh: Option<(&str, Config)>,
    ) -> Result<Option<(Army, usize)>, ArmyError> {
        let mut armies = self.0.armies.lock().unwrap();
        if let Some(army) = armies.get(key) {
            match army.phase() {
                Phase::Gathering => return Ok(Some((army.clone(), army.seat(run, username)?))),
                Phase::Running => {
                    return Err(ArmyError(format!(
                        "the {} army is already running",
                        army.0.name
                    )));
                }
                Phase::Over => {}
            }
        }
        let Some((name, config)) = fresh else {
            return Ok(None);
        };
        let army = Army::new(name, config);
        let player = army.seat(run, username)?;
        armies.insert(key.to_owned(), army.clone());
        Ok(Some((army, player)))
    }
}

#[derive(Clone)]
struct Army(Arc<Shared>);

struct Shared {
    name: String,
    config: Config,
    state: Mutex<State>,
    changed: watch::Sender<()>,
}

enum State {
    Forming(Vec<Option<RunId>>),
    Active(Active),
    Ended(String, Active),
}

enum Phase {
    Gathering,
    Running,
    Over,
}

#[derive(Default)]
struct Active {
    latest: Option<Step>,
    previous: Option<(u64, Vec<Value>)>,
    rotations: HashMap<String, Rotation>,
}

struct Rotation {
    players: Vec<usize>,
    seats: Vec<Seat>,
    holder: usize,
    skipped: usize,
    paused_until: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seat {
    Waiting,
    In,
    Left,
}

struct Step {
    index: u64,
    label: String,
    kind: Kind,
    arrivals: Vec<Option<Arrival>>,
    complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sync,
    Gather,
    Until,
}

enum Arrival {
    Synced,
    Gathered(Value),
    Done(bool),
}

enum EndCause<'a> {
    Left {
        player: usize,
        during: Option<&'a str>,
    },
    Diverged {
        player: usize,
        expected: (Kind, &'a str),
        got: (Kind, &'a str),
    },
    TimedOut {
        label: &'a str,
        missing: Vec<usize>,
    },
    Rotated {
        player: usize,
        label: &'a str,
        expected: Vec<usize>,
        got: Vec<usize>,
    },
}

impl Army {
    fn new(name: &str, config: Config) -> Self {
        let size = config.players.len();
        Self(Arc::new(Shared {
            name: name.to_owned(),
            config,
            state: Mutex::new(State::Forming(vec![None; size])),
            changed: watch::Sender::new(()),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap()
    }

    fn notify(&self) {
        self.0.changed.send_replace(());
    }

    fn phase(&self) -> Phase {
        match &*self.lock() {
            State::Forming(seats) if seats.iter().any(Option::is_some) => Phase::Gathering,
            State::Active(_) => Phase::Running,
            State::Forming(_) | State::Ended(..) => Phase::Over,
        }
    }

    fn seat(&self, run: RunId, username: &str) -> Result<usize, ArmyError> {
        let name = &self.0.name;
        let player = self.0.config.seat_of(username).ok_or_else(|| {
            ArmyError(format!(
                "{} isn't one of the {name} army's players",
                username.trim()
            ))
        })?;
        let mut state = self.lock();
        let State::Forming(seats) = &mut *state else {
            return Err(ArmyError(format!("the {name} army is already running")));
        };
        if seats[player].is_some_and(|holder| holder != run) {
            return Err(ArmyError(format!(
                "{} already started the {name} army in another tab",
                self.0.config.players[player]
            )));
        }
        seats[player] = Some(run);
        if seats.iter().all(Option::is_some) {
            *state = State::Active(Active::default());
            drop(state);
            self.notify();
        }
        Ok(player)
    }

    async fn formed(&self) {
        let mut changed = self.0.changed.subscribe();
        while matches!(*self.lock(), State::Forming(_)) {
            let _ = changed.changed().await;
        }
    }

    async fn settled(&self, index: u64) -> Result<Vec<Value>, ArmyError> {
        let mut changed = self.0.changed.subscribe();
        loop {
            if let Some(result) = self.result(index) {
                return result;
            }
            let _ = changed.changed().await;
        }
    }

    fn result(&self, index: u64) -> Option<Result<Vec<Value>, ArmyError>> {
        match &*self.lock() {
            State::Ended(message, settled) => Some(
                settled
                    .result(index)
                    .ok_or_else(|| ArmyError(message.clone())),
            ),
            State::Active(active) => active.result(index).map(Ok),
            State::Forming(_) => None,
        }
    }

    fn expire(&self, index: u64) -> Result<Vec<Value>, ArmyError> {
        let state = self.lock();
        let missing = match &*state {
            State::Ended(message, settled) => {
                return settled
                    .result(index)
                    .ok_or_else(|| ArmyError(message.clone()));
            }
            State::Forming(_) => Vec::new(),
            State::Active(active) => {
                if let Some(values) = active.result(index) {
                    return Ok(values);
                }
                active.latest.as_ref().map_or_else(Vec::new, Step::missing)
            }
        };
        let label = match &*state {
            State::Active(Active {
                latest: Some(step), ..
            }) => step.label.clone(),
            _ => String::new(),
        };
        Err(self.end_locked(
            state,
            EndCause::TimedOut {
                label: &label,
                missing,
            },
        ))
    }

    fn end(&self, cause: EndCause<'_>) {
        let state = self.lock();
        if matches!(*state, State::Active(_)) {
            self.end_locked(state, cause);
        }
    }

    fn end_locked(&self, mut state: MutexGuard<'_, State>, cause: EndCause<'_>) -> ArmyError {
        let message = self.describe(cause);
        let settled = match &mut *state {
            State::Active(active) => std::mem::take(active),
            _ => Active::default(),
        };
        *state = State::Ended(message.clone(), settled);
        drop(state);
        self.notify();
        ArmyError(message)
    }

    fn describe(&self, cause: EndCause<'_>) -> String {
        let player =
            |index: usize| format!("player {} ({})", index + 1, self.0.config.players[index]);
        let reason = match cause {
            EndCause::Left {
                player: index,
                during: Some(label),
            } => format!("{} left during \"{label}\"", player(index)),
            EndCause::Left {
                player: index,
                during: None,
            } => format!("{} left", player(index)),
            EndCause::Diverged {
                player: index,
                expected,
                got,
            } => format!(
                "{} reached {} \"{}\" while the army was at {} \"{}\"",
                player(index),
                got.0.name(),
                got.1,
                expected.0.name(),
                expected.1
            ),
            EndCause::TimedOut { label, missing } => {
                let missing: Vec<String> = missing.into_iter().map(player).collect();
                format!("\"{label}\" timed out waiting for {}", list(&missing))
            }
            EndCause::Rotated {
                player: index,
                label,
                expected,
                got,
            } => {
                let numbers = |players: Vec<usize>| {
                    let numbers: Vec<String> = players
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect();
                    list(&numbers)
                };
                format!(
                    "{} took turns in \"{label}\" with players {} while the army uses {}",
                    player(index),
                    numbers(got),
                    numbers(expected)
                )
            }
        };
        format!("The {} army ended because {reason}.", self.0.name)
    }
}

fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Active {
    fn result(&self, index: u64) -> Option<Vec<Value>> {
        let step = self.latest.as_ref()?;
        if step.index == index && step.complete {
            return Some(step.values());
        }
        if step.index > index {
            return Some(match &self.previous {
                Some((previous, values)) if *previous == index => values.clone(),
                _ => Vec::new(),
            });
        }
        None
    }
}

const IDLE_LAP_PAUSE: Duration = Duration::from_secs(1);

impl Rotation {
    fn new(players: Vec<usize>) -> Self {
        Self {
            seats: vec![Seat::Waiting; players.len()],
            players,
            holder: 0,
            skipped: 0,
            paused_until: None,
        }
    }

    fn started(&self) -> bool {
        !self.seats.contains(&Seat::Waiting)
    }

    fn seated(&self) -> usize {
        self.seats.iter().filter(|seat| **seat == Seat::In).count()
    }

    fn advance(&mut self) {
        for step in 1..=self.seats.len() {
            let next = (self.holder + step) % self.seats.len();
            if self.seats[next] != Seat::Left {
                self.holder = next;
                return;
            }
        }
    }

    fn pass(&mut self, acted: bool) {
        if acted {
            self.skipped = 0;
        } else {
            self.skipped += 1;
        }
        self.advance();
        if self.skipped >= self.seated() {
            self.skipped = 0;
            self.paused_until = Some(Instant::now() + IDLE_LAP_PAUSE);
        }
    }

    fn leave(&mut self, position: usize) {
        if self.seats[position] != Seat::In {
            return;
        }
        self.seats[position] = Seat::Left;
        if self.holder == position {
            self.skipped = 0;
            self.advance();
        }
    }
}

impl Step {
    fn new(index: u64, label: &str, kind: Kind, size: usize) -> Self {
        Self {
            index,
            label: label.to_owned(),
            kind,
            arrivals: (0..size).map(|_| None).collect(),
            complete: false,
        }
    }

    fn is_complete(&self) -> bool {
        self.arrivals.iter().all(|arrival| match arrival {
            Some(Arrival::Done(done)) => *done,
            Some(_) => true,
            None => false,
        })
    }

    fn missing(&self) -> Vec<usize> {
        self.arrivals
            .iter()
            .enumerate()
            .filter(|(_, arrival)| {
                !matches!(
                    arrival,
                    Some(Arrival::Synced | Arrival::Gathered(_) | Arrival::Done(true))
                )
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn values(&self) -> Vec<Value> {
        if self.kind != Kind::Gather {
            return Vec::new();
        }
        self.arrivals
            .iter()
            .map(|arrival| match arrival {
                Some(Arrival::Gathered(value)) => value.clone(),
                _ => Value::Null,
            })
            .collect()
    }
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Sync => "sync",
            Kind::Gather => "gather",
            Kind::Until => "until",
        }
    }
}

struct Seating {
    army: Army,
    player: usize,
    run: RunId,
    armed: bool,
}

impl Seating {
    async fn wait(mut self, timeout: Option<Duration>) -> Result<Option<Member>, ArmyError> {
        match timeout {
            None => self.army.formed().await,
            Some(timeout) => {
                let _ = tokio::time::timeout(timeout, self.army.formed()).await;
            }
        }
        self.armed = false;
        let mut state = self.army.lock();
        match &mut *state {
            State::Forming(seats) => {
                if seats[self.player] == Some(self.run) {
                    seats[self.player] = None;
                }
                Ok(None)
            }
            State::Active(_) => Ok(Some(Member {
                army: self.army.clone(),
                player: self.player,
                next: Cell::new(0),
                label: RefCell::default(),
            })),
            State::Ended(message, _) => Err(ArmyError(message.clone())),
        }
    }
}

impl Drop for Seating {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = self.army.lock();
        match &mut *state {
            State::Forming(seats) => {
                if seats[self.player] == Some(self.run) {
                    seats[self.player] = None;
                }
            }
            State::Active(_) => {
                self.army.end_locked(
                    state,
                    EndCause::Left {
                        player: self.player,
                        during: None,
                    },
                );
            }
            State::Ended(..) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Info {
    name: String,
    number: usize,
    size: usize,
    room: u32,
    players: Vec<String>,
}

pub(crate) struct Member {
    army: Army,
    player: usize,
    next: Cell<u64>,
    label: RefCell<String>,
}

impl Member {
    pub(crate) fn info(&self) -> Info {
        let shared = &self.army.0;
        Info {
            name: shared.name.clone(),
            number: self.player + 1,
            size: shared.config.players.len(),
            room: shared.config.room,
            players: shared.config.players.clone(),
        }
    }

    pub(crate) fn gear(&self, set: &str) -> Result<Gear, ArmyError> {
        self.army.0.config.gear(set, self.player).ok_or_else(|| {
            ArmyError(format!(
                "the {} army has no set \"{set}\"",
                self.army.0.name
            ))
        })
    }

    pub(crate) async fn sync(
        &self,
        label: &str,
        timeout: Option<Duration>,
    ) -> Result<(), ArmyError> {
        let index = self.arrive(label, Kind::Sync, Arrival::Synced)?;
        self.wait(index, timeout).await.map(drop)
    }

    pub(crate) async fn gather(
        &self,
        label: &str,
        value: Value,
        timeout: Option<Duration>,
    ) -> Result<Vec<Value>, ArmyError> {
        let index = self.arrive(label, Kind::Gather, Arrival::Gathered(value))?;
        self.wait(index, timeout).await
    }

    pub(crate) fn until(&self, label: &str, done: bool) -> Result<bool, ArmyError> {
        let index = self.arrive(label, Kind::Until, Arrival::Done(done))?;
        self.army
            .result(index)
            .transpose()
            .map(|values| values.is_some())
    }

    pub(crate) fn report(&self, done: bool) -> Result<bool, ArmyError> {
        let Some(index) = self.next.get().checked_sub(1) else {
            return Ok(true);
        };
        let mut state = self.army.lock();
        let active = match &mut *state {
            State::Active(active) => active,
            State::Ended(_, settled) if settled.result(index).is_some() => return Ok(true),
            State::Ended(message, _) => return Err(ArmyError(message.clone())),
            State::Forming(_) => return Ok(true),
        };
        let Some(step) = active
            .latest
            .as_mut()
            .filter(|step| step.index == index && step.kind == Kind::Until && !step.complete)
        else {
            return Ok(true);
        };
        step.arrivals[self.player] = Some(Arrival::Done(done));
        step.complete = step.is_complete();
        let complete = step.complete;
        drop(state);
        if complete {
            self.army.notify();
        }
        Ok(complete)
    }

    pub(crate) async fn settle(&self, timeout: Option<Duration>) -> Result<(), ArmyError> {
        let Some(index) = self.next.get().checked_sub(1) else {
            return Ok(());
        };
        self.wait(index, timeout).await.map(drop)
    }

    pub(crate) fn rotate(&self, label: &str, players: &[usize]) -> Result<bool, ArmyError> {
        let size = self.army.0.config.players.len();
        let players: Vec<usize> = players
            .iter()
            .map(|number| number.wrapping_sub(1))
            .collect();
        let valid = !players.is_empty()
            && players.iter().all(|index| *index < size)
            && players
                .iter()
                .enumerate()
                .all(|(position, index)| !players[..position].contains(index));
        if !valid {
            return Err(ArmyError(format!(
                "takeTurns needs different player numbers from 1 to {size}"
            )));
        }
        let Some(position) = players.iter().position(|index| *index == self.player) else {
            return Ok(false);
        };
        let mut state = self.army.lock();
        let active = match &mut *state {
            State::Active(active) => active,
            State::Ended(message, _) => return Err(ArmyError(message.clone())),
            State::Forming(_) => unreachable!("members only exist once the army has formed"),
        };
        let rotation = active
            .rotations
            .entry(label.to_owned())
            .or_insert_with(|| Rotation::new(players.clone()));
        if rotation.players != players {
            let expected = rotation.players.clone();
            return Err(self.army.end_locked(
                state,
                EndCause::Rotated {
                    player: self.player,
                    label,
                    expected,
                    got: players,
                },
            ));
        }
        if rotation.seats[position] == Seat::In {
            return Err(ArmyError(format!(
                "This player is already taking turns in \"{label}\"."
            )));
        }
        rotation.seats[position] = Seat::In;
        if rotation.seats[rotation.holder] == Seat::Left {
            rotation.advance();
        }
        drop(state);
        self.army.notify();
        Ok(true)
    }

    pub(crate) async fn turn(&self, label: &str, acted: Option<bool>) -> Result<(), ArmyError> {
        if let Some(acted) = acted {
            self.with_rotation(label, |rotation, position| {
                if rotation.holder == position {
                    rotation.pass(acted);
                }
            })?;
            self.army.notify();
        }
        let mut leaving = LeavingRotation {
            member: self,
            label,
            armed: true,
        };
        let mut changed = self.army.0.changed.subscribe();
        let result = loop {
            let turn = self.with_rotation(label, |rotation, position| {
                (rotation.started() && rotation.holder == position).then_some(rotation.paused_until)
            });
            match turn {
                Err(error) => break Err(error),
                Ok(Some(Some(until))) if until > Instant::now() => {
                    tokio::time::sleep_until(until).await;
                }
                Ok(Some(_)) => break Ok(()),
                Ok(None) => {
                    let _ = changed.changed().await;
                }
            }
        };
        leaving.armed = false;
        result
    }

    pub(crate) fn leave_rotation(&self, label: &str) {
        let left = self.with_rotation(label, |rotation, position| rotation.leave(position));
        if left.is_ok() {
            let mut state = self.army.lock();
            if let State::Active(active) = &mut *state
                && active
                    .rotations
                    .get(label)
                    .is_some_and(|rotation| rotation.seated() == 0 && rotation.started())
            {
                active.rotations.remove(label);
            }
            drop(state);
            self.army.notify();
        }
    }

    fn with_rotation<T>(
        &self,
        label: &str,
        action: impl FnOnce(&mut Rotation, usize) -> T,
    ) -> Result<T, ArmyError> {
        let mut state = self.army.lock();
        let active = match &mut *state {
            State::Active(active) => active,
            State::Ended(message, _) => return Err(ArmyError(message.clone())),
            State::Forming(_) => unreachable!("members only exist once the army has formed"),
        };
        let rotation = active
            .rotations
            .get_mut(label)
            .ok_or_else(|| ArmyError(format!("This player isn't taking turns in \"{label}\".")))?;
        let position = rotation
            .players
            .iter()
            .position(|index| *index == self.player)
            .ok_or_else(|| ArmyError(format!("This player isn't taking turns in \"{label}\".")))?;
        Ok(action(rotation, position))
    }

    fn arrive(&self, label: &str, kind: Kind, arrival: Arrival) -> Result<u64, ArmyError> {
        let index = self.next.get();
        let size = self.army.0.config.players.len();
        let mut state = self.army.lock();
        let active = match &mut *state {
            State::Active(active) => active,
            State::Ended(message, _) => return Err(ArmyError(message.clone())),
            State::Forming(_) => unreachable!("members only exist once the army has formed"),
        };
        let opens = match &active.latest {
            None => index == 0,
            Some(step) => step.index + 1 == index && step.complete,
        };
        if opens {
            if let Some(done) = active.latest.take() {
                active.previous = Some((done.index, done.values()));
            }
            active.latest = Some(Step::new(index, label, kind, size));
        }
        let Some(step) = active
            .latest
            .as_mut()
            .filter(|step| step.index == index && step.label == label && step.kind == kind)
        else {
            let (expected_kind, expected_label) = active
                .latest
                .as_ref()
                .map_or((kind, String::new()), |step| {
                    (step.kind, step.label.clone())
                });
            return Err(self.army.end_locked(
                state,
                EndCause::Diverged {
                    player: self.player,
                    expected: (expected_kind, &expected_label),
                    got: (kind, label),
                },
            ));
        };
        step.arrivals[self.player] = Some(arrival);
        step.complete = step.is_complete();
        let complete = step.complete;
        drop(state);
        self.next.set(index + 1);
        self.label.replace(label.to_owned());
        if complete {
            self.army.notify();
        }
        Ok(index)
    }

    async fn wait(&self, index: u64, timeout: Option<Duration>) -> Result<Vec<Value>, ArmyError> {
        let mut leaving = Leaving {
            member: self,
            armed: true,
        };
        let result = match timeout {
            None => self.army.settled(index).await,
            Some(timeout) => match tokio::time::timeout(timeout, self.army.settled(index)).await {
                Ok(result) => result,
                Err(_) => self.army.expire(index),
            },
        };
        leaving.armed = false;
        result
    }
}

struct Leaving<'a> {
    member: &'a Member,
    armed: bool,
}

impl Drop for Leaving<'_> {
    fn drop(&mut self) {
        if self.armed {
            let label = self.member.label.borrow();
            self.member.army.end(EndCause::Left {
                player: self.member.player,
                during: Some(&label),
            });
        }
    }
}

struct LeavingRotation<'a> {
    member: &'a Member,
    label: &'a str,
    armed: bool,
}

impl Drop for LeavingRotation<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.member.leave_rotation(self.label);
        }
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        self.army.end(EndCause::Left {
            player: self.player,
            during: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, future::Future, pin::Pin, task::Poll};

    use serde_json::json;

    use super::*;

    const SHORT: Duration = Duration::from_millis(40);

    const DUO: &str = "players: [player1, player2]\nroom: 1234\nitems:\n  sword: Example Sword\nsets:\n  boss:\n    default: { weapon: sword }\n    player2: { class: Example Class }\n";
    const TRIO: &str = "players: [player1, player2, player3]\nroom: 4321\n";

    struct Folder(PathBuf);

    impl Folder {
        fn new(files: &[(&str, &str)]) -> Self {
            let mut random = [0; 8];
            getrandom::getrandom(&mut random).unwrap();
            let root =
                std::env::temp_dir().join(format!("lyra-army-{:x}", u64::from_ne_bytes(random)));
            fs::create_dir_all(&root).unwrap();
            for (name, text) in files {
                fs::write(root.join(name), text).unwrap();
            }
            Self(root)
        }

        fn armies(&self) -> Armies {
            Armies::new(self.0.clone())
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
        let mut futures: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
        let mut outputs: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();
        std::future::poll_fn(|cx| {
            let mut pending = false;
            for (future, output) in futures.iter_mut().zip(outputs.iter_mut()) {
                if output.is_none() {
                    match future.as_mut().poll(cx) {
                        Poll::Ready(value) => *output = Some(value),
                        Poll::Pending => pending = true,
                    }
                }
            }
            if pending {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        outputs.into_iter().map(Option::unwrap).collect()
    }

    async fn form(armies: &Armies, name: &str, usernames: &[&str]) -> Vec<Member> {
        let starts = usernames
            .iter()
            .enumerate()
            .map(|(run, username)| armies.start(name, RunId(run as u64), username, None))
            .collect();
        let mut members: Vec<Member> = join_all(starts)
            .await
            .into_iter()
            .map(|started| started.unwrap().unwrap())
            .collect();
        members.sort_by_key(|member| member.player);
        members
    }

    #[tokio::test]
    async fn players_are_numbered_in_config_order_once_everyone_starts() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["PLAYER2", "player1"]).await;
        assert_eq!(
            members[1].info(),
            Info {
                name: "duo".into(),
                number: 2,
                size: 2,
                room: 1234,
                players: vec!["player1".into(), "player2".into()],
            }
        );
        assert_eq!(members[0].info().number, 1);
    }

    #[tokio::test]
    async fn a_start_that_times_out_frees_its_place() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let armies = folder.armies();
        assert_eq!(
            armies
                .start("duo", RunId(1), "player1", Some(SHORT))
                .await
                .map(|member| member.is_some()),
            Ok(false)
        );
        let members = form(&armies, "duo", &["player1", "player2"]).await;
        assert_eq!(members.len(), 2);
    }

    #[tokio::test]
    async fn start_refuses_strangers_a_second_tab_and_a_running_army() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let armies = folder.armies();
        let error = |result: Result<Option<Member>, ArmyError>| result.err().unwrap().0;
        assert_eq!(
            error(armies.start("duo", RunId(1), "player3", None).await),
            "player3 isn't one of the duo army's players"
        );
        assert_eq!(
            error(armies.start("Missing", RunId(1), "player1", None).await),
            format!("{} doesn't exist", folder.0.join("Missing.yaml").display())
        );
        let waiting = armies.start("duo", RunId(1), "player1", Some(Duration::from_secs(1)));
        tokio::pin!(waiting);
        let gathering = || {
            armies
                .0
                .armies
                .lock()
                .unwrap()
                .get("duo")
                .is_some_and(|army| matches!(army.phase(), Phase::Gathering))
        };
        while !gathering() {
            assert!(
                pending(&mut waiting),
                "the first start ended before taking its place"
            );
            tokio::task::yield_now().await;
        }
        assert_eq!(
            error(armies.start("duo", RunId(2), "player1", None).await),
            "player1 already started the duo army in another tab"
        );
        assert!(matches!(waiting.await, Ok(None)));
        let _members = form(&armies, "duo", &["player1", "player2"]).await;
        assert_eq!(
            error(armies.start("DUO", RunId(9), "player1", None).await),
            "the duo army is already running"
        );
    }

    #[tokio::test]
    async fn gather_gives_every_player_the_values_in_player_order() {
        let folder = Folder::new(&[("trio.yaml", TRIO)]);
        let members = form(&folder.armies(), "trio", &["player1", "player2", "player3"]).await;
        let gathered = join_all(vec![
            members[2].gather("count", json!(3), None),
            members[0].gather("count", json!(1), None),
            members[1].gather("count", json!({ "two": 2 }), None),
        ])
        .await;
        for values in gathered {
            assert_eq!(values, Ok(vec![json!(1), json!({ "two": 2 }), json!(3)]));
        }
        let synced = join_all(vec![
            members[0].sync("next", None),
            members[1].sync("next", None),
            members[2].sync("next", None),
        ])
        .await;
        assert!(synced.iter().all(Result::is_ok), "{synced:?}");
    }

    #[tokio::test]
    async fn a_finished_step_still_reaches_a_player_that_reads_it_late() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let mut members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        {
            let late = members[1].gather("count", json!(2), None);
            tokio::pin!(late);
            assert!(
                tokio::time::timeout(Duration::ZERO, &mut late)
                    .await
                    .is_err()
            );
            assert_eq!(
                members[0].gather("count", json!(1), None).await,
                Ok(vec![json!(1), json!(2)])
            );
            assert_eq!(members[0].until("drops", false), Ok(false));
            assert_eq!(late.await, Ok(vec![json!(1), json!(2)]));
            assert_eq!(members[1].until("drops", true), Ok(false));
            assert_eq!(members[0].report(true), Ok(true));
        }
        let first = members.remove(0);
        let late = members[0].sync("finished", None);
        tokio::pin!(late);
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut late)
                .await
                .is_err()
        );
        assert_eq!(first.sync("finished", None).await, Ok(()));
        drop(first);
        assert_eq!(late.await, Ok(()));
    }

    #[tokio::test]
    async fn players_that_reach_different_steps_end_the_army() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        let message = "The duo army ended because player 2 (player2) reached gather \"loot\" while the army was at sync \"boss\".";
        let [first, second] = join_all(vec![
            Box::pin(members[0].sync("boss", None)) as Pin<Box<dyn Future<Output = _>>>,
            Box::pin(async { members[1].gather("loot", json!(1), None).await.map(drop) }),
        ])
        .await
        .try_into()
        .ok()
        .unwrap();
        assert_eq!(first, Err(ArmyError(message.into())));
        assert_eq!(second, Err(ArmyError(message.into())));
        assert_eq!(
            members[1].sync("later", None).await,
            Err(ArmyError(message.into()))
        );
    }

    #[tokio::test]
    async fn interrupting_a_pending_step_ends_the_army() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        assert!(
            tokio::time::timeout(SHORT, members[1].sync("boss", None))
                .await
                .is_err()
        );
        assert_eq!(
            members[0].sync("boss", None).await,
            Err(ArmyError(
                "The duo army ended because player 2 (player2) left during \"boss\".".into()
            ))
        );
    }

    #[tokio::test]
    async fn a_member_whose_script_ends_ends_the_army() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let mut members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        let second = members.pop().unwrap();
        let waiting = members[0].sync("boss", None);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut waiting)
                .await
                .is_err()
        );
        drop(second);
        assert_eq!(
            waiting.await,
            Err(ArmyError(
                "The duo army ended because player 2 (player2) left.".into()
            ))
        );
    }

    #[tokio::test]
    async fn a_step_that_times_out_names_the_players_it_waited_for() {
        let folder = Folder::new(&[("trio.yaml", TRIO)]);
        let members = form(&folder.armies(), "trio", &["player1", "player2", "player3"]).await;
        let message = "The trio army ended because \"boss\" timed out waiting for player 2 (player2) and player 3 (player3).";
        assert_eq!(
            members[0].sync("boss", Some(SHORT)).await,
            Err(ArmyError(message.into()))
        );
        assert_eq!(
            members[1].sync("boss", None).await,
            Err(ArmyError(message.into()))
        );
    }

    #[tokio::test]
    async fn until_completes_the_first_time_every_player_is_done() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        assert_eq!(members[0].until("drops", true), Ok(false));
        assert_eq!(members[1].until("drops", false), Ok(false));
        assert_eq!(members[0].report(false), Ok(false));
        assert_eq!(members[1].report(true), Ok(false));
        let settled = members[1].settle(None);
        tokio::pin!(settled);
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut settled)
                .await
                .is_err()
        );
        assert_eq!(members[0].report(true), Ok(true));
        assert_eq!(settled.await, Ok(()));
        assert_eq!(members[0].report(false), Ok(true));
        assert_eq!(members[0].settle(None).await, Ok(()));
    }

    #[tokio::test]
    async fn an_until_that_times_out_names_the_players_still_working() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        assert_eq!(members[0].until("drops", true), Ok(false));
        assert_eq!(members[1].until("drops", false), Ok(false));
        assert_eq!(
            members[0].settle(Some(SHORT)).await,
            Err(ArmyError(
                "The duo army ended because \"drops\" timed out waiting for player 2 (player2)."
                    .into()
            ))
        );
    }

    #[tokio::test]
    async fn armies_with_different_configs_run_side_by_side() {
        let folder = Folder::new(&[("duo.yaml", DUO), ("trio.yaml", TRIO)]);
        let armies = folder.armies();
        let duo = form(&armies, "duo", &["player1", "player2"]).await;
        let trio = form(&armies, "trio", &["player1", "player2", "player3"]).await;
        assert_eq!((duo[0].info().room, trio[0].info().room), (1234, 4321));
        assert_eq!(duo[0].until("work", true), Ok(false));
        let gathered = join_all(vec![
            trio[0].gather("count", json!(1), None),
            trio[1].gather("count", json!(2), None),
            trio[2].gather("count", json!(3), None),
        ])
        .await;
        assert_eq!(gathered[0], Ok(vec![json!(1), json!(2), json!(3)]));
    }

    #[tokio::test]
    async fn an_army_forms_again_from_the_file_after_it_ends() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let armies = folder.armies();
        let members = form(&armies, "duo", &["player1", "player2"]).await;
        drop(members);
        fs::write(folder.0.join("duo.yaml"), DUO.replace("1234", "5678")).unwrap();
        let members = form(&armies, "duo", &["player1", "player2"]).await;
        assert_eq!(members[0].info().room, 5678);
    }

    #[tokio::test]
    async fn gear_resolves_this_players_set() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        let gear = serde_json::to_value(members[1].gear("boss").unwrap()).unwrap();
        assert_eq!(gear["weapon"], json!("Example Sword"));
        assert_eq!(gear["class"], json!("Example Class"));
        assert_eq!(
            members[0].gear("farm").map(drop),
            Err(ArmyError("the duo army has no set \"farm\"".into()))
        );
    }

    fn pending<F: Future + Unpin>(future: &mut F) -> bool {
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        Pin::new(future).poll(&mut context).is_pending()
    }

    #[tokio::test]
    async fn turns_go_around_the_listed_players_once_all_of_them_take_part() {
        let folder = Folder::new(&[("trio.yaml", TRIO)]);
        let members = form(&folder.armies(), "trio", &["player1", "player2", "player3"]).await;
        assert_eq!(members[2].rotate("taunt", &[3, 1]), Ok(true));
        assert_eq!(members[1].rotate("taunt", &[3, 1]), Ok(false));
        let mut third = Box::pin(members[2].turn("taunt", None));
        assert!(pending(&mut third), "the rotation waits for player 1");
        assert_eq!(members[0].rotate("taunt", &[3, 1]), Ok(true));
        assert_eq!(third.await, Ok(()));
        let mut first = Box::pin(members[0].turn("taunt", None));
        assert!(pending(&mut first));
        let mut third = Box::pin(members[2].turn("taunt", Some(true)));
        assert!(pending(&mut third));
        assert_eq!(first.await, Ok(()));
        let mut first = Box::pin(members[0].turn("taunt", Some(false)));
        assert!(pending(&mut first));
        assert_eq!(third.await, Ok(()));
        assert_eq!(members[1].rotate("heal", &[1, 2]), Ok(true));
        assert_eq!(members[0].rotate("heal", &[1, 2]), Ok(true));
        let mut second = Box::pin(members[1].turn("heal", None));
        assert!(
            pending(&mut second),
            "turns start at the first listed player"
        );
        assert_eq!(members[0].turn("heal", None).await, Ok(()));
    }

    #[tokio::test]
    async fn a_player_that_stops_taking_turns_is_skipped() {
        let folder = Folder::new(&[("trio.yaml", TRIO)]);
        let members = form(&folder.armies(), "trio", &["player1", "player2", "player3"]).await;
        for member in &members {
            assert_eq!(member.rotate("taunt", &[1, 2, 3]), Ok(true));
        }
        assert_eq!(members[0].turn("taunt", None).await, Ok(()));
        let mut second = Box::pin(members[1].turn("taunt", None));
        let mut third = Box::pin(members[2].turn("taunt", None));
        assert!(pending(&mut second) && pending(&mut third));
        drop(second);
        members[0].leave_rotation("taunt");
        assert_eq!(third.await, Ok(()));
        assert_eq!(members[2].turn("taunt", Some(true)).await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lap_where_nobody_acted_pauses_the_next_turn() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        for member in &members {
            assert_eq!(member.rotate("taunt", &[1, 2]), Ok(true));
        }
        assert_eq!(members[0].turn("taunt", None).await, Ok(()));
        let mut first = Box::pin(members[0].turn("taunt", Some(false)));
        assert!(pending(&mut first));
        assert_eq!(members[1].turn("taunt", None).await, Ok(()));
        let mut second = Box::pin(members[1].turn("taunt", Some(false)));
        assert!(pending(&mut second));
        let started = Instant::now();
        assert_eq!(first.await, Ok(()));
        assert!(
            started.elapsed() >= IDLE_LAP_PAUSE,
            "{:?}",
            started.elapsed()
        );
        let mut first = Box::pin(members[0].turn("taunt", Some(true)));
        assert!(pending(&mut first));
        let started = Instant::now();
        assert_eq!(second.await, Ok(()));
        assert!(
            started.elapsed() < IDLE_LAP_PAUSE,
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn rotations_need_one_list_of_different_player_numbers() {
        let folder = Folder::new(&[("duo.yaml", DUO)]);
        let members = form(&folder.armies(), "duo", &["player1", "player2"]).await;
        for players in [&[][..], &[0], &[3], &[1, 1]] {
            assert_eq!(
                members[0].rotate("taunt", players),
                Err(ArmyError(
                    "takeTurns needs different player numbers from 1 to 2".into()
                )),
                "{players:?}"
            );
        }
        assert_eq!(members[0].rotate("taunt", &[1, 2]), Ok(true));
        assert_eq!(
            members[0].rotate("taunt", &[1, 2]),
            Err(ArmyError(
                "This player is already taking turns in \"taunt\".".into()
            ))
        );
        let message = "The duo army ended because player 2 (player2) took turns in \"taunt\" with players 2 and 1 while the army uses 1 and 2.";
        assert_eq!(
            members[1].rotate("taunt", &[2, 1]),
            Err(ArmyError(message.into()))
        );
        assert_eq!(
            members[0].turn("taunt", None).await,
            Err(ArmyError(message.into()))
        );
    }
}
