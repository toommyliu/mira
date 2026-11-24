use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Subscription, Task};

use super::{
    RuffleView, ScriptPhase, ScriptState,
    inputs_dialog::{InputsDialog, InputsEvent, Preflight, Purpose, preflight},
    inputs_form::{self, Read},
    queue::Queue,
    script_dialog::ScriptDialogs,
    script_names,
};
use crate::{
    account_settings,
    accounts::AccountId,
    packages,
    ruffle::{Command, Msg},
    script_inputs,
    scripting::{Declared, Launch, Level, Limit, Limits, Outcome, Run},
};

pub(super) struct CloseTab;

impl EventEmitter<CloseTab> for RuffleView {}

pub(super) struct Finished(pub(super) PathBuf);

impl EventEmitter<Finished> for RuffleView {}

#[derive(Default)]
pub(super) struct Script {
    loaded: Option<Loaded>,
    dialogs: Option<Dialogs>,
    leaving: Option<Task<()>>,
    restart: Option<Restart>,
    next: Option<Next>,
    inputs: Option<Inputs>,
    checking: Option<Task<()>>,
    declares: Option<(PathBuf, bool)>,
    reads: u64,
}

struct Inputs {
    view: Entity<InputsDialog>,
    _events: Subscription,
}

struct Next {
    launch: Launch,
    start: bool,
}

struct Restart {
    account: AccountId,
    queued: bool,
}

struct Loaded {
    launch: Launch,
    character: Option<AccountId>,
    phase: Phase,
    since: Instant,
}

struct Dialogs {
    view: Entity<ScriptDialogs>,
    waiting: bool,
    _changes: Subscription,
}

enum Phase {
    Ready,
    Preparing { _moving: Task<()> },
    Running { run: Run, stopping: bool },
    Interrupted,
    Finished(Outcome),
}

impl Script {
    pub(super) fn running(&self) -> bool {
        matches!(
            self.loaded,
            Some(Loaded {
                phase: Phase::Preparing { .. } | Phase::Running { .. },
                ..
            })
        )
    }

    pub(super) fn stoppable(&self) -> bool {
        self.running()
            || matches!(
                self.loaded,
                Some(Loaded {
                    phase: Phase::Interrupted,
                    ..
                })
            )
    }

    pub(super) fn failed(&self) -> bool {
        matches!(
            self.loaded,
            Some(Loaded {
                phase: Phase::Finished(Outcome::Failed { .. } | Outcome::Killed(_)),
                ..
            })
        )
    }

    fn bind(&mut self, character: &AccountId) -> bool {
        let running = self.running();
        match &mut self.loaded {
            Some(loaded) if running => {
                loaded.character.get_or_insert_with(|| character.clone()) == character
            }
            _ => true,
        }
    }

    fn load(&mut self, launch: Launch, start: bool) {
        self.next = Some(Next { launch, start });
    }

    fn hold_next(&mut self) {
        if let Some(next) = &mut self.next {
            next.start = false;
        }
    }

    fn end(&mut self, outcome: Outcome, interrupted: bool) {
        if let Some(loaded) = &mut self.loaded {
            loaded.since = Instant::now();
            loaded.phase = if interrupted {
                Phase::Interrupted
            } else {
                Phase::Finished(outcome)
            };
        }
    }

    fn take_next(&mut self) -> bool {
        if self.stoppable() {
            return false;
        }
        let Some(Next { launch, start }) = self.next.take() else {
            return false;
        };
        self.loaded = Some(Loaded {
            launch,
            character: None,
            phase: Phase::Ready,
            since: Instant::now(),
        });
        start
    }

    pub(super) fn entry(&self) -> Option<&PathBuf> {
        self.loaded.as_ref().map(|loaded| &loaded.launch.entry)
    }

    fn upcoming(&self) -> Option<&PathBuf> {
        self.next
            .as_ref()
            .map(|next| &next.launch.entry)
            .or_else(|| self.entry())
    }

