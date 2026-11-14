use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use crate::scripting::{Launch, Outcome, Supplied, Values};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct EntryId(u64);

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) id: EntryId,
    pub(super) script: PathBuf,
    pub(super) values: Option<Values>,
    pub(super) state: State,
}

#[derive(Clone)]
pub(super) enum State {
    Waiting,
    Running { started: Instant },
    Ended { outcome: Outcome, took: Duration },
}

impl Entry {
    pub(super) fn waiting(&self) -> bool {
        matches!(self.state, State::Waiting)
    }

    pub(super) fn running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }
}

#[derive(Default)]
pub(super) struct Queue {
    entries: Vec<Entry>,
    playing: bool,
}

impl Queue {
    pub(super) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(super) fn playing(&self) -> bool {
        self.playing
    }

    pub(super) fn next(&self) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.waiting())
    }

    pub(super) fn running(&self) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.running())
    }

    pub(super) fn waits_for(&self, script: &Path) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.waiting() && entry.script == script)
    }

    pub(super) fn add(&mut self, script: PathBuf, values: Values) -> EntryId {
        let entry = Self::entry(script, values);
        let id = entry.id;
        self.entries.push(entry);
        id
    }

    pub(super) fn add_next(&mut self, script: PathBuf, values: Values) -> EntryId {
        let entry = Self::entry(script, values);
        let id = entry.id;
        let at = self
            .entries
            .iter()
            .position(Entry::waiting)
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
        id
    }

    fn entry(script: PathBuf, values: Values) -> Entry {
        static IDS: AtomicU64 = AtomicU64::new(0);
        Entry {
            id: EntryId(IDS.fetch_add(1, Ordering::Relaxed)),
            script,
            values: Some(values),
            state: State::Waiting,
        }
    }

    pub(super) fn play(&mut self) {
        self.playing = self.next().is_some() || self.running().is_some();
    }

    pub(super) fn pause(&mut self) {
        self.playing = false;
    }

    pub(super) fn start_next(&mut self, now: Instant) -> Option<Launch> {
        if !self.playing || self.running().is_some() {
            return None;
        }
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.waiting()) else {
            self.playing = false;
            return None;
        };
        let values = entry.values.clone()?;
        entry.state = State::Running { started: now };
        Some(Launch {
            entry: entry.script.clone(),
            inputs: Supplied::Values(values),
        })
    }

    pub(super) fn ended(&mut self, outcome: &Outcome, now: Instant) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.running())
            && let State::Running { started } = entry.state
        {
            entry.state = State::Ended {
                outcome: outcome.clone(),
                took: now.saturating_duration_since(started),
            };
        }
        if matches!(outcome, Outcome::Exited { logout: true, .. }) {
            self.playing = false;
        }
    }

    pub(super) fn move_entry(&mut self, id: EntryId, to: usize) {
        let Some(from) = self.position(id) else {
            return;
        };
        if !self.entries[from].waiting() || !self.entries.get(to).is_some_and(Entry::waiting) {
            return;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
    }

    pub(super) fn set_values(&mut self, id: EntryId, values: Option<Values>) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id && entry.waiting())
        {
            entry.values = values;
        }
    }

    pub(super) fn remove(&mut self, id: EntryId) {
        self.entries
            .retain(|entry| entry.id != id || entry.running());
    }

    pub(super) fn clear(&mut self) {
        self.entries.retain(Entry::running);
    }

    pub(super) fn clear_finished(&mut self) {
        self.entries
            .retain(|entry| !matches!(entry.state, State::Ended { .. }));
    }

    fn position(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::scripting::Limit;

    fn values(value: serde_json::Value) -> Values {
        value.as_object().unwrap().clone()
    }

    fn queue(scripts: &[&str]) -> Queue {
        let mut queue = Queue::default();
        for script in scripts {
            queue.add(PathBuf::from(script), Values::new());
        }
        queue.play();
        queue
    }

    fn start(queue: &mut Queue) -> Option<String> {
        queue
            .start_next(Instant::now())
            .map(|launch| launch.entry.display().to_string())
    }

    fn outcomes(queue: &Queue) -> Vec<Option<Outcome>> {
        queue
            .entries()
            .iter()
            .map(|entry| match &entry.state {
                State::Ended { outcome, .. } => Some(outcome.clone()),
                _ => None,
            })
            .collect()
    }

    fn failed() -> Outcome {
        Outcome::Failed {
            message: "boom".into(),
            stack: None,
        }
    }

    #[test]
    fn entries_run_in_order_and_the_queue_stops_playing_after_the_last() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        assert_eq!(start(&mut queue).as_deref(), Some("a.ts"));
        assert_eq!(start(&mut queue), None, "one entry runs at a time");
        queue.ended(&Outcome::Completed, Instant::now());
        assert_eq!(start(&mut queue).as_deref(), Some("b.ts"));
        queue.ended(&Outcome::Completed, Instant::now());
        assert_eq!(start(&mut queue), None);
        assert!(!queue.playing());
        assert_eq!(
            outcomes(&queue),
            [Some(Outcome::Completed), Some(Outcome::Completed)]
        );
    }

    #[test]
    fn failures_and_plain_exits_are_recorded_and_the_next_entry_runs() {
        let ends = [
            failed(),
            Outcome::Killed(Limit::Memory),
            Outcome::Exited {
                logout: false,
                close_tab: false,
            },
        ];
        for end in ends {
            let mut queue = queue(&["a.ts", "b.ts"]);
            start(&mut queue);
            queue.ended(&end, Instant::now());
            assert_eq!(start(&mut queue).as_deref(), Some("b.ts"), "{end:?}");
            assert_eq!(outcomes(&queue)[0], Some(end));
        }
    }

    #[test]
    fn exiting_with_a_logout_pauses_the_queue() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        queue.ended(
            &Outcome::Exited {
                logout: true,
                close_tab: false,
            },
            Instant::now(),
        );
        assert_eq!(start(&mut queue), None);
        queue.play();
        assert_eq!(start(&mut queue).as_deref(), Some("b.ts"));
    }

    #[test]
    fn a_paused_queue_records_the_stop_and_resumes_with_the_next_entry() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        queue.pause();
        queue.ended(&Outcome::Stopped, Instant::now());
        assert_eq!(start(&mut queue), None);
        assert_eq!(outcomes(&queue)[0], Some(Outcome::Stopped));
        queue.play();
        assert_eq!(start(&mut queue).as_deref(), Some("b.ts"));
    }

    #[test]
    fn a_skipped_entry_is_recorded_as_stopped_and_the_next_one_runs() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        queue.ended(&Outcome::Stopped, Instant::now());
        assert_eq!(start(&mut queue).as_deref(), Some("b.ts"));
    }

    #[test]
    fn a_run_outside_the_queue_ending_records_nothing() {
        let mut queue = queue(&["a.ts"]);
        queue.ended(&failed(), Instant::now());
        assert_eq!(outcomes(&queue), [None]);
        assert_eq!(start(&mut queue).as_deref(), Some("a.ts"));
    }

    #[test]
    fn two_entries_for_one_script_run_with_their_own_values() {
        let mut queue = Queue::default();
        queue.add("hunt.ts".into(), values(json!({ "item": "Helm" })));
        queue.add("hunt.ts".into(), values(json!({ "item": "Cape" })));
        queue.play();
        let mut supplied = Vec::new();
        while let Some(launch) = queue.start_next(Instant::now()) {
            supplied.push(launch.inputs);
            queue.ended(&Outcome::Completed, Instant::now());
        }
        assert_eq!(
            supplied,
            [
                Supplied::Values(values(json!({ "item": "Helm" }))),
                Supplied::Values(values(json!({ "item": "Cape" }))),
            ]
        );
    }

    #[test]
    fn waiting_entries_reorder_and_the_running_one_stays_put() {
        let mut queue = queue(&["a.ts", "b.ts", "c.ts", "d.ts"]);
        start(&mut queue);
        queue.ended(&Outcome::Completed, Instant::now());
        start(&mut queue);
        let ids: Vec<EntryId> = queue.entries().iter().map(|entry| entry.id).collect();

        queue.move_entry(ids[3], 2);
        queue.move_entry(ids[1], 3);
        queue.move_entry(ids[0], 3);
        queue.move_entry(ids[2], 1);

        let order: Vec<String> = queue
            .entries()
            .iter()
            .map(|entry| entry.script.display().to_string())
            .collect();
        assert_eq!(order, ["a.ts", "b.ts", "d.ts", "c.ts"]);
        queue.ended(&Outcome::Completed, Instant::now());
        assert_eq!(start(&mut queue).as_deref(), Some("d.ts"));
    }

    #[test]
    fn the_running_entry_cannot_be_removed_or_edited() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        let running = queue.entries()[0].id;
        queue.remove(running);
        queue.set_values(running, Some(values(json!({ "x": 1 }))));
        queue.clear();
        assert_eq!(queue.entries().len(), 1);
        assert!(queue.entries()[0].running());
        assert_eq!(queue.entries()[0].values, Some(Values::new()));
    }

    #[test]
    fn entries_in_different_queues_never_share_an_id() {
        let first = queue(&["a.ts"]);
        let second = queue(&["a.ts"]);
        assert_ne!(first.entries()[0].id, second.entries()[0].id);
    }

    #[test]
    fn a_waiting_entry_runs_with_its_edited_values() {
        let mut queue = queue(&["a.ts"]);
        let id = queue.entries()[0].id;
        queue.set_values(id, Some(values(json!({ "kills": 3 }))));
        let launch = queue.start_next(Instant::now()).unwrap();
        assert_eq!(
            launch.inputs,
            Supplied::Values(values(json!({ "kills": 3 })))
        );
    }

    #[test]
    fn the_queue_waits_at_an_entry_that_needs_inputs_until_they_are_fixed() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        let next = queue.entries()[1].id;
        queue.set_values(next, None);
        queue.ended(&Outcome::Completed, Instant::now());
        assert_eq!(start(&mut queue), None);
        assert!(queue.playing());
        queue.set_values(next, Some(values(json!({ "kills": 3 }))));
        assert_eq!(start(&mut queue).as_deref(), Some("b.ts"));
    }

    #[test]
    fn an_entry_added_next_runs_before_the_waiting_ones() {
        let mut queue = queue(&["a.ts", "b.ts"]);
        start(&mut queue);
        queue.add_next("now.ts".into(), Values::new());
        queue.ended(&Outcome::Stopped, Instant::now());
        assert_eq!(start(&mut queue).as_deref(), Some("now.ts"));
    }

    #[test]
    fn clearing_finished_entries_keeps_the_running_and_waiting_ones() {
        let mut queue = queue(&["a.ts", "b.ts", "c.ts"]);
        start(&mut queue);
        queue.ended(&Outcome::Completed, Instant::now());
        start(&mut queue);
        queue.clear_finished();
        let order: Vec<String> = queue
            .entries()
            .iter()
            .map(|entry| entry.script.display().to_string())
            .collect();
        assert_eq!(order, ["b.ts", "c.ts"]);
    }
}
