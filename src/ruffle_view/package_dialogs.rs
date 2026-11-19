use std::path::PathBuf;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, Focusable as _, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, StyledExt as _, Tone, h_flex,
    primitives::{
        Alert, Button, Dialog, Input, InputEvent, Toast, Toaster,
        button::{Size, Variant},
    },
    v_flex,
};

use super::form::note;
use crate::packages::{self, Activity, Checked, Edits, Failure, Recommended, Updated};

const WIDTH: f32 = 440.0;
const LISTED_EDITS: usize = 8;

pub(super) enum PackageEvent {
    Installed(PathBuf),
    Changed,
}

struct Install {
    link: Entity<Input>,
    branch: Entity<Input>,
    error: Option<SharedString>,
    recommended: Vec<Recommended>,
    _changes: [Subscription; 2],
}

struct Confirm {
    root: PathBuf,
    name: SharedString,
    edits: Edits,
}

pub(super) struct PackageDialogs {
    install: Option<Install>,
    confirm: Option<Confirm>,
    _packages: Subscription,
}

impl EventEmitter<PackageEvent> for PackageDialogs {}

fn toast(title: impl Into<SharedString>, tone: Tone, body: Option<String>, cx: &mut gpui::App) {
    let toast = Toast::new(title).tone(tone);
    Toaster::push(
        match body {
            Some(body) => toast.body(body),
            None => toast,
        },
        cx,
    );
}

