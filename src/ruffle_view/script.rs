use std::path::PathBuf;

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription};

use super::{RuffleView, ScriptPhase, ScriptState, script_dialog::ScriptDialogs};
use crate::{
    ruffle::Msg,
    scripting::{Launch, Level, Limit, Limits, Outcome, Run},
};

pub(super) struct CloseTab;

impl EventEmitter<CloseTab> for RuffleView {}

#[derive(Default)]
pub(super) struct Script {
    loaded: Option<Loaded>,
    dialogs: Option<Dialogs>,
}

struct Loaded {
    launch: Launch,
    name: SharedString,
    phase: Phase,
}

struct Dialogs {
    view: Entity<ScriptDialogs>,
    waiting: bool,
    _changes: Subscription,
}

enum Phase {
    Running { run: Run, stopping: bool },
    Finished(Outcome),
}

impl Script {
    pub(super) fn running(&self) -> bool {
        matches!(
            self.loaded,
            Some(Loaded {
                phase: Phase::Running { .. },
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

    pub(super) fn entry(&self) -> Option<&PathBuf> {
        self.loaded.as_ref().map(|loaded| &loaded.launch.entry)
    }

    pub(super) fn loaded(&self) -> Option<&SharedString> {
        self.loaded.as_ref().map(|loaded| &loaded.name)
    }

    pub(super) fn failure(&self) -> Option<String> {
        match &self.loaded.as_ref()?.phase {
            Phase::Finished(Outcome::Failed { message, .. }) => Some(message.clone()),
            Phase::Finished(Outcome::Killed(Limit::Turn)) => {
                Some("Stopped for running too long without waiting".into())
            }
            Phase::Finished(Outcome::Killed(Limit::Memory)) => {
                Some("Stopped for using too much memory".into())
            }
            _ => None,
        }
    }

    pub(super) fn dialogs(&self) -> Option<Entity<ScriptDialogs>> {
        Some(self.dialogs.as_ref()?.view.clone())
    }

    pub(super) fn state(&self) -> Option<ScriptState> {
        let Loaded {
            launch,
            name,
            phase,
        } = self.loaded.as_ref()?;
        let waiting = self.dialogs.as_ref().is_some_and(|dialogs| dialogs.waiting);
        let phase = match phase {
            Phase::Running {
                stopping: false, ..
            } if waiting => ScriptPhase::NeedsInput,
            Phase::Running {
                stopping: false, ..
            } => ScriptPhase::Running,
            Phase::Running { stopping: true, .. } => ScriptPhase::Stopping,
            Phase::Finished(Outcome::Completed | Outcome::Exited { .. }) => ScriptPhase::Finished,
            Phase::Finished(Outcome::Stopped) => ScriptPhase::Stopped,
            Phase::Finished(_) => ScriptPhase::Failed(self.failure().unwrap_or_default().into()),
        };
        Some(ScriptState {
            entry: launch.entry.clone(),
            name: name.clone(),
            phase,
        })
    }

    pub(super) fn status(&self) -> Option<String> {
        let Loaded { name, phase, .. } = self.loaded.as_ref()?;
        let waiting = self.dialogs.as_ref().is_some_and(|dialogs| dialogs.waiting);
        match phase {
            Phase::Running {
                stopping: false, ..
            } if waiting => Some(format!("{name} is waiting for you")),
            Phase::Running {
                stopping: false, ..
            } => Some(format!("Running {name}")),
            Phase::Running { stopping: true, .. } => Some(format!("Stopping {name}")),
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

impl RuffleView {
    pub(super) fn start_loaded_script(&mut self, cx: &mut Context<Self>) {
        if let Some(loaded) = &self.script.loaded {
            self.start_script(loaded.launch.clone(), cx);
        }
    }

    pub(super) fn start_script(&mut self, launch: Launch, cx: &mut Context<Self>) {
        if self.script.running() {
            return;
        }
        let name: SharedString = super::account::file_name(&launch.entry).into();
        let inbox = self.session.inbox();
        let path = launch.entry.clone();
        let (finished, outcome) = async_channel::bounded(1);
        let (asks, asked) = async_channel::unbounded();
        let started = Run::start(
            launch.clone(),
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
                    let stack = stack
                        .as_deref()
                        .map(|stack| format!("\n{stack}"))
                        .unwrap_or_default();
                    tracing::warn!(target: "mira::script", "{} failed: {message}{stack}", path.display());
                }
                let _ = finished.try_send(outcome);
            },
        );
        let phase = match started {
            Ok(run) => {
                let dialogs = self.script_dialogs(cx);
                dialogs.update(cx, |dialogs, cx| dialogs.start(name.clone(), asked, cx));
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
                tracing::warn!(target: "mira::script", "{} failed: {error}", launch.entry.display());
                Phase::Finished(Outcome::Failed {
                    message: error.to_string(),
                    stack: None,
                })
            }
        };
        self.script.loaded = Some(Loaded {
            launch,
            name,
            phase,
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
        let close_tab = outcome == Outcome::Exited { close_tab: true };
        if let Some(loaded) = &mut self.script.loaded {
            loaded.phase = Phase::Finished(outcome);
        }
        if let Some(dialogs) = &self.script.dialogs {
            dialogs.view.update(cx, |dialogs, cx| dialogs.end(cx));
        }
        if close_tab {
            cx.emit(CloseTab);
        } else {
            self.run_queued(cx);
        }
        self.changed(cx);
    }

    pub(super) fn stop_script(&mut self, cx: &mut Context<Self>) {
        if let Some(Loaded {
            phase: Phase::Running { run, stopping },
            ..
        }) = &mut self.script.loaded
        {
            run.stop();
            *stopping = true;
            self.changed(cx);
        }
    }
}