    pub(super) fn has_inputs(&self) -> bool {
        let Some(entry) = self.upcoming() else {
            return false;
        };
        match &self.declares {
            Some((read, declares)) if read == entry => *declares,
            _ => true,
        }
    }

    fn next_read(&mut self) -> u64 {
        self.reads += 1;
        self.reads
    }

    fn read(&mut self, entry: &Path, read: &Read, issued: u64) -> bool {
        if issued != self.reads {
            return false;
        }
        let declares = declares_inputs(read);
        let before = self.has_inputs();
        self.declares = Some((entry.to_path_buf(), declares));
        self.has_inputs() != before
    }

    pub(super) fn inputs(&self) -> Option<Entity<InputsDialog>> {
        Some(self.inputs.as_ref()?.view.clone())
    }

    pub(super) fn asking_inputs(&self, cx: &App) -> bool {
        self.inputs
            .as_ref()
            .is_some_and(|inputs| inputs.view.read(cx).open())
    }

    fn start_launch(&self, cx: &App) -> Option<Launch> {
        let entry = self.entry()?.clone();
        let inputs = script_inputs::saved(&entry, cx);
        Some(Launch { entry, inputs })
    }

    pub(super) fn failure(&self) -> Option<String> {
        match &self.loaded.as_ref()?.phase {
            Phase::Finished(outcome) => failure(outcome),
            Phase::Ready | Phase::Preparing { .. } | Phase::Running { .. } | Phase::Interrupted => {
                None
            }
        }
    }

    pub(super) fn dialogs(&self) -> Option<Entity<ScriptDialogs>> {
        Some(self.dialogs.as_ref()?.view.clone())
    }

    pub(super) fn state(&self, cx: &App) -> Option<ScriptState> {
        let Loaded {
            launch,
            phase,
            since,
            ..
        } = self.loaded.as_ref()?;
        let waiting = self.dialogs.as_ref().is_some_and(|dialogs| dialogs.waiting);
        let phase = match phase {
            Phase::Ready => return None,
            Phase::Preparing { .. } => ScriptPhase::Running,
            Phase::Interrupted => ScriptPhase::Queued,
            Phase::Running {
                stopping: false, ..
            } if waiting => ScriptPhase::NeedsInput,
            Phase::Running {
                stopping: false, ..
            } => ScriptPhase::Running,
            Phase::Running { stopping: true, .. } => ScriptPhase::Stopping,
            Phase::Finished(_) if self.leaving.is_some() => ScriptPhase::Stopping,
            Phase::Finished(Outcome::Completed | Outcome::Exited { .. }) => ScriptPhase::Finished,
            Phase::Finished(Outcome::Stopped) => ScriptPhase::Stopped,
            Phase::Finished(_) => ScriptPhase::Failed(self.failure().unwrap_or_default().into()),
        };
        Some(ScriptState {
            entry: launch.entry.clone(),
            name: script_names::name(&launch.entry, cx),
            phase,
            since: Some(*since),
        })
    }

    pub(super) fn status(&self, cx: &App) -> Option<String> {
        let Loaded { launch, phase, .. } = self.loaded.as_ref()?;
        let name = script_names::name(&launch.entry, cx);
        let waiting = self.dialogs.as_ref().is_some_and(|dialogs| dialogs.waiting);
        match phase {
            Phase::Ready => None,
            Phase::Preparing { .. } => Some(format!("Starting {name}")),
            Phase::Interrupted => Some(format!("{name} restarts after reconnecting")),
            Phase::Running {
                stopping: false, ..
            } if waiting => Some(format!("{name} is waiting for you")),
            Phase::Running {
                stopping: false, ..
            } => Some(format!("Running {name}")),
            Phase::Running { stopping: true, .. } => Some(format!("Stopping {name}")),
            Phase::Finished(_) if self.leaving.is_some() => Some(format!("Stopping {name}")),
            Phase::Finished(outcome) => Some(match outcome {
                Outcome::Completed => format!("{name} finished"),
                Outcome::Failed { message, .. } => format!("{name} failed: {message}"),
                Outcome::Stopped => format!("{name} stopped"),
                Outcome::Exited { .. } => format!("{name} exited"),
                Outcome::Killed(Limit::Turn) => {
                    format!("{name} was stopped for running too long without waiting")
                }
                Outcome::Killed(Limit::Memory) => {
                    format!("{name} was stopped for using too much memory")
                }
            }),
        }
    }
}

