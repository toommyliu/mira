use std::{ops::Range, rc::Rc};

use chrono::{DateTime, Local};
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use mira_ui::{
    ActiveTheme as _,
    components::{Empty, LogBody, LogLevel, LogRow, LogState, LogView},
    primitives::{Button, Input, InputEvent, button::Variant},
    v_flex,
};

use super::script_log::{Body, Entry, EntryId, ScriptLog};
use crate::{clock, scripting::Level};

pub(super) struct LogPage {
    search: Entity<Input>,
    log: Option<Entity<ScriptLog>>,
    state: LogState,
    shown: Rc<Shown>,
    seen: EntryId,
    cleared: u64,
    behind: bool,
    _log_changes: Option<Subscription>,
    _search: Subscription,
}

#[derive(Clone, Debug, PartialEq)]
enum Row {
    Entry(Entry),
    Day {
        before: EntryId,
        at: DateTime<Local>,
    },
}

impl Row {
    fn id(&self) -> EntryId {
        match self {
            Row::Entry(entry) => entry.id,
            Row::Day { before, .. } => *before,
        }
    }

    fn at(&self) -> DateTime<Local> {
        match self {
            Row::Entry(entry) => entry.at,
            Row::Day { at, .. } => *at,
        }
    }

    fn opens(&self) -> bool {
        match self {
            Row::Entry(entry) => matches!(entry.body, Body::Run { starts: true, .. }),
            Row::Day { .. } => true,
        }
    }
}

#[derive(Clone, Default)]
struct Shown {
    rows: Vec<Row>,
    sections: Vec<usize>,
}

impl Shown {
    fn push(&mut self, entry: &Entry) {
        if self
            .rows
            .last()
            .is_some_and(|row| row.at().date_naive() != entry.at.date_naive())
        {
            self.add(Row::Day {
                before: entry.id,
                at: entry.at,
            });
        }
        self.add(Row::Entry(entry.clone()));
    }

    fn add(&mut self, row: Row) {
        if row.opens() {
            self.sections.push(self.rows.len());
        }
        self.rows.push(row);
    }

    fn drop_before(&mut self, first: EntryId) -> usize {
        let mut dropped = self.rows.partition_point(|row| row.id() < first);
        if matches!(self.rows.get(dropped), Some(Row::Day { .. })) {
            dropped += 1;
        }
        self.rows.drain(..dropped);
        self.sections
            .retain_mut(|at| match at.checked_sub(dropped) {
                Some(moved) => {
                    *at = moved;
                    true
                }
                None => false,
            });
        dropped
    }

    fn section_of(&self, ix: usize) -> Option<usize> {
        let at = self.sections.partition_point(|&start| start <= ix);
        at.checked_sub(1).map(|at| self.sections[at])
    }
}

impl LogPage {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            Input::new("Search output", cx)
                .placeholder("Search output")
                .search()
        });
        let changes = cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Changed) {
                this.rebuild(cx);
            }
        });
        Self {
            search,
            log: None,
            state: LogState::default(),
            shown: Rc::default(),
            seen: EntryId::default(),
            cleared: 0,
            behind: false,
            _log_changes: None,
            _search: changes,
        }
    }

    pub(super) fn show(&mut self, log: Option<Entity<ScriptLog>>, cx: &mut Context<Self>) {
        if self.log == log {
            return;
        }
        self._log_changes = log
            .as_ref()
            .map(|log| cx.observe(log, |this, _, cx| this.sync(cx)));
        self.log = log;
        self.rebuild(cx);
    }

    pub(super) fn search(&self) -> &Entity<Input> {
        &self.search
    }

    fn query(&self, cx: &Context<Self>) -> String {
        self.search.read(cx).text().trim().to_lowercase()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let mut shown = Shown::default();
        if let Some(log) = &self.log {
            let log = log.read(cx);
            for entry in log.entries().iter().filter(|entry| entry.matches(&query)) {
                shown.push(entry);
            }
            self.seen = log.next_id();
            self.cleared = log.cleared();
        }
        self.behind = false;
        self.state.reset(shown.rows.len());
        self.shown = Rc::new(shown);
        cx.notify();
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(log) = &self.log else {
            return;
        };
        if log.read(cx).cleared() != self.cleared {
            self.rebuild(cx);
        } else if self.state.following() {
            self.catch_up(cx);
        } else {
            self.behind = true;
        }
    }

    fn catch_up(&mut self, cx: &mut Context<Self>) {
        let Some(log) = self.log.clone() else {
            return;
        };
        let query = self.query(cx);
        let log = log.read(cx);
        let shown = Rc::make_mut(&mut self.shown);

        let first = log
            .entries()
            .front()
            .map_or(log.next_id(), |entry| entry.id);
        let dropped = shown.drop_before(first);
        if dropped > 0 {
            self.state.splice(0..dropped, 0);
        }

        let last = shown.rows.len().saturating_sub(1);
        if let Some(Row::Entry(row)) = shown.rows.last_mut()
            && let Some(entry) = log.get(row.id)
            && entry != row
        {
            *row = entry.clone();
            self.state.splice(last..last + 1, 1);
        }

        let unseen = log.entries().partition_point(|entry| entry.id < self.seen);
        let start = shown.rows.len();
        for entry in log.entries().range(unseen..) {
            if entry.matches(&query) {
                shown.push(entry);
            }
        }
        if shown.rows.len() > start {
            self.state.splice(start..start, shown.rows.len() - start);
        }
        self.seen = log.next_id();
        self.behind = false;
        cx.notify();
    }

    fn copied(&self, range: Range<usize>) -> Option<ClipboardItem> {
        let text = self
            .shown
            .rows
            .get(range)?
            .iter()
            .map(copied)
            .collect::<Vec<_>>()
            .join("\n");
        Some(ClipboardItem::new_string(text))
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if let Some(log) = &self.log {
            log.update(cx, |log, cx| log.clear(cx));
        }
    }
}

