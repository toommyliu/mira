use std::{
    path::Path,
    time::{Duration, Instant},
};

use gpui::SharedString;

use super::{Login, ScriptPhase, ScriptState, Summary, capitalized, scripts::home_relative};
use crate::accounts::Account;

#[derive(Clone, Debug)]
pub(super) enum Presence {
    Open(Summary),
    Waiting {
        script: Option<ScriptState>,
        open: Option<Summary>,
    },
    Unopened(SharedString),
    Closed(Option<ScriptState>),
}

impl Presence {
    pub(super) fn live(&self) -> Option<&Summary> {
        match self {
            Presence::Open(summary)
            | Presence::Waiting {
                open: Some(summary),
                ..
            } => Some(summary),
            _ => None,
        }
    }

    pub(super) fn running(&self) -> Option<&Path> {
        let script = self.live()?.script.as_ref()?;
        script.running().then_some(script.entry.as_path())
    }

    pub(super) fn queued(&self) -> Option<&Path> {
        if let Presence::Waiting {
            script: Some(script),
            ..
        } = self
        {
            return Some(&script.entry);
        }
        match self.live()?.script.as_ref()? {
            ScriptState {
                entry,
                phase: ScriptPhase::Queued,
                ..
            } => Some(entry),
            _ => None,
        }
    }

    pub(super) fn signed_out(&self) -> bool {
        self.live()
            .is_some_and(|summary| matches!(summary.login, Login::Disconnected | Login::Failed(_)))
    }