impl PackageDialogs {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            install: None,
            confirm: None,
            _packages: cx.observe(&packages::packages(cx), |_, _, cx| cx.notify()),
        }
    }

    pub(super) fn open_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let link = cx.new(|cx| Input::new("GitHub link", cx).placeholder("github.com/owner/repo"));
        let branch = cx.new(|cx| Input::new("Branch", cx).placeholder("Default branch"));
        let changes = [
            cx.subscribe(&link, Self::input_changed),
            cx.subscribe(&branch, Self::input_changed),
        ];
        window.focus(&link.focus_handle(cx), cx);
        self.install = Some(Install {
            link,
            branch,
            error: None,
            recommended: packages::packages(cx).read(cx).recommended(),
            _changes: changes,
        });
        cx.notify();
    }

    fn input_changed(&mut self, _: Entity<Input>, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Submitted => self.install_link(cx),
            InputEvent::Changed => {
                if let Some(install) = &mut self.install {
                    install.error = None;
                }
                cx.notify();
            }
        }
    }

    fn close_install(&mut self, cx: &mut Context<Self>) {
        self.install = None;
        cx.notify();
    }

    fn install_link(&mut self, cx: &mut Context<Self>) {
        let Some(install) = &self.install else {
            return;
        };
        let link = install.link.read(cx).text().trim().to_owned();
        if link.is_empty() {
            return;
        }
        let branch = install.branch.read(cx).text().to_owned();
        let task = packages::packages(cx)
            .update(cx, |packages, cx| packages.install_link(&link, &branch, cx));
        self.finish_install(task, cx);
    }

    fn install_recommended(&mut self, recommended: Recommended, cx: &mut Context<Self>) {
        let task = packages::packages(cx)
            .update(cx, |packages, cx| packages.install(recommended.locator, cx));
        self.finish_install(task, cx);
    }

    fn finish_install(
        &mut self,
        task: gpui::Task<Result<PathBuf, Failure>>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(root) => {
                    this.close_install(cx);
                    cx.emit(PackageEvent::Installed(root));
                }
                Err(Failure::AlreadyInstalled(root)) => {
                    this.close_install(cx);
                    toast("Already installed", Tone::Neutral, None, cx);
                    cx.emit(PackageEvent::Installed(root));
                }
                Err(Failure::Other(message)) => {
                    if let Some(install) = &mut this.install {
                        install.error = Some(message.into());
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn update(
        &mut self,
        root: PathBuf,
        name: SharedString,
        overwrite_edits: bool,
        cx: &mut Context<Self>,
    ) {
        self.confirm = None;
        let task = packages::packages(cx).update(cx, |packages, cx| {
            packages.update(root.clone(), overwrite_edits, cx)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Updated::Current) => {
                        toast(format!("{name} is up to date"), Tone::Neutral, None, cx);
                    }
                    Ok(Updated::Edited(edits)) => {
                        this.confirm = Some(Confirm { root, name, edits });
                    }
                    Ok(Updated::Replaced) => {
                        toast(format!("Updated {name}"), Tone::Success, None, cx);
                        cx.emit(PackageEvent::Changed);
                    }
                    Err(message) => {
                        toast(
                            format!("Couldn't update {name}"),
                            Tone::Danger,
                            Some(message),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn remove(&mut self, root: PathBuf, name: SharedString, cx: &mut Context<Self>) {
        let task = packages::packages(cx).update(cx, |packages, cx| packages.remove(root, cx));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |_, cx| match result {
                Ok(()) => {
                    toast(
                        format!("Moved {name} to the Trash"),
                        Tone::Neutral,
                        None,
                        cx,
                    );
                    cx.emit(PackageEvent::Changed);
                }
                Err(message) => {
                    toast(
                        format!("Couldn't remove {name}"),
                        Tone::Danger,
                        Some(message),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    pub(super) fn check(&mut self, cx: &mut Context<Self>) {
        let task = packages::packages(cx).update(cx, |packages, cx| packages.check(false, cx));
        cx.spawn(async move |_, cx| {
            let result = task.await;
            cx.update(|cx| match result {
                Ok(Checked { failed, .. }) if failed > 0 => toast(
                    if failed == 1 {
                        "Couldn't check 1 package".to_owned()
                    } else {
                        format!("Couldn't check {failed} packages")
                    },
                    Tone::Danger,
                    Some("Open Sources to see why.".into()),
                    cx,
                ),
                Ok(Checked { available: 0, .. }) => {
                    toast("Your packages are up to date", Tone::Neutral, None, cx);
                }
                Ok(Checked { available: 1, .. }) => {
                    toast("1 update available", Tone::Accent, None, cx);
                }
                Ok(Checked { available, .. }) => {
                    toast(
                        format!("{available} updates available"),
                        Tone::Accent,
                        None,
                        cx,
                    );
                }
                Err(message) => {
                    toast(
                        "Couldn't check for updates",
                        Tone::Danger,
                        Some(message),
                        cx,
                    );
                }
            })
        })
        .detach();
    }
}

fn edit_lines(edits: &Edits) -> Vec<String> {
    let tagged = |paths: &[crate::packages::PackagePath], tag: &str| {
        paths
            .iter()
            .map(|path| format!("{path}{tag}"))
            .collect::<Vec<_>>()
    };
    let mut lines = tagged(&edits.changed, "");
    lines.extend(tagged(&edits.added, " (added)"));
    lines.extend(tagged(&edits.removed, " (deleted)"));
    if lines.len() > LISTED_EDITS {
        let more = lines.len() - (LISTED_EDITS - 1);
        lines.truncate(LISTED_EDITS - 1);
        lines.push(format!("and {more} more"));
    }
    lines
}

impl Render for PackageDialogs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let installing = matches!(
            packages::packages(cx).read(cx).activity(),
            Some(Activity::Installing)
        );
        let busy = packages::packages(cx).read(cx).activity().is_some();
        let install = self.install.as_ref().map(|install| {
            let label = |text: &'static str| {
                div()
                    .text_small()
                    .text_color(theme.text_secondary)
                    .child(text)
            };
            v_flex()
                .gap(px(12.0))
                .child(
                    v_flex()
                        .gap(px(4.0))
                        .child(label("GitHub link"))
                        .child(install.link.clone())
                        .child(note(
                            "Its package.json needs a \u{201c}mira\u{201d} field.",
                            true,
                            &theme,
                        )),
                )
                .child(
                    v_flex()
                        .gap(px(4.0))
                        .child(label("Branch"))
                        .child(install.branch.clone()),
                )
                .when_some(install.error.clone(), |body, error| {
                    body.child(
                        Alert::new("install-error", Tone::Danger, "Couldn't install").body(error),
                    )
                })
                .when(!install.recommended.is_empty(), |body| {
                    body.child(
                        v_flex()
                            .gap(px(6.0))
                            .pt(px(4.0))
                            .child(label("Recommended"))
                            .children(install.recommended.iter().enumerate().map(
                                |(index, recommended)| {
                                    let chosen = recommended.clone();
                                    h_flex()
                                        .gap(px(12.0))
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(recommended.name.clone()),
                                                )
                                                .when_some(
                                                    recommended.description.clone(),
                                                    |column, description| {
                                                        column.child(note(
                                                            description,
                                                            true,
                                                            &theme,
                                                        ))
                                                    },
                                                ),
                                        )
                                        .child(
                                            Button::new(("install-recommended", index), "Install")
                                                .size(Size::Small)
                                                .disabled(busy)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.install_recommended(chosen.clone(), cx)
                                                })),
                                        )
                                },
                            )),
                    )
                })
        });
        let has_link = self
            .install
            .as_ref()
            .is_some_and(|install| !install.link.read(cx).text().trim().is_empty());
        let confirm = self.confirm.as_ref().map(|confirm| {
            v_flex()
                .gap(px(2.0))
                .children(edit_lines(&confirm.edits).into_iter().map(|line| {
                    div()
                        .text_small()
                        .truncate()
                        .text_color(theme.text_secondary)
                        .child(line)
                }))
        });
        let confirm_title = self
            .confirm
            .as_ref()
            .map(|confirm| format!("Update {}?", confirm.name))
            .unwrap_or_default();
        div()
            .child(
                Dialog::new("install-package", self.install.is_some(), "Install package")
                    .width(px(WIDTH))
                    .when_some(install, |dialog, body| dialog.body(body))
                    .action(
                        Button::new("cancel-install", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close_install(cx))),
                    )
                    .action(
                        Button::new(
                            "confirm-install",
                            if installing { "Installing…" } else { "Install" },
                        )
                        .variant(Variant::Primary)
                        .disabled(busy || !has_link)
                        .on_click(cx.listener(|this, _, _, cx| this.install_link(cx))),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.close_install(cx));
                        }
                    }),
            )
            .child(
                Dialog::new("confirm-update", self.confirm.is_some(), confirm_title)
                    .alert()
                    .description(
                        "You changed these files since installing. Updating replaces them, and your copy goes to the Trash.",
                    )
                    .when_some(confirm, |dialog, body| dialog.body(body))
                    .action(Button::new("keep-edits", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.confirm = None;
                            cx.notify();
                        },
                    )))
                    .action(
                        Button::new("overwrite-edits", "Update")
                            .variant(Variant::Primary)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(confirm) = this.confirm.take() {
                                    this.update(confirm.root, confirm.name, true, cx);
                                }
                            })),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.confirm = None;
                                cx.notify();
                            });
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::size;

    use super::*;
    use crate::packages::PackagePath;

    fn dialogs(
        name: &str,
        prepare: impl Fn(&mut PackageDialogs, &mut Window, &mut Context<PackageDialogs>)
        + Copy
        + 'static,
    ) {
        for written in mira_ui::snapshot(name, size(px(720.0), px(500.0)), move |window, cx| {
            let scratch = std::env::temp_dir().join(format!("mira-{name}"));
            packages::init(
                scratch.join("packages"),
                scratch.join("skua"),
                scratch.join("token"),
                cx,
            );
            cx.new(|cx| {
                let mut dialogs = PackageDialogs::new(cx);
                prepare(&mut dialogs, window, cx);
                dialogs
            })
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_install_package() {
        dialogs("install-package", |dialogs, window, cx| {
            dialogs.open_install(window, cx);
            let install = dialogs.install.as_mut().unwrap();
            install.recommended = vec![
                Recommended {
                    name: "Dailies".into(),
                    description: Some("Daily quests, spins and boss runs.".into()),
                    locator: crate::packages::Locator {
                        owner: "toommyliu".into(),
                        repo: "mira".into(),
                        branch: Some("main".into()),
                        path: PackagePath::parse("script-packages/dailies").unwrap(),
                    },
                },
                Recommended {
                    name: "Consumables".into(),
                    description: None,
                    locator: crate::packages::Locator {
                        owner: "toommyliu".into(),
                        repo: "mira".into(),
                        branch: Some("main".into()),
                        path: PackagePath::parse("script-packages/consumables").unwrap(),
                    },
                },
            ];
        });
    }

    #[test]
    fn snapshot_install_package_error() {
        dialogs("install-package-error", |dialogs, window, cx| {
            dialogs.open_install(window, cx);
            let install = dialogs.install.as_mut().unwrap();
            install.error = Some(
                "This isn't a Mira script package. Its package.json needs a \u{201c}mira\u{201d} field."
                    .into(),
            );
        });
    }

    #[test]
    fn snapshot_confirm_update() {
        dialogs("confirm-package-update", |dialogs, _, _| {
            let path = |text: &str| PackagePath::parse(text).unwrap();
            dialogs.confirm = Some(Confirm {
                root: "toommyliu-mira-dailies".into(),
                name: "Dailies".into(),
                edits: Edits {
                    changed: vec![path("scripts/spin.ts"), path("lib/wheel.ts")],
                    added: vec![path("scripts/my-farm.ts")],
                    removed: vec![path("scripts/old.ts")],
                },
            });
        });
    }
}
