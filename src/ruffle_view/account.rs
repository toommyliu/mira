use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use gpui::{App, Context, EventEmitter, SharedString};
use mira_ui::Tone;

use super::{Loading, RuffleView, script_names};
use crate::{
    accounts::{Account, AccountId},
    automation::{auto_relogin::Phase, sign_in::Status},
    scripting::Values,
};

pub(crate) struct Changed;

impl EventEmitter<Changed> for RuffleView {}

pub(crate) struct Updated;

impl EventEmitter<Updated> for RuffleView {}

pub(crate) struct Identified(pub(crate) String);

impl EventEmitter<Identified> for RuffleView {}

pub(super) struct Link {
    name: String,
    username: String,
}

impl Link {
    fn new(account: &Account) -> Self {
        Self {
            name: account.name.clone(),
            username: account.username.clone(),
        }
    }

    fn id(&self) -> AccountId {
        AccountId::of(&self.username)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Login {
    Loading(Option<u8>),
    LoggingIn { attempt: u32, of: u32 },
    Retrying { attempt: u32, of: u32 },
    ChooseServer,
    Online,
    Reconnecting,
    Disconnected,
    Failed(SharedString),
    Broken(SharedString),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) server: Option<SharedString>,
    pub(crate) area: Option<SharedString>,
    pub(crate) cell: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Character {
    pub(crate) level: Option<u32>,
    pub(crate) class: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScriptState {
    pub(crate) entry: PathBuf,
    pub(crate) name: SharedString,
    pub(crate) phase: ScriptPhase,
    pub(crate) since: Option<Instant>,
}

impl ScriptState {
    pub(crate) fn queued(entry: &Path, cx: &App) -> Self {
        Self {
            entry: entry.to_path_buf(),
            name: script_names::name(entry, cx),
            phase: ScriptPhase::Queued,
            since: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScriptPhase {
    Queued,
    Running,
    NeedsInput,
    Stopping,
    Finished,
    Stopped,
    Failed(SharedString),
}

#[derive(Clone)]
pub(crate) struct Glance {
    pub(crate) label: SharedString,
    pub(crate) tone: Tone,
}

impl Glance {
    fn new(label: impl Into<SharedString>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            tone,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Details {
    pub(crate) server: Option<SharedString>,
    pub(crate) script: Option<SharedString>,
    pub(crate) error: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Summary {
    pub(crate) login: Login,
    pub(crate) place: Option<Place>,
    pub(crate) character: Option<Character>,
    pub(crate) script: Option<ScriptState>,
}

impl RuffleView {
    pub(super) fn sign_in(
        &mut self,
        account: &Account,
        script: Option<(PathBuf, Values)>,
        cx: &mut Context<Self>,
    ) {
        self.link(account);
        if let Some((script, values)) = script {
            self.queue.add(script, values);
            self.queue.play();
        }
        self.use_account_settings(account.id(), cx);
        self.status = None;
        self.session
            .sign_in(account.credentials(), account.server.clone());
        self.changed(cx);
    }

    pub(super) fn adopt(&mut self, account: &Account, cx: &mut Context<Self>) {
        self.link(account);
        self.changed(cx);
    }

    fn link(&mut self, account: &Account) {
        if self.linked_account() != Some(account.id()) {
            self.span
                .in_scope(|| tracing::info!(account = %account.username, "linked an account"));
        }
        self.account = Some(Link::new(account));
    }

    pub(super) fn identify(&mut self, player: &str, cx: &mut Context<Self>) -> bool {
        let id = AccountId::of(player);
        if self.account.as_ref().is_some_and(|link| link.id() == id) {
            return false;
        }
        if let Some(link) = self.account.take() {
            self.span
                .in_scope(|| tracing::info!(account = %link.username, "unlinked the account"));
            self.queue.pause();
        }
        cx.emit(Identified(player.to_owned()));
        true
    }

    pub(super) fn signed_in(&mut self, status: Status, cx: &mut Context<Self>) {
        let lost = matches!(status, Status::Offline);
        self.status = Some(status);
        if lost {
            self.connection_lost(cx);
        }
        self.resume_script(cx);
        self.run_queued(cx);
        self.changed(cx);
    }

    pub(super) fn linked_account(&self) -> Option<AccountId> {
        self.account.as_ref().map(Link::id)
    }

    pub(super) fn online(&self) -> bool {
        matches!(self.status, Some(Status::Online(_)))
    }

    pub(super) fn queue_script(
        &mut self,
        script: PathBuf,
        values: Values,
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        if replace {
            self.queue.add_next(script, values);
            self.skip_script(cx);
        } else if !self.queue.waits_for(&script) {
            self.queue.add(script, values);
        }
        self.queue.play();
        self.run_queued(cx);
        self.changed(cx);
    }

    pub(super) fn run_queued(&mut self, cx: &mut Context<Self>) {
        self.run_next(false, cx);
    }

    pub(super) fn run_next(&mut self, continuing: bool, cx: &mut Context<Self>) {
        if !self.online() || self.script.running() {
            return;
        }
        if let Some(launch) = self.queue.start_next(Instant::now()) {
            self.begin_script(launch, !continuing, cx);
        }
    }

    fn waiting(&self) -> Option<&Path> {
        self.queue
            .next()
            .filter(|_| self.queue.playing() && !self.script.running())
            .map(|entry| entry.script.as_path())
    }

    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(Changed);
        cx.notify();
    }

    pub(crate) fn summary(&self, cx: &App) -> Summary {
        let login = match (&self.error, self.status.as_ref()) {
            (Some(error), _) => Login::Broken(error.clone().into()),
            (None, None) => Login::Loading(match self.loading {
                Loading::Percent(percent) => Some(percent),
                Loading::Started | Loading::Done => None,
            }),
            (None, Some(Status::LoggingIn { attempt, of })) => Login::LoggingIn {
                attempt: *attempt,
                of: *of,
            },
            (None, Some(Status::Retrying { attempt, of })) => Login::Retrying {
                attempt: *attempt,
                of: *of,
            },
            (None, Some(Status::ChooseServer)) => Login::ChooseServer,
            (None, Some(Status::Online(_))) => Login::Online,
            (None, Some(Status::Offline)) if relogging(&self.auto_relogin) => Login::Reconnecting,
            (None, Some(Status::Offline)) => Login::Disconnected,
            (None, Some(Status::Failed(error))) => Login::Failed(error.clone().into()),
        };
        let online = login == Login::Online;
        let place = online.then(|| {
            let location = self.bar.location();
            Place {
                server: match self.status.as_ref() {
                    Some(Status::Online(server)) => server.clone().map(Into::into),
                    _ => None,
                },
                area: location.map(|location| match location.room {
                    Some(room) => format!("{}-{room}", location.map).into(),
                    None => location.map.clone().into(),
                }),
                cell: location.map(|location| location.cell.clone().into()),
            }
        });
        let character = self
            .bar
            .identity()
            .filter(|_| online)
            .map(|identity| Character {
                level: identity.level,
                class: identity.class.clone().map(Into::into),
            });
        let queued = self.waiting().map(|script| ScriptState::queued(script, cx));
        Summary {
            login,
            place,
            character,
            script: queued.or_else(|| self.script.state(cx)),
        }
    }

    pub(super) fn glance(&self, cx: &App) -> Option<Glance> {
        let link = self.account.as_ref();
        let queued = self.waiting();
        if self.error.is_some() {
            return Some(Glance::new("Couldn't start", Tone::Danger));
        }
        let online = match self.status.as_ref() {
            Some(status @ Status::Online(_)) => Some(login_label(status)),
            Some(Status::Failed(_)) => return Some(Glance::new("Login failed", Tone::Danger)),
            Some(Status::LoggingIn { attempt: 1, .. }) => {
                return Some(Glance::new("Logging in", Tone::Accent));
            }
            Some(Status::LoggingIn { attempt, of }) => {
                return Some(Glance::new(
                    format!("Logging in {attempt}/{of}"),
                    Tone::Accent,
                ));
            }
            Some(Status::Retrying { attempt, of }) => {
                return Some(Glance::new(
                    format!("Retrying {attempt}/{of}"),
                    Tone::Warning,
                ));
            }
            Some(Status::ChooseServer) => return Some(Glance::new("Choose server", Tone::Warning)),
            Some(Status::Offline) if link.is_none() => {
                return Some(Glance::new("Not logged in", Tone::Neutral));
            }
            Some(Status::Offline) => return Some(Glance::new("Offline", Tone::Warning)),
            None => match (link, self.loading) {
                (None, Loading::Done) => None,
                (_, Loading::Percent(percent)) => {
                    return Some(Glance::new(format!("Loading {percent}%"), Tone::Neutral));
                }
                (_, _) => return Some(Glance::new("Loading", Tone::Neutral)),
            },
        };
        let script = self.script_status(cx);
        if let Some(entry) = self.script.entry()
            && self.script.failed()
            && queued.is_none()
        {
            let name = script_names::name(entry, cx);
            return Some(Glance::new(format!("{name} failed"), Tone::Danger));
        }
        if self.script.running() {
            return script.map(|status| Glance::new(status, Tone::Accent));
        }
        if let Some(queued) = queued {
            let waiting = format!("Waiting to run {}", script_names::name(queued, cx));
            return Some(Glance::new(waiting, Tone::Neutral));
        }
        match script {
            Some(script) => Some(Glance::new(script, Tone::Neutral)),
            None => online.map(|online| Glance::new(online, Tone::Neutral)),
        }
    }

    pub(super) fn details(&self, cx: &App) -> Details {
        let status = self.status.as_ref();
        let queued = self.waiting();
        let server = match status {
            Some(Status::Online(server)) => server.clone().map(Into::into),
            _ => None,
        };
        let script = queued
            .or_else(|| self.script.entry().map(PathBuf::as_path))
            .map(|script| script_names::name(script, cx));
        let error = match (&self.error, status) {
            (Some(error), _) | (None, Some(Status::Failed(error))) => Some(error.clone().into()),
            _ => self
                .script
                .failure()
                .filter(|_| queued.is_none())
                .map(Into::into),
        };
        Details {
            server,
            script,
            error,
        }
    }

    pub(super) fn character(&self) -> Option<&str> {
        self.bar.character()
    }

    pub(super) fn username(&self) -> Option<&str> {
        self.account.as_ref().map(|link| link.username.as_str())
    }

    pub(super) fn account_name(&self) -> Option<&str> {
        self.account.as_ref().map(|link| link.name.as_str())
    }

    pub(super) fn account_id(&self) -> Option<AccountId> {
        self.account.as_ref().map(Link::id)
    }

    pub(super) fn signed_out(&self) -> bool {
        self.account.is_some() && signed_out(self.status.as_ref(), &self.auto_relogin)
    }

    pub(super) fn sign_in_again(&mut self, account: &Account, cx: &mut Context<Self>) {
        if self.signed_out() {
            self.sign_in(account, None, cx);
        }
    }
}

fn signed_out(status: Option<&Status>, relogin: &Phase) -> bool {
    matches!(status, Some(Status::Offline | Status::Failed(_))) && !relogging(relogin)
}

fn relogging(relogin: &Phase) -> bool {
    matches!(
        relogin,
        Phase::Waiting | Phase::Attempt { .. } | Phase::Backoff { .. }
    )
}

fn login_label(status: &Status) -> SharedString {
    match status {
        Status::LoggingIn { attempt: 1, .. } => "Logging in".into(),
        Status::LoggingIn { attempt, of } => {
            format!("Logging in, attempt {attempt} of {of}").into()
        }
        Status::Retrying { attempt, of } => {
            format!("Attempt {attempt} of {of} failed, retrying").into()
        }
        Status::ChooseServer => "Choose a server".into(),
        Status::Online(Some(server)) => server.clone().into(),
        Status::Online(None) => "Online".into(),
        Status::Offline => "Offline".into(),
        Status::Failed(error) => format!("Login failed: {error}").into(),
    }
}

pub(crate) fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.display().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_signs_a_logged_out_tab_back_in() {
        let failed = Status::Failed("Wrong password".into());
        let attempt = Phase::Attempt { attempt: 1, of: 3 };
        assert!(signed_out(Some(&Status::Offline), &Phase::Off));
        assert!(signed_out(Some(&Status::Offline), &Phase::Armed));
        assert!(signed_out(Some(&failed), &Phase::Off));
        assert!(!signed_out(Some(&Status::Offline), &Phase::Waiting));
        assert!(!signed_out(Some(&Status::Offline), &attempt));
        assert!(!signed_out(Some(&Status::Online(None)), &Phase::Armed));
        assert!(!signed_out(
            Some(&Status::LoggingIn { attempt: 1, of: 3 }),
            &Phase::Off
        ));
        assert!(!signed_out(None, &Phase::Off));
    }
}