fn declares_inputs(read: &Read) -> bool {
    !matches!(
        read,
        Ok(Declared {
            inputs: Ok(None),
            ..
        })
    )
}

pub(super) fn failure(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Failed { message, .. } => Some(message.clone()),
        Outcome::Killed(Limit::Turn) => Some("Stopped for running too long without waiting".into()),
        Outcome::Killed(Limit::Memory) => Some("Stopped for using too much memory".into()),
        Outcome::Completed | Outcome::Stopped | Outcome::Exited { .. } => None,
    }
}

impl RuffleView {
    pub(super) fn load_script(&mut self, entry: PathBuf, start: bool, cx: &mut Context<Self>) {
        let inputs = script_inputs::saved(&entry, cx);
        self.stop_script(cx);
        self.script.load(
            Launch {
                entry: entry.clone(),
                inputs,
            },
            start,
        );
        if self.script.take_next() {
            self.request_start(cx);
        } else {
            self.check_inputs(entry, Purpose::Fill, cx);
        }
        self.changed(cx);
    }

    pub(super) fn edit_inputs(&mut self, cx: &mut Context<Self>) {
        if self.script.asking_inputs(cx) {
            return;
        }
        if let Some(entry) = self.script.upcoming().cloned() {
            self.check_inputs(entry, Purpose::Edit, cx);
        }
    }

    pub(super) fn request_start(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.script.entry().filter(|_| !self.script.running()) {
            self.check_inputs(entry.clone(), Purpose::Start, cx);
        }
    }