    pub(super) fn opened(&self) -> bool {
        matches!(self, Presence::Open(_) | Presence::Waiting { .. })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Status {
    NotOpen,
    Starting,
    Running,
    Idle,
    NeedsYou,
    Failed,
    Done,
}

impl Status {
    pub(super) fn label(self) -> &'static str {
        match self {
            Status::NotOpen => "Not open",
            Status::Starting => "Starting",
            Status::Running => "Running",
            Status::Idle => "Idle",
            Status::NeedsYou => "Needs you",
            Status::Failed => "Failed",
            Status::Done => "Done",
        }
    }

    pub(super) fn needs_you(self) -> bool {
        matches!(self, Status::NeedsYou | Status::Failed)
    }
}

pub(super) fn status(presence: &Presence, done: bool) -> Status {
    let status = match presence {
        Presence::Waiting { .. } => Status::Starting,
        Presence::Unopened(_) => Status::Failed,
        Presence::Closed(_) => Status::NotOpen,
        Presence::Open(summary) => match &summary.login {
            Login::Loading(_)
            | Login::LoggingIn { .. }
            | Login::Retrying { .. }
            | Login::Reconnecting => Status::Starting,
            Login::ChooseServer | Login::Disconnected => Status::NeedsYou,
            Login::Failed(_) | Login::Broken(_) => Status::Failed,
            Login::Online => match summary.script.as_ref().map(|script| &script.phase) {
                Some(ScriptPhase::Running | ScriptPhase::Stopping) => Status::Running,
                Some(ScriptPhase::NeedsInput) => Status::NeedsYou,
                Some(ScriptPhase::Failed(_)) => Status::Failed,
                Some(ScriptPhase::Queued) => Status::Starting,
                Some(ScriptPhase::Finished | ScriptPhase::Stopped) | None => Status::Idle,
            },
        },
    };
    match status {
        Status::Idle | Status::NotOpen if done => Status::Done,
        status => status,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mood {
    Quiet,
    Plain,
    Warning,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Action {
    EditLogin,
    Restart,
    LogIn,
    Show,
    Stop,
    Launch,
}

impl Action {
    pub(super) fn label(self) -> &'static str {
        match self {
            Action::EditLogin => "Edit login",
            Action::Restart => "Restart",
            Action::LogIn => "Log in",
            Action::Show => "Show",
            Action::Stop => "Stop",
            Action::Launch => "Launch",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Line {
    pub(super) text: SharedString,
    pub(super) mood: Mood,
    pub(super) path: Option<SharedString>,
    pub(super) detail: Option<(SharedString, SharedString)>,
    pub(super) time: Option<SharedString>,
    about: About,
}

impl Line {
    fn new(text: impl Into<SharedString>, mood: Mood) -> Self {
        Self {
            text: text.into(),
            mood,
            path: None,
            detail: None,
            time: None,
            about: About::Login,
        }
    }

    fn error(
        text: impl Into<SharedString>,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
    ) -> Self {
        Self {
            detail: Some((title.into(), message.into())),
            ..Self::new(text, Mood::Danger)
        }
    }

    fn path(mut self, path: &Path) -> Self {
        self.path = Some(home_relative(path));
        self.about = About::Script;
        self
    }

    fn time(mut self, time: Option<String>) -> Self {
        self.time = time.map(Into::into);
        self
    }
}

#[derive(Clone, Debug)]
pub(super) enum Job {
    Loading(Option<u8>),
    Line(Line),
}

#[derive(Clone, Debug)]
pub(super) struct Card {
    pub(super) status: Status,
    pub(super) state: SharedString,
    pub(super) job: Job,
    pub(super) place: Line,
    pub(super) who: Option<SharedString>,
    pub(super) action: Option<Action>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum About {
    Script,
    Login,
}

impl Card {
    pub(super) fn about(&self) -> About {
        match &self.job {
            Job::Line(line) => line.about,
            Job::Loading(_) => About::Login,
        }
    }

    pub(super) fn matches(&self, account: &Account, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let job = match &self.job {
            Job::Line(line) => line.text.as_ref(),
            Job::Loading(_) => "",
        };
        [
            account.name.as_str(),
            account.username.as_str(),
            self.status.label(),
            &self.state,
            &self.place.text,
            self.who.as_deref().unwrap_or_default(),
            job,
        ]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
    }
}

pub(super) fn card(account: &Account, presence: &Presence, done: bool, now: Instant) -> Card {
    let status = status(presence, done);
    let saved = Line::new(account.server.clone().unwrap_or_default(), Mood::Quiet);
    let closed = |state: &str, job: Job, action| Card {
        status,
        state: state.to_owned().into(),
        job,
        place: saved.clone(),
        who: None,
        action,
    };
    match presence {
        Presence::Open(summary) => open(summary, status, saved, now),
        Presence::Waiting { script, .. } => closed(
            "Queued",
            Job::Line(match script {
                Some(script) => Line::new(script.name.clone(), Mood::Quiet).path(&script.entry),
                None => Line::new("", Mood::Quiet),
            }),
            None,
        ),
        Presence::Unopened(error) => closed(
            "Couldn't open",
            Job::Line(Line::error(error.clone(), "Couldn't open", error.clone())),
            None,
        ),
        Presence::Closed(last) => closed(
            "Not open",
            last.as_ref().map_or_else(
                || Job::Line(Line::new("", Mood::Quiet)),
                |last| Job::Line(ended(last, now)),
            ),
            Some(Action::Launch),
        ),
    }
}

fn open(summary: &Summary, status: Status, saved: Line, now: Instant) -> Card {
    let place = match &summary.place {
        Some(place) => Line::new(
            [&place.server, &place.area]
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
    let line = |text: String, mood| Job::Line(Line::new(text, mood));
    let (state, job, action): (String, Job, Option<Action>) = match &summary.login {
        Login::Loading(percent) => (
            percent.map_or_else(
                || "Loading".to_owned(),
                |percent| format!("Loading {percent}%"),
            ),
            Job::Loading(*percent),
            None,
        ),
        Login::LoggingIn { attempt: 1, .. } => (
            "Logging in".into(),
            line("Logging in".into(), Mood::Quiet),
            None,
        ),
        Login::LoggingIn { attempt, of } => (
            format!("Logging in {attempt}/{of}"),
            line(
                format!("Logging in, attempt {attempt} of {of}"),
                Mood::Quiet,
            ),
            None,
        ),
        Login::Retrying { attempt, of } => (
            format!("Retrying {attempt}/{of}"),
            line(format!("Retrying, attempt {attempt} of {of}"), Mood::Quiet),
            None,
        ),
        Login::Reconnecting => (
            "Reconnecting".into(),
            line("Lost connection, logging back in".into(), Mood::Quiet),
            None,
        ),
        Login::ChooseServer => (
            "Choose server".into(),
            line("Pick a server in the game".into(), Mood::Warning),
            Some(Action::Show),
        ),
        Login::Disconnected => (
            "Disconnected".into(),
            line(
                "Lost connection, and auto relogin is off".into(),
                Mood::Warning,
            ),
            Some(Action::LogIn),
        ),
        Login::Failed(error) => (
            "Login failed".into(),
            Job::Line(Line::error(
                capitalized(error),
                "Login failed",
                capitalized(error),
            )),
            Some(Action::EditLogin),
        ),
        Login::Broken(error) => (
            "Couldn't start".into(),
            Job::Line(Line::error(
                capitalized(error),
                "Couldn't start",
                capitalized(error),
            )),
            None,
        ),
        Login::Online => match &summary.script {
            Some(script) => {
                let name = &script.name;
                let text =
                    |text: String, mood| Job::Line(Line::new(text, mood).path(&script.entry));
                match &script.phase {
                    ScriptPhase::Running => (
                        "Running".into(),
                        Job::Line(
                            Line::new(name.clone(), Mood::Plain)
                                .path(&script.entry)
                                .time(script.since.map(|since| span(now - since))),
                        ),
                        Some(Action::Stop),
                    ),
                    ScriptPhase::NeedsInput => (
                        "Needs input".into(),
                        text(format!("{name} is waiting for you"), Mood::Warning),
                        Some(Action::Show),
                    ),
                    ScriptPhase::Stopping => (
                        "Stopping".into(),
                        text(format!("Stopping {name}"), Mood::Quiet),
                        None,
                    ),
                    ScriptPhase::Failed(message) => (
                        "Script failed".into(),
                        Job::Line(
                            Line::error(
                                capitalized(message),
                                format!("{name} failed"),
                                capitalized(message),
                            )
                            .path(&script.entry),
                        ),
                        Some(Action::Restart),
                    ),
                    ScriptPhase::Queued => {
                        ("Queued".into(), text(name.to_string(), Mood::Quiet), None)
                    }
                    ScriptPhase::Finished | ScriptPhase::Stopped => (
                        "Online".into(),
                        Job::Line(ended(script, now)),
                        Some(Action::Show),
                    ),
                }
            }
            None => (
                "Online".into(),
                Job::Line(Line {
                    about: About::Script,
                    ..Line::new("No script", Mood::Quiet)
                }),
                Some(Action::Show),
            ),
        },
    };
    Card {
        status,
        state: state.into(),
        job,
        place,
        who,
        action,
    }
}

fn ended(script: &ScriptState, now: Instant) -> Line {
    let name = &script.name;
    let ago = script.since.map(|since| ago(now - since));
    let (text, mood) = match &script.phase {
        ScriptPhase::Finished => (format!("{name} finished"), Mood::Quiet),
        ScriptPhase::Failed(message) => {
            return Line::error(
                format!("{name} failed: {message}"),
                format!("{name} failed"),
                capitalized(message),
            )
            .path(&script.entry);
        }
        _ => (format!("{name} stopped"), Mood::Quiet),
    };
    let text = match ago {
        Some(ago) if mood == Mood::Quiet => format!("{text} {ago}"),
        _ => text,
    };
    Line::new(text, mood).path(&script.entry)
}

pub(super) fn span(elapsed: Duration) -> String {
    match elapsed.as_secs() / 60 {
        0 => "<1m".to_owned(),
        minutes @ 1..60 => format!("{minutes}m"),
        minutes => format!("{}h {}m", minutes / 60, minutes % 60),
    }
}

pub(super) fn ago(elapsed: Duration) -> String {
    match elapsed.as_secs() / 60 {
        0 => "just now".to_owned(),
        minutes @ 1..60 => format!("{minutes}m ago"),
        minutes => format!("{}h ago", minutes / 60),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::ruffle_view::account::file_name;

    pub(in super::super) fn online(entry: &str, phase: ScriptPhase) -> Presence {
        Presence::Open(Summary {
            login: Login::Online,
            place: None,
            character: None,
            script: Some(ScriptState {
                entry: entry.into(),
                name: file_name(Path::new(entry)).into(),
                phase,
                since: None,
            }),
        })
    }

    pub(in super::super) fn queued(entry: &str) -> ScriptState {
        ScriptState {
            entry: entry.into(),
            name: file_name(Path::new(entry)).into(),
            phase: ScriptPhase::Queued,
            since: None,
        }
    }

    pub(in super::super) fn signing_in(login: Login) -> Presence {
        Presence::Open(Summary {
            login,
            place: None,
            character: None,
            script: None,
        })
    }

    #[test]
    fn every_session_state_reads_as_one_of_seven_words() {
        let failed = ScriptPhase::Failed("Bank is full".into());
        let cases = [
            (Presence::Closed(None), Status::NotOpen),
            (
                Presence::Waiting {
                    script: None,
                    open: None,
                },
                Status::Starting,
            ),
            (Presence::Unopened("No player".into()), Status::Failed),
            (signing_in(Login::Loading(Some(40))), Status::Starting),
            (
                signing_in(Login::LoggingIn { attempt: 2, of: 3 }),
                Status::Starting,
            ),
            (
                signing_in(Login::Retrying { attempt: 1, of: 3 }),
                Status::Starting,
            ),
            (signing_in(Login::Reconnecting), Status::Starting),
            (signing_in(Login::ChooseServer), Status::NeedsYou),
            (signing_in(Login::Disconnected), Status::NeedsYou),
            (signing_in(Login::Failed("x".into())), Status::Failed),
            (signing_in(Login::Broken("x".into())), Status::Failed),
            (signing_in(Login::Online), Status::Idle),
            (online("/s/a.ts", ScriptPhase::Running), Status::Running),
            (online("/s/a.ts", ScriptPhase::Stopping), Status::Running),
            (online("/s/a.ts", ScriptPhase::Queued), Status::Starting),
            (online("/s/a.ts", ScriptPhase::NeedsInput), Status::NeedsYou),
            (online("/s/a.ts", failed), Status::Failed),
            (online("/s/a.ts", ScriptPhase::Finished), Status::Idle),
            (online("/s/a.ts", ScriptPhase::Stopped), Status::Idle),
        ];
        for (presence, expected) in cases {
            assert_eq!(status(&presence, false), expected, "{presence:?}");
        }
    }

    #[test]
    fn done_only_replaces_a_quiet_status() {
        assert_eq!(status(&Presence::Closed(None), true), Status::Done);
        assert_eq!(
            status(&online("/s/a.ts", ScriptPhase::Finished), true),
            Status::Done
        );
        assert_eq!(
            status(&online("/s/a.ts", ScriptPhase::Running), true),
            Status::Running
        );
        assert_eq!(
            status(&signing_in(Login::Disconnected), true),
            Status::NeedsYou
        );
    }

    #[test]
    fn a_card_says_when_its_script_ended_and_which_script_is_queued() {
        let account = Account {
            name: "Main".into(),
            username: "main".into(),
            password: String::new(),
            server: Some("Twig".into()),
        };
        let now = Instant::now();
        let ended = Presence::Closed(Some(ScriptState {
            entry: "/s/daily.ts".into(),
            name: "daily.ts".into(),
            phase: ScriptPhase::Finished,
            since: Some(now - Duration::from_secs(20 * 60)),
        }));
        let job = |presence: &Presence| match card(&account, presence, false, now).job {
            Job::Line(line) => (
                line.text.to_string(),
                line.time.map(|time| time.to_string()),
            ),
            Job::Loading(_) => (String::new(), None),
        };
        assert_eq!(job(&ended).0, "daily.ts finished 20m ago");
        let queued = Presence::Waiting {
            script: Some(queued("/s/daily.ts")),
            open: None,
        };
        assert_eq!(job(&queued), ("daily.ts".to_owned(), None));
        let idle = signing_in(Login::Online);
        assert_eq!(card(&account, &idle, true, now).status, Status::Done);
        assert_eq!(card(&account, &idle, true, now).about(), About::Script);
        let loading = signing_in(Login::Loading(None));
        assert_eq!(card(&account, &loading, false, now).about(), About::Login);
        assert_eq!(
            card(&account, &Presence::Closed(None), false, now)
                .place
                .text,
            "Twig"
        );
        let failed = online(
            "/s/daily.ts",
            ScriptPhase::Failed("couldn't join house-12345678, the room is full".into()),
        );
        let Job::Line(line) = card(&account, &failed, false, now).job else {
            panic!("a failed script names its error");
        };
        assert_eq!(line.text, "Couldn't join house-12345678, the room is full");
        assert_eq!(
            line.detail,
            Some((
                "daily.ts failed".into(),
                "Couldn't join house-12345678, the room is full".into()
            ))
        );
    }

    #[test]
    fn elapsed_times_read_in_minutes_then_hours() {
        assert_eq!(span(Duration::from_secs(30)), "<1m");
        assert_eq!(span(Duration::from_secs(14 * 60)), "14m");
        assert_eq!(span(Duration::from_secs(7 * 3600)), "7h 0m");
        assert_eq!(ago(Duration::from_secs(10)), "just now");
        assert_eq!(ago(Duration::from_secs(20 * 60)), "20m ago");
        assert_eq!(ago(Duration::from_secs(3 * 3600 + 120)), "3h ago");
    }
}
