use std::path::PathBuf;

use gpui::{Context, EventEmitter, SharedString};

use super::{Loading, RuffleView};
use crate::{accounts::Account, automation::sign_in::Status};

pub(crate) struct Changed;

impl EventEmitter<Changed> for RuffleView {}

pub(super) struct Link {
    status: Option<Status>,
    queued: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Busy,
    Online,
    Offline,
    Failed,
}

pub(crate) struct Summary {
    pub(crate) state: State,
    pub(crate) login: SharedString,
    pub(crate) script: Option<SharedString>,
    pub(crate) script_failed: bool,
    pub(crate) running: bool,
}

impl RuffleView {
    pub(super) fn sign_in(
        &mut self,
        account: &Account,
        script: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.account = Some(Link {
            status: None,
            queued: script,
        });
        self.session
            .sign_in(account.credentials(), account.server.clone());
        self.changed(cx);
    }

    pub(super) fn signed_in(&mut self, status: Status, cx: &mut Context<Self>) {
        if let Some(link) = &mut self.account {
            link.status = Some(status);
        }
        self.run_queued(cx);
        self.changed(cx);
    }

    pub(super) fn queue_script(&mut self, script: PathBuf, replace: bool, cx: &mut Context<Self>) {
        let Some(link) = &mut self.account else {
            return;
        };
        link.queued = Some(script);
        if replace {
            self.stop_script(cx);
        }
        self.run_queued(cx);
        self.changed(cx);
    }

    pub(super) fn cancel_script(&mut self, cx: &mut Context<Self>) {
        if let Some(link) = &mut self.account {
            link.queued = None;
        }
        self.stop_script(cx);
        self.changed(cx);
    }

    pub(super) fn run_queued(&mut self, cx: &mut Context<Self>) {
        let Some(link) = &mut self.account else {
            return;
        };
        if !matches!(link.status, Some(Status::Online(_))) || self.script.running() {
            return;
        }
        if let Some(script) = link.queued.take() {
            self.start_script(script, cx);
        }
    }

    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(Changed);
        cx.notify();
    }

    pub(crate) fn summary(&self) -> Summary {
        let status = self.account.as_ref().and_then(|link| link.status.as_ref());
        let login: SharedString = match (&self.error, status, self.loading) {
            (Some(error), _, _) => format!("Couldn't start: {error}").into(),
            (None, None, Loading::Percent(percent)) => format!("Loading {percent}%").into(),
            (None, None, _) => "Loading".into(),
            (None, Some(status), _) => login_label(status),
        };
        let queued = self
            .account
            .as_ref()
            .and_then(|link| link.queued.as_deref())
            .filter(|_| !self.script.running())
            .map(|script| format!("Waiting to run {}", file_name(script)).into());
        let state = match (&self.error, status) {
            (Some(_), _) | (None, Some(Status::Failed(_))) => State::Failed,
            (None, Some(Status::Online(_))) => State::Online,
            (None, Some(Status::Offline)) => State::Offline,
            (None, _) => State::Busy,
        };
        Summary {
            state,
            login,
            script_failed: queued.is_none() && self.script.failed(),
            script: queued.or_else(|| self.script.status().map(Into::into)),
            running: self.script.running(),
        }
    }

    pub(super) fn retry_sign_in(&mut self, account: &Account, cx: &mut Context<Self>) {
        let failed = self
            .account
            .as_ref()
            .is_some_and(|link| matches!(link.status, Some(Status::Failed(_))));
        if failed {
            let queued = self.account.take().and_then(|link| link.queued);
            self.sign_in(account, queued, cx);
        }
    }
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
        Status::Online(Some(server)) => format!("Online on {server}").into(),
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