    fn check_inputs(&mut self, entry: PathBuf, purpose: Purpose, cx: &mut Context<Self>) {
        let issued = self.script.next_read();
        let read = inputs_form::read(entry.clone(), cx);
        self.script.checking = Some(cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                this.script.checking = None;
                if this.script.upcoming() != Some(&entry) {
                    return;
                }
                if this.script.read(&entry, &read, issued) {
                    this.changed(cx);
                }
                let saved = script_inputs::saved(&entry, cx);
                let ask = (purpose == Purpose::Edit && declares_inputs(&read))
                    || preflight(&read, &saved) == Preflight::Ask;
                match purpose {
                    Purpose::Start if this.script.running() => {}
                    Purpose::Start if !ask => this.start_loaded_script(cx),
                    _ if ask => this.ask_inputs(entry, read, purpose, cx),
                    _ => {}
                }
            });
        }));
    }

    fn ask_inputs(&mut self, entry: PathBuf, read: Read, purpose: Purpose, cx: &mut Context<Self>) {
        if let Some(inputs) = &self.script.inputs {
            let view = inputs.view.clone();
            let shown = view.read_with(cx, |view, _| view.open() && *view.script() == entry);
            if shown {
                if purpose == Purpose::Start {
                    view.update(cx, |view, cx| view.ask_to_start(cx));
                }
                return;
            }
        }
        let running = self.script.running();
        let view = cx.new(|cx| InputsDialog::new(entry, read, purpose, running, cx));
        let events = cx.subscribe(&view, |this, view, event: &InputsEvent, cx| match event {
            InputsEvent::Start if this.script.entry() == Some(view.read(cx).script()) => {
                this.start_loaded_script(cx)
            }
            InputsEvent::Start => {}
            InputsEvent::Closed => this.changed(cx),
        });
        self.script.inputs = Some(Inputs {
            view,
            _events: events,
        });
        self.changed(cx);
    }

    fn probe_inputs(&mut self, entry: PathBuf, cx: &mut Context<Self>) {
        let issued = self.script.next_read();
        let read = inputs_form::read(entry.clone(), cx);
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.script.upcoming() == Some(&entry) && this.script.read(&entry, &read, issued)
                {
                    this.changed(cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn start_loaded_script(&mut self, cx: &mut Context<Self>) {
        if let Some(launch) = self.script.start_launch(cx) {
            self.start_script(launch, cx);
        }
    }

    pub(super) fn start_script(&mut self, launch: Launch, cx: &mut Context<Self>) {
        self.begin_script(launch, true, cx);
    }

    pub(super) fn begin_script(&mut self, launch: Launch, fresh: bool, cx: &mut Context<Self>) {
        if self.script.running() {
            return;
        }
        self.probe_inputs(launch.entry.clone(), cx);
        self.script.leaving = None;
        self.script.restart = None;
        let character = self.character().map(AccountId::of);
        if !fresh || !self.safe_start_stop(cx) {
            return self.run_script(launch, character, cx);
        }
        let ready = launch.clone();
        let moving = self.retreat(cx, move |this, cx| {
            let character = this
                .script
                .loaded
                .as_mut()
                .and_then(|loaded| loaded.character.take());
            this.run_script(ready, character, cx)
        });
        self.script.loaded = Some(Loaded {
            launch,
            character,
            phase: Phase::Preparing { _moving: moving },
            since: Instant::now(),
        });
        self.changed(cx);
    }

    fn safe_start_stop(&self, cx: &App) -> bool {
        self.settings_account
            .as_ref()
            .map(|account| account_settings::get(account, cx).scripts)
            .unwrap_or_default()
            .safe_start_stop
    }

    fn retreat(
        &self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) -> Task<()> {
        let (moved, arrived) = async_channel::bounded(1);
        self.session.command(Command::Retreat(moved));
        let span = self.span.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Err(error)) = arrived.recv().await {
                span.in_scope(|| tracing::warn!(%error, "couldn't move somewhere safe"));
            }
            let _ = this.update(cx, |this, cx| then(this, cx));
        })
    }

    fn run_script(&mut self, launch: Launch, character: Option<AccountId>, cx: &mut Context<Self>) {
        script_names::refresh(&launch.entry, cx);
        let inbox = self.session.inbox();
        let path = launch.entry.clone();
        let (finished, outcome) = async_channel::bounded(1);
        let (asks, asked) = async_channel::unbounded();
        let installed = packages::packages(cx).read(cx).root().to_owned();
        let started = self.span.in_scope(|| {
            Run::start(
                launch.clone(),
                installed,
                Limits::default(),
                Box::new(|level, message| match level {
                    Level::Debug => tracing::debug!(target: "mira::script", "{message}"),
                    Level::Info => tracing::info!(target: "mira::script", "{message}"),
                    Level::Warn => tracing::warn!(target: "mira::script", "{message}"),
                    Level::Error => tracing::error!(target: "mira::script", "{message}"),
                }),
                Box::new(move |message| inbox.post(Msg::Script(message))),
                Box::new(move |message| {
                    let _ = asks.try_send(message);
                }),
                move |outcome| {
                    if let Outcome::Failed { message, stack } = &outcome {
                        tracing::warn!(
                            target: "mira::script",
                            path = %path.display(),
                            error = %message,
                            stack = stack.as_deref(),
                            "script failed"
                        );
                    }
                    let _ = finished.try_send(outcome);
                },
            )
        });
        let phase = match started {
            Ok(run) => {
                let dialogs = self.script_dialogs(cx);
                dialogs.update(cx, |dialogs, cx| {
                    dialogs.start(launch.entry.clone(), asked, cx)
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(outcome) = outcome.recv().await {
                        let _ = this.update(cx, |this, cx| this.script_ended(outcome, cx));
                    }
                })
                .detach();
                Phase::Running {
                    run,
                    stopping: false,
                }
            }
            Err(error) => {
                self.span.in_scope(|| {
                    tracing::warn!(
                        target: "mira::script",
                        path = %launch.entry.display(),
                        %error,
                        "couldn't start the script"
                    )
                });
                let outcome = Outcome::Failed {
                    message: error.to_string(),
                    stack: None,
                };
                self.script.loaded = Some(Loaded {
                    launch,
                    character,
                    phase: Phase::Finished(outcome.clone()),
                    since: Instant::now(),
                });
                self.script_ended(outcome, cx);
                return;
            }
        };
        self.script.loaded = Some(Loaded {
            launch,
            character,
            phase,
            since: Instant::now(),
        });
        self.changed(cx);
    }

    fn script_dialogs(&mut self, cx: &mut Context<Self>) -> Entity<ScriptDialogs> {
        if let Some(dialogs) = &self.script.dialogs {
            return dialogs.view.clone();
        }
        let this = cx.weak_entity();
        let stop = move |cx: &mut App| {
            let _ = this.update(cx, |this, cx| this.stop_script(cx));
        };
        let view = cx.new(|cx| ScriptDialogs::new(stop, cx));
        let changes = cx.observe(&view, |this, view, cx| {
            let waiting = view.read(cx).waiting();
            if let Some(dialogs) = &mut this.script.dialogs
                && dialogs.waiting != waiting
            {
                dialogs.waiting = waiting;
                this.changed(cx);
            }
        });
        self.script.dialogs = Some(Dialogs {
            view: view.clone(),
            waiting: false,
            _changes: changes,
        });
        view
    }

    fn script_ended(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let close_tab = matches!(
            outcome,
            Outcome::Exited {
                close_tab: true,
                ..
            }
        );
        let ran = matches!(
            self.script.loaded,
            Some(Loaded {
                phase: Phase::Running { .. },
                ..
            })
        );
        let logout = matches!(outcome, Outcome::Exited { logout: true, .. });
        let restart = self.script.restart.take().filter(|_| !logout);
        let interrupted = restart.is_some();
        if let Some(mut restart) = restart {
            restart.queued = self.queue.running().is_some();
            self.script.restart = Some(restart);
        } else {
            self.queue.ended(&outcome, Instant::now());
        }
        let finished =
            !interrupted && matches!(outcome, Outcome::Completed | Outcome::Exited { .. });
        if finished && let Some(entry) = self.script.entry() {
            cx.emit(Finished(entry.clone()));
        }
        self.script.end(outcome, interrupted);
        let start = self.script.take_next();
        if let Some(dialogs) = &self.script.dialogs {
            dialogs.view.update(cx, |dialogs, cx| dialogs.end(cx));
        }
        if close_tab {
            cx.emit(CloseTab);
        } else {
            self.resume_script(cx);
            self.run_next(ran, cx);
            if start {
                self.request_start(cx);
            } else if ran
                && !logout
                && self.online()
                && !self.script.running()
                && self.safe_start_stop(cx)
            {
                self.script.leaving = Some(self.retreat(cx, |this, cx| {
                    this.script.leaving = None;
                    this.changed(cx);
                }));
            }
        }
        self.changed(cx);
    }

    pub(super) fn script_status(&self, cx: &App) -> Option<String> {
        let status = self.script.status(cx)?;
        Some(match self.queue.ahead() {
            queued @ 1.. if self.script.running() => format!("{status} · {queued} queued"),
            _ => status,
        })
    }

    pub(super) fn edit_queue(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Queue)) {
        edit(&mut self.queue);
        self.run_queued(cx);
        self.changed(cx);
    }

    pub(super) fn stop_script(&mut self, cx: &mut Context<Self>) {
        self.script.checking = None;
        self.script.hold_next();
        self.queue.pause();
        self.skip_script(cx);
        self.changed(cx);
    }

    pub(super) fn skip_script(&mut self, cx: &mut Context<Self>) {
        let restart = self.script.restart.take();
        if let Some(Loaded {
            phase: Phase::Interrupted,
            ..
        }) = &self.script.loaded
        {
            self.script.end(Outcome::Stopped, false);
            if restart.is_some_and(|restart| restart.queued) {
                self.queue.ended(&Outcome::Stopped, Instant::now());
            }
            self.run_queued(cx);
            self.changed(cx);
            return;
        }
        self.end_run(cx);
    }

    fn end_run(&mut self, cx: &mut Context<Self>) {
        match &mut self.script.loaded {
            Some(Loaded {
                phase: Phase::Running { run, stopping },
                ..
            }) => {
                run.stop();
                *stopping = true;
                self.changed(cx);
            }
            Some(Loaded {
                phase: Phase::Preparing { .. },
                ..
            }) => self.script_ended(Outcome::Stopped, cx),
            _ => {}
        }
    }

    pub(super) fn connection_lost(&mut self, cx: &mut Context<Self>) {
        let running = matches!(
            self.script.loaded,
            Some(Loaded {
                phase: Phase::Preparing { .. }
                    | Phase::Running {
                        stopping: false,
                        ..
                    },
                ..
            })
        );
        let restarts = self.settings_account.as_ref().is_some_and(|account| {
            account_settings::get(account, cx)
                .scripts
                .restart_after_reconnect
        });
        let Some(account) = self.linked_account().filter(|_| running && restarts) else {
            return;
        };
        self.script.restart = Some(Restart {
            account,
            queued: false,
        });
        self.end_run(cx);
    }

    pub(super) fn bind_script(&mut self, player: &str, cx: &mut Context<Self>) {
        if self.script.bind(&AccountId::of(player)) {
            return;
        }
        if let Some(entry) = self.script.entry() {
            self.span.in_scope(|| {
                tracing::warn!(
                    target: "mira::script",
                    path = %entry.display(),
                    character = player,
                    "stopped the script because a different character signed in"
                )
            });
        }
        if let Some(Loaded {
            phase: Phase::Running { run, .. },
            ..
        }) = &self.script.loaded
        {
            run.refuse_game_calls();
        }
        self.stop_script(cx);
    }

    pub(super) fn resume_script(&mut self, cx: &mut Context<Self>) {
        if !self.online() || self.script.running() {
            return;
        }
        let Some(restart) = self.script.restart.take() else {
            return;
        };
        let Some(loaded) = &mut self.script.loaded else {
            return;
        };
        let launch = loaded.launch.clone();
        loaded.phase = Phase::Finished(Outcome::Stopped);
        loaded.since = Instant::now();
        if self.linked_account() != Some(restart.account) {
            if restart.queued {
                self.queue.ended(&Outcome::Stopped, Instant::now());
                self.queue.pause();
            }
        } else if restart.queued {
            self.queue.requeue();
            self.run_queued(cx);
        } else {
            self.start_script(launch, cx);
        }
        self.changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::Supplied;

    fn preparing(character: Option<&str>) -> Script {
        Script {
            loaded: Some(Loaded {
                launch: Launch {
                    entry: PathBuf::from("hunt.ts"),
                    inputs: Supplied::Values(Default::default()),
                },
                character: character.map(AccountId::of),
                phase: Phase::Preparing {
                    _moving: Task::ready(()),
                },
                since: Instant::now(),
            }),
            ..Script::default()
        }
    }

    #[test]
    fn a_run_keeps_going_only_for_the_character_it_started_on() {
        let (wildfox, kestrel) = (AccountId::of("wildfox"), AccountId::of("kestrel"));

        let mut started = preparing(Some("Wildfox"));
        assert!(started.bind(&wildfox));
        assert!(!started.bind(&kestrel));

        let mut unidentified = preparing(None);
        assert!(unidentified.bind(&kestrel));
        assert!(!unidentified.bind(&wildfox));

        let mut ended = preparing(Some("wildfox"));
        if let Some(loaded) = &mut ended.loaded {
            loaded.phase = Phase::Finished(Outcome::Stopped);
        }
        assert!(ended.bind(&kestrel));
    }

    #[test]
    fn a_loaded_file_replaces_the_run_once_it_ends_and_starts_only_when_asked() {
        let farm = || Launch {
            entry: PathBuf::from("farm.ts"),
            inputs: Supplied::Values(Default::default()),
        };

        let mut idle = Script::default();
        idle.load(farm(), false);
        assert!(!idle.take_next(), "loading alone doesn't start the file");
        assert_eq!(idle.entry(), Some(&PathBuf::from("farm.ts")));
        assert!(!idle.stoppable());
        assert_eq!(idle.failure(), None);

        let mut running = preparing(None);
        running.load(farm(), true);
        assert!(!running.take_next());
        assert_eq!(running.entry(), Some(&PathBuf::from("hunt.ts")));
        assert!(running.stoppable());
        running.end(Outcome::Stopped, false);
        assert!(
            running.take_next(),
            "the file starts once the run has ended"
        );
        assert_eq!(running.entry(), Some(&PathBuf::from("farm.ts")));
        assert!(!running.take_next());

        let mut stopped = preparing(None);
        stopped.load(farm(), true);
        stopped.hold_next();
        stopped.end(Outcome::Stopped, false);
        assert!(
            !stopped.take_next(),
            "Stop during the handoff keeps the file idle"
        );
        assert_eq!(stopped.entry(), Some(&PathBuf::from("farm.ts")));
    }

    #[test]
    fn a_file_without_inputs_has_none_to_edit_until_another_loads() {
        let launch = |entry: &str| Launch {
            entry: PathBuf::from(entry),
            inputs: Supplied::Values(Default::default()),
        };
        let declares = |inputs| {
            Ok(Declared {
                meta: Default::default(),
                inputs,
            })
        };

        let mut script = Script::default();
        assert!(!script.has_inputs(), "nothing is loaded");
        script.load(launch("farm.ts"), false);
        script.take_next();
        assert!(script.has_inputs(), "the file hasn't been read yet");
        let issued = script.next_read();
        script.read(Path::new("farm.ts"), &declares(Ok(None)), issued);
        assert!(!script.has_inputs());

        let stale = script.next_read();
        let issued = script.next_read();
        script.read(
            Path::new("farm.ts"),
            &Ok(crate::scripting::extract(
                "farm.ts",
                Path::new("farm.ts"),
                r#"export const inputs = { kills: { type: "number", label: "Kills" } };"#,
            )
            .unwrap()),
            issued,
        );
        assert!(script.has_inputs(), "inputs added to the file show up");
        script.read(Path::new("farm.ts"), &declares(Ok(None)), stale);
        assert!(script.has_inputs(), "an older read doesn't win");

        script.load(launch("hunt.ts"), false);
        assert!(script.has_inputs());
        let issued = script.next_read();
        script.read(Path::new("hunt.ts"), &declares(Err("bad".into())), issued);
        assert!(script.has_inputs(), "invalid inputs still show their error");
    }

    #[gpui::test]
    fn start_runs_the_inputs_saved_since_the_file_was_loaded(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder = std::env::temp_dir().join(format!(
            "mira-start-inputs-{:x}",
            u64::from_ne_bytes(random)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let hunt = folder.join("hunt.ts");
        std::fs::write(&hunt, "").unwrap();
        let values = |kills: u64| {
            serde_json::json!({ "kills": kills })
                .as_object()
                .unwrap()
                .clone()
        };
        cx.update(|cx| {
            script_inputs::init(folder.join("inputs.json"), cx);
            script_inputs::save(&hunt, values(10), cx);
            let mut script = Script::default();
            script.load(
                Launch {
                    entry: hunt.clone(),
                    inputs: script_inputs::saved(&hunt, cx),
                },
                false,
            );
            script.take_next();

            script_inputs::save(&hunt, values(50), cx);
            assert_eq!(
                script.start_launch(cx).map(|launch| launch.inputs),
                Some(Supplied::Values(values(50)))
            );

            script.end(Outcome::Completed, false);
            script_inputs::save(&hunt, values(3), cx);
            assert_eq!(
                script.start_launch(cx).map(|launch| launch.inputs),
                Some(Supplied::Values(values(3)))
            );
        });
        std::fs::remove_dir_all(folder).unwrap();
    }
}
