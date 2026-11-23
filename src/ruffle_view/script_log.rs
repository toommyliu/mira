use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Local};
use gpui::{Context, SharedString, Task};

use crate::scripting::{Level, Limit, Outcome};

const MAX_ENTRIES: usize = 5_000;
const MAX_BYTES: usize = 4 << 20;
const MAX_TEXT: usize = 16 << 10;
const FRAME: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EntryId(u64);

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub(super) id: EntryId,
    pub(super) at: DateTime<Local>,
    pub(super) level: Level,
    pub(super) body: Body,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Body {
    Output {
        text: SharedString,
        repeats: u32,
    },
    Run {
        script: SharedString,
        says: SharedString,
        detail: Option<SharedString>,
        starts: bool,
    },
}

impl Entry {
    pub(super) fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let found = |text: &str| text.to_lowercase().contains(query);
        match &self.body {
            Body::Output { text, .. } => found(text),
            Body::Run {
                script,
                says,
                detail,
                ..
            } => found(&format!("{script} {says}")) || detail.as_deref().is_some_and(found),
        }
    }

    fn bytes(&self) -> usize {
        match &self.body {
            Body::Output { text, .. } => text.len(),
            Body::Run {
                script,
                says,
                detail,
                ..
            } => script.len() + says.len() + detail.as_ref().map_or(0, |detail| detail.len()),
        }
    }

    fn repeats(&mut self, other: &Entry) -> bool {
        match (&mut self.body, &other.body) {
            (
                Body::Output { text, repeats },
                Body::Output {
                    text: next,
                    repeats: more,
                },
            ) if self.level == other.level
                && text == next
                && self.at.date_naive() == other.at.date_naive() =>
            {
                *repeats = repeats.saturating_add(*more);
                self.at = other.at;
                true
            }
            _ => false,
        }
    }
}

pub(super) enum Event<'a> {
    Started {
        place: Option<(usize, usize)>,
    },
    Ended {
        outcome: &'a Outcome,
        took: Duration,
    },
    Interrupted,
    NotRestarted,
    CharacterChanged,
    RetreatFailed(&'a str),
}

impl Event<'_> {
    fn describe(&self) -> (Level, String, Option<String>) {
        let info = |says: &str| (Level::Info, says.to_owned(), None);
        let warn = |says: &str| (Level::Warn, says.to_owned(), None);
        match self {
            Event::Started { place: None } => info("started"),
            Event::Started {
                place: Some((at, of)),
            } => info(&format!("started · {at} of {of} in the queue")),
            Event::Ended { outcome, took } => match outcome {
                Outcome::Completed => info(&format!("finished in {}", duration(*took))),
                Outcome::Stopped => info("stopped"),
                Outcome::Exited { logout: true, .. } => info("exited and logged out"),
                Outcome::Exited { logout: false, .. } => info("exited"),
                Outcome::Failed { message, stack } => (
                    Level::Error,
                    "failed".into(),
                    Some(match stack {
                        Some(stack) => format!("{message}\n{stack}"),
                        None => message.clone(),
                    }),
                ),
                Outcome::Killed(Limit::Turn) => (
                    Level::Error,
                    "was stopped for running too long without waiting".into(),
                    None,
                ),
                Outcome::Killed(Limit::Memory) => (
                    Level::Error,
                    "was stopped for using too much memory".into(),
                    None,
                ),
            },
            Event::Interrupted => warn("lost the connection and restarts after reconnecting"),
            Event::NotRestarted => warn("didn't restart because a different account signed in"),
            Event::CharacterChanged => warn("is stopping because a different character signed in"),
            Event::RetreatFailed(error) => warn(&format!("couldn't move somewhere safe: {error}")),
        }
    }
}

