use std::path::PathBuf;

use gpui::{Context, PathPromptOptions, SharedString, Window};

use super::RuffleView;
use crate::{
    ruffle::Msg,
    scripting::{Level, Limit, Limits, Outcome, Run},
};

#[derive(Default)]
pub(super) enum Script {
    #[default]
    Idle,
    Running {
        run: Run,
        name: SharedString,
        stopping: bool,
    },
    Finished {
        name: SharedString,
        outcome: Outcome,
    },
}

impl Script {
    pub(super) fn running(&self) -> bool {
        matches!(self, Script::Running { .. })
    }

    pub(super) fn label(&self) -> SharedString {
        match self {
            Script::Idle => "None".into(),
            Script::Running { stopping: true, .. } => "Stopping".into(),
            Script::Running { name, .. } => name.clone(),
            Script::Finished { outcome, .. } => match outcome {
                Outcome::Completed => "Finished".into(),
                Outcome::Failed { .. } => "Failed".into(),
                Outcome::Stopped | Outcome::Killed(_) => "Stopped".into(),
            },
        }
    }

    pub(super) fn status(&self) -> Option<String> {
        match self {
            Script::Idle => None,
            Script::Running {
                name,
                stopping: false,
                ..
            } => Some(format!("Running {name}")),
            Script::Running { name, .. } => Some(format!("Stopping {name}")),
            Script::Finished { name, outcome } => Some(match outcome {
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
                let _ = this.update(cx, |this, cx| this.start_script(entry, cx));
            }
        })
        .detach();
    }

    fn start_script(&mut self, entry: PathBuf, cx: &mut Context<Self>) {
        if self.script.running() {
            return;
        }
        let name: SharedString = entry
            .file_name()
            .map_or_else(
                || entry.display().to_string(),
                |name| name.display().to_string(),
            )
            .into();
        let inbox = self.session.inbox();
        let (finished, outcome) = async_channel::bounded(1);
        let started = Run::start(
            entry,
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
        self.script = match started {
            Ok(run) => {
                cx.spawn(async move |this, cx| {
                    if let Ok(outcome) = outcome.recv().await {
                        let _ = this.update(cx, |this, cx| {
                            if let Script::Running { name, .. } = std::mem::take(&mut this.script) {
                                this.script = Script::Finished { name, outcome };
                            }
                            cx.notify();
                        });
                    }
                })
                .detach();
                Script::Running {
                    run,
                    name,
                    stopping: false,
                }
            }
            Err(error) => Script::Finished {
                name,
                outcome: Outcome::Failed {
                    message: error.to_string(),
                    stack: None,
                },
            },
        };
        cx.notify();
    }

    pub(super) fn stop_script(&mut self, cx: &mut Context<Self>) {
        if let Script::Running { run, stopping, .. } = &mut self.script {
            run.stop();
            *stopping = true;
            cx.notify();
        }
    }
}