fn level(level: Level) -> LogLevel {
    match level {
        Level::Debug => LogLevel::Debug,
        Level::Info => LogLevel::Info,
        Level::Warn => LogLevel::Warn,
        Level::Error => LogLevel::Error,
    }
}

fn row(row: &Row) -> LogRow {
    let entry = match row {
        Row::Entry(entry) => entry,
        Row::Day { at, .. } => {
            return LogRow {
                time: SharedString::default(),
                level: LogLevel::Info,
                body: LogBody::Divider(clock::day(*at).into()),
                repeats: 1,
                opens_section: true,
            };
        }
    };
    let time = clock::aligned_time(entry.at).into();
    let level = level(entry.level);
    match &entry.body {
        Body::Output { text, repeats } => LogRow {
            time,
            level,
            body: LogBody::Text(text.clone()),
            repeats: *repeats,
            opens_section: false,
        },
        Body::Run {
            script,
            says,
            detail,
            starts,
        } => LogRow {
            time,
            level,
            body: LogBody::Text(match detail {
                Some(detail) => format!("{script} {says}\n{detail}").into(),
                None => format!("{script} {says}").into(),
            }),
            repeats: 1,
            opens_section: *starts,
        },
    }
}

fn copied(row: &Row) -> String {
    let entry = match row {
        Row::Entry(entry) => entry,
        Row::Day { at, .. } => return clock::day(*at),
    };
    let tag = match entry.level {
        Level::Debug => "DEBUG ",
        Level::Info => "",
        Level::Warn => "WARN ",
        Level::Error => "ERROR ",
    };
    let time = clock::time(entry.at);
    match &entry.body {
        Body::Output { text, repeats: 1 } => format!("{time} {tag}{text}"),
        Body::Output { text, repeats } => format!("{time} {tag}{text} (×{repeats})"),
        Body::Run {
            script,
            says,
            detail: None,
            ..
        } => format!("{time} {tag}{script} {says}"),
        Body::Run {
            script,
            says,
            detail: Some(detail),
            ..
        } => format!("{time} {tag}{script} {says}\n{detail}"),
    }
}