pub(super) fn duration(took: Duration) -> String {
    let seconds = took.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[derive(Default)]
struct Entries {
    items: VecDeque<Entry>,
    bytes: usize,
    next: u64,
}

impl Entries {
    fn push(&mut self, mut entry: Entry) {
        if let Some(last) = self.items.back_mut()
            && last.repeats(&entry)
        {
            return;
        }
        entry.id = EntryId(self.next);
        self.next += 1;
        self.bytes += entry.bytes();
        self.items.push_back(entry);
        while self.items.len() > MAX_ENTRIES || self.bytes > MAX_BYTES && self.items.len() > 1 {
            if let Some(dropped) = self.items.pop_front() {
                self.bytes -= dropped.bytes();
            }
        }
    }

    fn clear(&mut self) {
        self.items.clear();
        self.bytes = 0;
    }
}

struct Inbox {
    pending: Mutex<Entries>,
    wake: async_channel::Sender<()>,
}

#[derive(Clone)]
pub(super) struct Sink(Arc<Inbox>);

impl Sink {
    pub(super) fn push(&self, level: Level, text: &str) {
        let entry = Entry {
            id: EntryId(0),
            at: Local::now(),
            level,
            body: Body::Output {
                text: clip(text).into(),
                repeats: 1,
            },
        };
        self.0.pending.lock().unwrap().push(entry);
        let _ = self.0.wake.try_send(());
    }
}

fn clip(text: &str) -> String {
    if text.len() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} more bytes)", &text[..end], text.len() - end)
}

pub(super) struct ScriptLog {
    entries: Entries,
    cleared: u64,
    inbox: Arc<Inbox>,
    _drain: Task<()>,
}

