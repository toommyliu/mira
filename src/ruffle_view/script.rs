use std::path::PathBuf;

use gpui::{Context, EventEmitter, SharedString};

use super::RuffleView;
use crate::{
    ruffle::Msg,
    scripting::{Launch, Level, Limit, Limits, Outcome, Run},
};

pub(super) struct CloseTab;

impl EventEmitter<CloseTab> for RuffleView {}

#[derive(Default)]
pub(super) struct Script(Option<Loaded>);

struct Loaded {
    launch: Launch,
    name: SharedString,
    phase: Phase,
}

enum Phase {
    Running { run: Run, stopping: bool },
    Finished(Outcome),
}

impl Script {
    pub(super) fn running(&self) -> bool {
        matches!(
            self.0,
            Some(Loaded {
                phase: Phase::Running { .. },
                ..
            })
        )
    }

    pub(super) fn failed(&self) -> bool {
        matches!(
            self.0,
            Some(Loaded {
                phase: Phase::Finished(Outcome::Failed { .. } | Outcome::Killed(_)),
                ..
            })
        )
    }

    pub(super) fn entry(&self) -> Option<&PathBuf> {
        self.0.as_ref().map(|loaded| &loaded.launch.entry)
    }

    pub(super) fn loaded(&self) -> Option<&SharedString> {
        self.0.as_ref().map(|loaded| &loaded.name)
    }

    pub(super) fn failure(&self) -> Option<String> {
        match &self.0.as_ref()?.phase {
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

    pub(super) fn status(&self) -> Option<String> {
        let Loaded { name, phase, .. } = self.0.as_ref()?;
        match phase {
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
        if let Some(loaded) = &self.script.0 {
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
        self.script = Script(Some(Loaded {
            launch,
            name,
            phase,
        }));
        self.changed(cx);
    }

    fn script_ended(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let close_tab = outcome == Outcome::Exited { close_tab: true };
        if let Some(loaded) = &mut self.script.0 {
            loaded.phase = Phase::Finished(outcome);
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
        }) = &mut self.script.0
        {
            run.stop();
            *stopping = true;
            self.changed(cx);
        }
    }
}