impl Render for LogPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.behind && self.state.following() {
            self.catch_up(cx);
        }
        let theme = cx.theme().clone();
        let empty_log = self
            .log
            .as_ref()
            .is_none_or(|log| log.read(cx).entries().is_empty());
        let toolbar = theme
            .toolbar()
            .child(div().w(px(240.0)).child(self.search.clone()))
            .child(div().flex_1())
            .child(
                Button::new("log-copy", "Copy")
                    .variant(Variant::Ghost)
                    .disabled(self.shown.rows.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(item) = this.copied(0..this.shown.rows.len()) {
                            cx.write_to_clipboard(item);
                        }
                    })),
            )
            .child(
                Button::new("log-clear", "Clear")
                    .variant(Variant::Ghost)
                    .disabled(empty_log)
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            );
        let empty = match &self.log {
            None => Some(Empty::new(
                "No tabs open",
                "Open a tab to see what its scripts print.",
            )),
            Some(_) if empty_log => Some(Empty::new(
                "No script output yet",
                "What a script prints on this tab shows here.",
            )),
            Some(_) if self.shown.rows.is_empty() => Some(Empty::new(
                "No matching output",
                "Nothing in the log matches the search.",
            )),
            Some(_) => None,
        };
        let body = match (&self.log, empty) {
            (_, Some(empty)) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(empty)
                .into_any_element(),
            (None, None) => div().into_any_element(),
            (Some(_), None) => {
                let (rows, sections) = (self.shown.clone(), self.shown.clone());
                let this = cx.weak_entity();
                LogView::new("script-log", "Script output", self.state.clone(), {
                    move |ix, _| match rows.rows.get(ix) {
                        Some(shown) => row(shown),
                        None => LogRow {
                            time: SharedString::default(),
                            level: LogLevel::Info,
                            body: LogBody::Text(SharedString::default()),
                            repeats: 1,
                            opens_section: false,
                        },
                    }
                })
                .section_of(move |ix| sections.section_of(ix))
                .on_copy(move |range, cx| {
                    let copied = this.upgrade().and_then(|this| this.read(cx).copied(range));
                    if let Some(item) = copied {
                        cx.write_to_clipboard(item);
                    }
                })
                .into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(toolbar)
            .child(div().flex_1().min_h_0().flex().child(body))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::TestAppContext;

    use super::super::script_log::{Event, Sink};
    use super::*;
    use crate::scripting::Outcome;

    fn page_for(log: &Entity<ScriptLog>, cx: &mut TestAppContext) -> Entity<LogPage> {
        let log = log.clone();
        cx.new(|cx| {
            let mut page = LogPage::new(cx);
            page.show(Some(log), cx);
            page
        })
    }

    fn copied_all(page: &Entity<LogPage>, cx: &TestAppContext) -> String {
        page.read_with(cx, |page, _| {
            page.copied(0..page.shown.rows.len())
                .and_then(|item| item.text())
                .unwrap_or_default()
        })
    }

    fn rows(page: &Entity<LogPage>, cx: &TestAppContext) -> usize {
        page.read_with(cx, |page, _| page.state.len())
    }

    fn search(page: &Entity<LogPage>, query: &str, cx: &mut TestAppContext) {
        let input = page.read_with(cx, |page, _| page.search.clone());
        input.update(cx, |input, cx| input.set_text(query, cx));
    }

    fn hunt(log: &Entity<ScriptLog>, sink: &Sink, rounds: usize, cx: &mut TestAppContext) {
        for round in 0..rounds {
            log.update(cx, |log, cx| {
                log.run("Hunt".into(), Event::Started { place: None }, cx)
            });
            for kill in 0..40 {
                sink.push(Level::Info, &format!("round {round} killed {kill}"));
                sink.push(Level::Info, "waiting for a respawn");
                sink.push(Level::Info, "waiting for a respawn");
            }
            cx.run_until_parked();
            log.update(cx, |log, cx| {
                let ended = Outcome::Completed;
                log.run(
                    "Hunt".into(),
                    Event::Ended {
                        outcome: &ended,
                        took: Duration::from_secs(61),
                    },
                    cx,
                )
            });
        }
    }

    #[gpui::test]
    fn a_page_watching_output_arrive_shows_what_a_fresh_page_would(cx: &mut TestAppContext) {
        cx.update(mira_ui::init);
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        let watching = page_for(&log, cx);
        let searching = page_for(&log, cx);
        search(&searching, "Round 3", cx);

        hunt(&log, &sink, 70, cx);

        let entries = log.read_with(cx, |log, _| log.entries().len());
        assert!(entries < 70 * 82, "the log never dropped its oldest rows");
        let fresh = page_for(&log, cx);
        assert_eq!(rows(&watching, cx), entries);
        assert_eq!(copied_all(&watching, cx), copied_all(&fresh, cx));
        search(&fresh, "round 3", cx);
        let found = copied_all(&searching, cx);
        assert_eq!(rows(&searching, cx), rows(&fresh, cx));
        assert_eq!(found, copied_all(&fresh, cx));
        assert!(found.lines().all(|line| line.contains("round 3")));
        assert!(found.contains("round 37 killed 39"));
    }

    #[gpui::test]
    fn copying_writes_levels_repeats_and_run_details_as_text(cx: &mut TestAppContext) {
        cx.update(mira_ui::init);
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        let page = page_for(&log, cx);
        log.update(cx, |log, cx| {
            log.run(
                "Hunt".into(),
                Event::Started {
                    place: Some((1, 2)),
                },
                cx,
            )
        });
        sink.push(Level::Info, "waiting for a respawn");
        sink.push(Level::Info, "waiting for a respawn");
        sink.push(Level::Debug, "target 2");
        sink.push(Level::Warn, "fell behind");
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
            )
        });

        let copied = copied_all(&page, cx);
        let times: Vec<String> = page.read_with(cx, |page, _| {
            page.shown
                .rows
                .iter()
                .map(|row| format!("{} ", clock::time(row.at())))
                .collect()
        });
        let untimed: Vec<&str> = copied
            .lines()
            .map(|line| {
                times
                    .iter()
                    .find_map(|time| line.strip_prefix(time.as_str()))
                    .unwrap_or(line)
            })
            .collect();
        assert_eq!(
            untimed,
            [
                "Hunt started · 1 of 2 in the queue",
                "waiting for a respawn (×2)",
                "DEBUG target 2",
                "WARN fell behind",
                "ERROR Hunt failed",
                "boom",
                "    at default (main.ts:3:9)",
            ]
        );

        log.update(cx, |log, cx| log.clear(cx));
        assert_eq!(copied_all(&page, cx), "");
    }

    #[gpui::test]
    fn scrolling_away_holds_the_rows_until_the_reader_jumps_back(cx: &mut TestAppContext) {
        use gpui::{ScrollDelta, ScrollEvent, point, px};
        cx.update(mira_ui::init);
        let log = cx.new(ScriptLog::new);
        let sink = log.read_with(cx, |log, _| log.sink());
        hunt(&log, &sink, 10, cx);
        let (page, cx) = cx.add_window_view({
            let log = log.clone();
            move |_, cx| {
                let mut page = LogPage::new(cx);
                page.show(Some(log), cx);
                page
            }
        });
        cx.run_until_parked();
        let size = cx.update(|window, _| window.viewport_size());
        cx.simulate_event(ScrollEvent {
            position: point(size.width / 2.0, size.height / 2.0),
            delta: ScrollDelta::Pixels(point(px(0.0), px(300.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        let held = copied_all(&page, cx);

        hunt(&log, &sink, 70, cx);
        assert_eq!(
            copied_all(&page, cx),
            held,
            "the rows moved while scrolled away"
        );

        page.update(cx, |page, _| page.state.jump_to_latest());
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let fresh = page_for(&log, cx);
        assert_eq!(copied_all(&page, cx), copied_all(&fresh, cx));
        assert_eq!(rows(&page, cx), rows(&fresh, cx));
    }

    #[gpui::test]
    fn a_run_past_midnight_gets_a_divider_only_where_shown_rows_change_day(
        cx: &mut TestAppContext,
    ) {
        use chrono::TimeZone as _;
        cx.update(mira_ui::init);
        let log = cx.new(ScriptLog::new);
        let page = page_for(&log, cx);
        let late = Local.with_ymd_and_hms(2026, 11, 23, 23, 59, 59).unwrap();
        let early = Local.with_ymd_and_hms(2026, 11, 24, 0, 0, 1).unwrap();
        log.update(cx, |log, cx| {
            log.record(late, Level::Info, "killed 41/50", cx);
            log.record(late, Level::Info, "waiting for a respawn", cx);
            log.record(early, Level::Info, "waiting for a respawn", cx);
            log.record(early, Level::Info, "killed 42/50", cx);
            log.record(early, Level::Warn, "low on potions", cx);
        });

        let copied = copied_all(&page, cx);
        assert_eq!(
            copied.lines().collect::<Vec<_>>(),
            [
                format!("{} killed 41/50", clock::time(late)),
                format!("{} waiting for a respawn", clock::time(late)),
                clock::day(early),
                format!("{} waiting for a respawn", clock::time(early)),
                format!("{} killed 42/50", clock::time(early)),
                format!("{} WARN low on potions", clock::time(early)),
            ]
        );
        assert_eq!(copied, copied_all(&page_for(&log, cx), cx));
        let one_side = page_for(&log, cx);
        search(&one_side, "42/50", cx);
        assert_eq!(copied_all(&one_side, cx).lines().count(), 1);

        log.update(cx, |log, cx| {
            for kill in 0..5_000 {
                log.record(early, Level::Info, &format!("killed {kill}"), cx);
            }
        });
        let kept = copied_all(&page, cx);
        assert!(
            !kept.contains(&clock::day(early)),
            "the day-1 rows left a divider behind"
        );
        assert_eq!(kept, copied_all(&page_for(&log, cx), cx));
    }
}