impl ScriptLog {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let (wake, woken) = async_channel::bounded(1);
        let drain = cx.spawn(async move |this, cx| {
            while woken.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.take(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(FRAME).await;
            }
        });
        Self {
            entries: Entries::default(),
            cleared: 0,
            inbox: Arc::new(Inbox {
                pending: Mutex::new(Entries::default()),
                wake,
            }),
            _drain: drain,
        }
    }

    pub(super) fn sink(&self) -> Sink {
        Sink(self.inbox.clone())
    }

    pub(super) fn entries(&self) -> &VecDeque<Entry> {
        &self.entries.items
    }

    pub(super) fn get(&self, id: EntryId) -> Option<&Entry> {
        let first = self.entries.items.front()?.id.0;
        let index = usize::try_from(id.0.checked_sub(first)?).ok()?;
        self.entries.items.get(index)
    }

    pub(super) fn next_id(&self) -> EntryId {
        EntryId(self.entries.next)
    }

    pub(super) fn cleared(&self) -> u64 {
        self.cleared
    }

    pub(super) fn run(&mut self, script: SharedString, event: Event, cx: &mut Context<Self>) {
        self.take(cx);
        let (level, says, detail) = event.describe();
        self.entries.push(Entry {
            id: EntryId(0),
            at: Local::now(),
            level,
            body: Body::Run {
                script,
                says: says.into(),
                detail: detail.map(|detail| clip(&detail).into()),
                starts: matches!(event, Event::Started { .. }),
            },
        });
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn record(
        &mut self,
        at: DateTime<Local>,
        level: Level,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.take(cx);
        self.entries.push(Entry {
            id: EntryId(0),
            at,
            level,
            body: Body::Output {
                text: clip(text).into(),
                repeats: 1,
            },
        });
        cx.notify();
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.inbox.pending.lock().unwrap().clear();
        self.entries.clear();
        self.cleared += 1;
        cx.notify();
    }

    fn take(&mut self, cx: &mut Context<Self>) {
        let pending = std::mem::take(&mut *self.inbox.pending.lock().unwrap());
        if pending.items.is_empty() {
            return;
        }
        for entry in pending.items {
            self.entries.push(entry);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, Entity, TestAppContext};

    use super::*;

    fn settle(cx: &mut TestAppContext) {
        cx.run_until_parked();
        cx.executor().advance_clock(FRAME);
        cx.run_until_parked();
    }

    fn texts(log: &Entity<ScriptLog>, cx: &TestAppContext) -> Vec<(Level, String, u32)> {
        log.read_with(cx, |log, _| {
            log.entries()
                .iter()
                .map(|entry| match &entry.body {
                    Body::Output { text, repeats } => (entry.level, text.to_string(), *repeats),
                    Body::Run { script, says, .. } => (entry.level, format!("{script} {says}"), 1),
                })
                .collect()
        })
    }

    #[gpui::test]
    fn back_to_back_repeats_collapse_and_anything_between_them_breaks_the_streak(
        cx: &mut TestAppContext,
    ) {
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        for _ in 0..3 {
            sink.push(Level::Info, "waiting for a respawn");
        }
        settle(cx);
        sink.push(Level::Info, "waiting for a respawn");
        sink.push(Level::Warn, "waiting for a respawn");
        sink.push(Level::Info, "killed 1/5");
        sink.push(Level::Info, "killed 1/5");
        settle(cx);
        log.update(cx, |log, cx| {
            let ended = Outcome::Completed;
            log.run(
                "Hunt".into(),
                Event::Ended {
                    outcome: &ended,
                    took: Duration::from_secs(192),
                },
                cx,
            );
        });
        sink.push(Level::Info, "killed 1/5");
        settle(cx);

        assert_eq!(
            texts(&log, cx),
            [
                (Level::Info, "waiting for a respawn".into(), 4),
                (Level::Warn, "waiting for a respawn".into(), 1),
                (Level::Info, "killed 1/5".into(), 2),
                (Level::Info, "Hunt finished in 3m".into(), 1),
                (Level::Info, "killed 1/5".into(), 1),
            ]
        );
    }

    #[gpui::test]
    fn output_logged_before_a_run_ends_lands_above_its_end_row(cx: &mut TestAppContext) {
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        sink.push(Level::Error, "boom");
        log.update(cx, |log, cx| {
            let failed = Outcome::Failed {
                message: "boom".into(),
                stack: Some("    at default (main.ts:3:9)".into()),
            };
            log.run(
                "Hunt".into(),
                Event::Ended {
                    outcome: &failed,
                    took: Duration::ZERO,
                },
                cx,
            );
        });

        log.read_with(cx, |log, _| {
            let rows: Vec<_> = log.entries().iter().map(|entry| &entry.body).collect();
            assert_eq!(
                rows,
                [
                    &Body::Output {
                        text: "boom".into(),
                        repeats: 1
                    },
                    &Body::Run {
                        script: "Hunt".into(),
                        says: "failed".into(),
                        detail: Some("boom\n    at default (main.ts:3:9)".into()),
                        starts: false,
                    },
                ]
            );
        });
    }

    #[gpui::test]
    fn the_log_keeps_its_newest_entries_and_clear_drops_unread_output(cx: &mut TestAppContext) {
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        for line in 0..MAX_ENTRIES + 10 {
            sink.push(Level::Info, &format!("line {line}"));
        }
        settle(cx);
        log.read_with(cx, |log, _| {
            assert_eq!(log.entries().len(), MAX_ENTRIES);
            let first = log.entries().front().unwrap();
            assert_eq!(
                first.body,
                Body::Output {
                    text: "line 10".into(),
                    repeats: 1
                }
            );
            assert_eq!(log.get(first.id), Some(first));
            assert_eq!(log.get(log.next_id()), None);
        });

        let huge = "x".repeat(MAX_TEXT * 2);
        for _ in 0..MAX_BYTES / MAX_TEXT + 10 {
            sink.push(Level::Info, &huge);
            sink.push(Level::Info, "between");
        }
        settle(cx);
        log.read_with(cx, |log, _| {
            let bytes: usize = log.entries().iter().map(Entry::bytes).sum();
            assert!(bytes <= MAX_BYTES, "kept {bytes} bytes");
            assert!(
                log.entries()
                    .iter()
                    .all(|entry| entry.bytes() < MAX_TEXT + 64)
            );
        });

        sink.push(Level::Info, "unread");
        log.update(cx, |log, cx| log.clear(cx));
        settle(cx);
        assert_eq!(texts(&log, cx), []);
    }

    #[gpui::test]
    fn output_keeps_arriving_whole_after_the_byte_budget_has_turned_over(cx: &mut TestAppContext) {
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        let huge = "x".repeat(MAX_TEXT);
        for _ in 0..3 {
            for _ in 0..MAX_BYTES / MAX_TEXT / 2 {
                sink.push(Level::Info, &huge);
                sink.push(Level::Info, "between");
            }
            settle(cx);
        }
        let line = "y".repeat(1024);
        for n in 0..100 {
            sink.push(Level::Info, &format!("after {n} {line}"));
        }
        settle(cx);
        let after = texts(&log, cx)
            .into_iter()
            .filter(|(_, text, _)| text.starts_with("after"))
            .count();
        assert_eq!(after, 100);
    }
}
