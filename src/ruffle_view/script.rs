use std::path::PathBuf;

use gpui::{Context, PathPromptOptions, SharedString, Window};

use super::RuffleView;
use crate::{
    ruffle::Msg,
    scripting::{Level, Limit, Limits, Outcome, Run},
};

#[derive(Default)]
pub(super) struct Script(Option<Loaded>);

struct Loaded {
    entry: PathBuf,
    name: SharedString,
    phase: Phase,
}

enum Phase {
    Ready,
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

    pub(super) fn loaded(&self) -> Option<&SharedString> {
        self.0.as_ref().map(|loaded| &loaded.name)
    }

    pub(super) fn label(&self) -> SharedString {
        let Some(Loaded { name, phase, .. }) = &self.0 else {
            return "None".into();
        };
        match phase {
            Phase::Ready
            | Phase::Running {
                stopping: false, ..
            } => name.clone(),
            Phase::Running { stopping: true, .. } => "Stopping".into(),
            Phase::Finished(outcome) => match outcome {
                Outcome::Completed => "Finished".into(),
                Outcome::Failed { .. } => "Failed".into(),
                Outcome::Stopped | Outcome::Killed(_) => "Stopped".into(),
            },
        }
    }

    pub(super) fn status(&self) -> Option<String> {
        let Loaded { name, phase, .. } = self.0.as_ref()?;
        match phase {
            Phase::Ready => None,
            Phase::Running {
                stopping: false, ..
            } => Some(format!("Running {name}")),
            Phase::Running { stopping: true, .. } => Some(format!("Stopping {name}")),
            Phase::Finished(outcome) => Some(match outcome {
                Outcome::Completed => format!("{name} finished"),
                Outcome::Failed { message, .. } => format!("{name} failed: {message}"),
                Outcome::Stopped => format!("{name} stopped"),
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
    pub(super) fn load_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            if let Some(entry) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| {
                    if !this.script.running() {
                        this.script = Script(Some(Loaded {
                            name: super::account::file_name(&entry).into(),
                            entry,
                            phase: Phase::Ready,
                        }));
                        this.changed(cx);
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn start_loaded_script(&mut self, cx: &mut Context<Self>) {
        if let Some(loaded) = &self.script.0 {
            self.start_script(loaded.entry.clone(), cx);
        }
    }

    pub(super) fn start_script(&mut self, entry: PathBuf, cx: &mut Context<Self>) {
        if self.script.running() {
            return;
        }
        let name: SharedString = super::account::file_name(&entry).into();
        let inbox = self.session.inbox();
        let (finished, outcome) = async_channel::bounded(1);
        let started = Run::start(
            entry.clone(),
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
                    tracing::warn!(target: "mira::script", stack = stack.as_deref().unwrap_or(""), "{message}");
                }
                let _ = finished.try_send(outcome);
            },
        );
        let phase = match started {
            Ok(run) => {
                cx.spawn(async move |this, cx| {
                    if let Ok(outcome) = outcome.recv().await {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(loaded) = &mut this.script.0 {
                                loaded.phase = Phase::Finished(outcome);
                            }
                            this.run_queued(cx);
                            this.changed(cx);
                        });
                    }
                })
                .detach();
                Phase::Running {
                    run,
                    stopping: false,
                }
            }
            Err(error) => Phase::Finished(Outcome::Failed {
                message: error.to_string(),
                stack: None,
            }),
        };
        self.script = Script(Some(Loaded { entry, name, phase }));
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
