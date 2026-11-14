use gpui::SharedString;

use super::{Login, ScriptPhase, ScriptState, Summary, scripts::home_relative};
use crate::accounts::Account;

pub(super) enum Presence {
    Open(Summary),
    Waiting {
        place: usize,
        of: usize,
        open: Option<Summary>,
    },
    Unopened(SharedString),
    Closed(Option<ScriptState>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Show {
    All,
    Running,
    Problems,
    NotOpen,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mood {
    Quiet,
    Muted,
    Plain,
    Accent,
    Warning,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fix {
    EditLogin,
    Restart,
}

pub(super) struct Line {
    pub(super) text: SharedString,
    pub(super) mood: Mood,
    pub(super) path: Option<SharedString>,
}

impl Line {
    pub(super) fn new(text: impl Into<SharedString>, mood: Mood) -> Self {
        Self {
            text: text.into(),
            mood,
            path: None,
        }
    }

    pub(super) fn path(mut self, path: &std::path::Path) -> Self {
        self.path = Some(home_relative(path));
        self
    }
}

pub(super) struct Face {
    pub(super) state: Line,
    pub(super) place: Line,
    pub(super) who: Option<SharedString>,
    pub(super) script: Option<Line>,
    pub(super) plan: Option<Line>,
    pub(super) last: Option<Line>,
    pub(super) fix: Option<Fix>,
    pub(super) group: Show,
    pub(super) open: bool,
}

impl Face {
    pub(super) fn note(&self) -> Option<&Line> {
        self.script
            .as_ref()
            .or(self.plan.as_ref())
            .or(self.last.as_ref())
    }

    pub(super) fn lines(&self) -> usize {
        1 + usize::from(self.who.is_some()) + usize::from(self.note().is_some())
    }

    pub(super) fn shown_in(&self, show: Show) -> bool {
        show == Show::All || show == self.group
    }

    pub(super) fn matches(&self, account: &Account, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        [
            account.name.as_str(),
            account.username.as_str(),
            &self.state.text,
            &self.place.text,
            self.who.as_deref().unwrap_or_default(),
            self.note().map_or("", |line| &line.text),
        ]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
    }
}

pub(super) fn face(account: &Account, presence: &Presence) -> Face {
    let saved = Line::new(
        account
            .server
            .clone()
            .unwrap_or_else(|| "Any server".into()),
        Mood::Quiet,
    );
    match presence {
        Presence::Open(summary) => open(summary, saved),
        Presence::Waiting { place, of, open } => Face {
            state: Line::new(format!("Waiting {place}/{of}"), Mood::Muted),
            place: saved,
            who: None,
            script: active(open.as_ref().and_then(|summary| summary.script.as_ref())),
            plan: None,
            last: settled(open.as_ref().and_then(|summary| summary.script.as_ref())),
            fix: None,
            group: Show::NotOpen,
            open: false,
        },
        Presence::Unopened(error) => Face {
            state: Line::new("Couldn't open", Mood::Danger),
            place: saved,
            who: None,
            script: Some(Line::new(error.clone(), Mood::Danger)),
            plan: None,
            last: None,
            fix: None,
            group: Show::Problems,
            open: false,
        },
        Presence::Closed(last) => Face {
            state: Line::new("Not open", Mood::Quiet),
            place: saved,
            who: None,
            script: active(last.as_ref()),
            plan: None,
            last: settled(last.as_ref()),
            fix: None,
            group: Show::NotOpen,
            open: false,
        },
    }
}

fn open(summary: &Summary, saved: Line) -> Face {
    let place = match &summary.place {
        Some(place) => Line::new(
            [&place.server, &place.area, &place.cell]
                .into_iter()
                .flatten()
                .map(SharedString::as_ref)
                .collect::<Vec<_>>()
                .join(" · "),
            Mood::Plain,
        ),
        None => saved,
    };
    let who = summary.character.as_ref().and_then(|character| {
        let parts: Vec<String> = [
            character.level.map(|level| format!("Lv {level}")),
            character.class.as_ref().map(ToString::to_string),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(" · ").into())
    });
    let script = active(summary.script.as_ref());
    let last = settled(summary.script.as_ref());
    let face = |state: Line, group: Show, script: Option<Line>, fix: Option<Fix>| Face {
        state,
        place,
        who,
        script,
        plan: None,
        last,
        fix,
        group,
        open: true,
    };
    match &summary.login {
        Login::Loading(Some(percent)) => face(
            Line::new(format!("Loading {percent}%"), Mood::Muted),
            Show::All,
            script,
            None,
        ),
        Login::Loading(None) => face(Line::new("Loading", Mood::Muted), Show::All, script, None),
        Login::LoggingIn { attempt: 1, .. } => face(
            Line::new("Logging in", Mood::Accent),
            Show::All,
            script,
            None,
        ),
        Login::LoggingIn { attempt, of } => face(
            Line::new(format!("Logging in {attempt}/{of}"), Mood::Accent),
            Show::All,
            script,
            None,
        ),
        Login::Retrying { attempt, of } => face(
            Line::new(format!("Retrying {attempt}/{of}"), Mood::Warning),
            Show::All,
            script,
            None,
        ),
        Login::Reconnecting => face(
            Line::new("Reconnecting", Mood::Warning),
            Show::All,
            script,
            None,
        ),
        Login::ChooseServer => face(
            Line::new("Choose server", Mood::Warning),
            Show::Problems,
            script,
            None,
        ),
        Login::Disconnected => face(
            Line::new("Disconnected", Mood::Warning),
            Show::Problems,
            script,
            None,
        ),
        Login::Failed(error) => face(
            Line::new("Login failed", Mood::Danger),
            Show::Problems,
            Some(Line::new(error.clone(), Mood::Danger)),
            Some(Fix::EditLogin),
        ),
        Login::Broken(error) => face(
            Line::new("Couldn't start", Mood::Danger),
            Show::Problems,
            Some(Line::new(error.clone(), Mood::Danger)),
            None,
        ),
        Login::Online => {
            let phase = summary.script.as_ref().map(|script| &script.phase);
            let (state, group, fix) = match phase {
                Some(ScriptPhase::Running) => {
                    (Line::new("Running", Mood::Accent), Show::Running, None)
                }
                Some(ScriptPhase::Stopping) => {
                    (Line::new("Stopping", Mood::Muted), Show::Running, None)
                }
                Some(ScriptPhase::NeedsInput) => (
                    Line::new("Needs input", Mood::Warning),
                    Show::Problems,
                    None,
                ),
                Some(ScriptPhase::Failed(_)) => (
                    Line::new("Script failed", Mood::Danger),
                    Show::Problems,
                    Some(Fix::Restart),
                ),
                Some(ScriptPhase::Queued | ScriptPhase::Finished | ScriptPhase::Stopped) | None => {
                    (Line::new("Online", Mood::Muted), Show::All, None)
                }
            };
            face(state, group, script, fix)
        }
    }
}

fn active(script: Option<&ScriptState>) -> Option<Line> {
    script.filter(|script| !is_settled(script)).map(script_line)
}

fn settled(script: Option<&ScriptState>) -> Option<Line> {
    script.filter(|script| is_settled(script)).map(script_line)
}

fn is_settled(script: &ScriptState) -> bool {
    matches!(script.phase, ScriptPhase::Finished | ScriptPhase::Stopped)
}

fn script_line(script: &ScriptState) -> Line {
    let name = &script.name;
    let line = match &script.phase {
        ScriptPhase::Queued => Line::new(format!("Waiting to run {name}"), Mood::Muted),
        ScriptPhase::Running => Line::new(name.clone(), Mood::Plain),
        ScriptPhase::NeedsInput => Line::new(format!("{name} is waiting for you"), Mood::Warning),
        ScriptPhase::Stopping => Line::new(format!("Stopping {name}"), Mood::Muted),
        ScriptPhase::Finished => Line::new(format!("{name} finished"), Mood::Muted),
        ScriptPhase::Stopped => Line::new(format!("{name} stopped"), Mood::Muted),
        ScriptPhase::Failed(message) => {
            Line::new(format!("{name} failed: {message}"), Mood::Danger)
        }
    };
    line.path(&script.entry)
}
