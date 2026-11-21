use std::{collections::VecDeque, path::PathBuf};

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, FontWeight, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, StyledExt as _, Tone, h_flex,
    primitives::{
        Alert, Button, Dialog, Input, InputEvent, Toast, ToastId, Toaster,
        button::{Size, Variant},
    },
    v_flex,
};

use super::form::note;
use crate::packages::{self, Activity, Edits, Failure, Installed, Recommended, Updated};

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

struct Uninstall {
    root: PathBuf,
    name: SharedString,
    error: Option<SharedString>,
}

#[derive(Clone)]
pub(super) struct Pending {
    pub(super) root: PathBuf,
    pub(super) name: SharedString,
    pub(super) from: SharedString,
    pub(super) to: SharedString,
}

#[derive(Clone, Copy)]
enum Progress {
    Dialog,
    Toast(ToastId),
}

pub(super) struct PackageDialogs {
    install: Option<Install>,
    installing: Option<Progress>,
    confirm: Option<Confirm>,
    uninstall: Option<Uninstall>,
    review: Option<Vec<Pending>>,
    queue: VecDeque<(PathBuf, SharedString)>,
    toasts: Entity<Toaster>,
    _packages: Subscription,
}

impl EventEmitter<PackageEvent> for PackageDialogs {}

impl PackageDialogs {
    pub(super) fn new(toasts: Entity<Toaster>, cx: &mut Context<Self>) -> Self {
        Self {
            install: None,
            installing: None,
            confirm: None,
            uninstall: None,
            review: None,
            queue: VecDeque::new(),
            toasts,
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
        if let Some(Progress::Dialog) = self.installing {
            let toast = Toast::new("Installing package…").loading();
            self.installing = Some(Progress::Toast(Toaster::push(&self.toasts, toast, cx)));
        }
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
        self.begin_install(cx, |cx| {
            packages::packages(cx)
                .update(cx, |packages, cx| packages.install_link(&link, &branch, cx))
        });
    }

    fn install_recommended(&mut self, recommended: Recommended, cx: &mut Context<Self>) {
        self.begin_install(cx, |cx| {
            packages::packages(cx)
                .update(cx, |packages, cx| packages.install(recommended.locator, cx))
        });
    }

    fn begin_install(
        &mut self,
        cx: &mut Context<Self>,
        start: impl FnOnce(&mut App) -> gpui::Task<Result<Installed, Failure>>,
    ) {
        if self.installing.is_some() {
            return;
        }
        let task = start(cx);
        self.installing = Some(Progress::Dialog);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(Progress::Toast(loading)) = this.installing.take() {
                    let toast = match result {
                        Ok(Installed { name, .. }) => {
                            cx.emit(PackageEvent::Changed);
                            Toast::new(format!("Installed {name}")).tone(Tone::Success)
                        }
                        Err(Failure::AlreadyInstalled(_)) => Toast::new("Already installed"),
                        Err(Failure::Other(message)) => Toast::new("Couldn't install package")
                            .body(message)
                            .tone(Tone::Danger),
                    };
                    Toaster::replace(&this.toasts, loading, toast, cx);
                    return;
                }
                match result {
                    Ok(Installed { root, .. }) | Err(Failure::AlreadyInstalled(root)) => {
                        this.close_install(cx);
                        cx.emit(PackageEvent::Installed(root));
                    }
                    Err(Failure::Other(message)) => {
                        if let Some(install) = &mut this.install {
                            install.error = Some(message.into());
                        }
                        cx.notify();
                    }
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
                    Ok(Updated::Edited(edits)) => {
                        this.confirm = Some(Confirm { root, name, edits });
                    }
                    Ok(Updated::Replaced) => {
                        cx.emit(PackageEvent::Changed);
                        this.advance(cx);
                    }
                    Ok(Updated::Current) | Err(_) => this.advance(cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn review_updates(&mut self, pending: Vec<Pending>, cx: &mut Context<Self>) {
        self.review = (!pending.is_empty()).then_some(pending);
        cx.notify();
    }

    fn update_all(&mut self, cx: &mut Context<Self>) {
        if let Some(review) = self.review.take() {
            self.queue = review
                .into_iter()
                .map(|pending| (pending.root, pending.name))
                .collect();
            self.advance(cx);
        }
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        if let Some((root, name)) = self.queue.pop_front() {
            self.update(root, name, false, cx);
        }
    }

    fn skip_edited(&mut self, cx: &mut Context<Self>) {
        self.confirm = None;
        self.advance(cx);
        cx.notify();
    }

    pub(super) fn open_uninstall(
        &mut self,
        root: PathBuf,
        name: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.uninstall = Some(Uninstall {
            root,
            name,
            error: None,
        });
        cx.notify();
    }

    fn close_uninstall(&mut self, cx: &mut Context<Self>) {
        self.uninstall = None;
        cx.notify();
    }

    fn confirm_uninstall(&mut self, cx: &mut Context<Self>) {
        let Some(uninstall) = &mut self.uninstall else {
            return;
        };
        uninstall.error = None;
        let root = uninstall.root.clone();
        let task = packages::packages(cx).update(cx, |packages, cx| packages.uninstall(root, cx));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.uninstall = None;
                        cx.emit(PackageEvent::Changed);
                    }
                    Err(message) => {
                        if let Some(uninstall) = &mut this.uninstall {
                            uninstall.error = Some(message.into());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn check(&mut self, cx: &mut Context<Self>) {
        let task = packages::packages(cx).update(cx, |packages, cx| packages.check(false, cx));
        cx.spawn(async move |_, _| {
            if let Err(message) = task.await {
                tracing::warn!(%message, "couldn't check packages for updates");
            }
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
        let layout = theme.layout;
        let typography = theme.typography;
        let installing = matches!(
            packages::packages(cx).read(cx).activity(),
            Some(Activity::Installing)
        );
        let busy = packages::packages(cx).read(cx).activity().is_some();
        let install = self.install.as_ref().map(|install| {
            let label = |text: &'static str| {
                div()
                    .text_caption(typography)
                    .text_color(theme.muted_foreground)
                    .child(text)
            };
            v_flex()
                .gap(layout.space(3.0))
                .child(
                    v_flex()
                        .gap(layout.space(1.0))
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
                        .gap(layout.space(1.0))
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
                            .gap(layout.space(1.5))
                            .pt(layout.space(1.0))
                            .child(label("Recommended"))
                            .children(install.recommended.iter().enumerate().map(
                                |(index, recommended)| {
                                    let chosen = recommended.clone();
                                    h_flex()
                                        .gap(layout.space(3.0))
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
                .gap(layout.space(0.5))
                .children(edit_lines(&confirm.edits).into_iter().map(|line| {
                    div()
                        .text_caption(typography)
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(line)
                }))
        });
        let confirm_title = self
            .confirm
            .as_ref()
            .map(|confirm| format!("Update {}?", confirm.name))
            .unwrap_or_default();
        let uninstall_title = self
            .uninstall
            .as_ref()
            .map(|uninstall| format!("Uninstall {}?", uninstall.name))
            .unwrap_or_default();
        let removing = self.uninstall.as_ref().is_some_and(|uninstall| {
            matches!(
                packages::packages(cx).read(cx).activity(),
                Some(Activity::Removing(root)) if *root == uninstall.root
            )
        });
        let uninstall_error = self
            .uninstall
            .as_ref()
            .and_then(|uninstall| uninstall.error.clone())
            .map(|error| {
                Alert::new("uninstall-error", Tone::Danger, "Couldn't uninstall").body(error)
            });
        let review_title = match self.review.as_deref() {
            Some([pending]) => format!("Update {}?", pending.name),
            Some(pending) => format!("Update {} packages?", pending.len()),
            None => String::new(),
        };
        let review = self.review.as_ref().map(|pending| {
            v_flex()
                .gap(layout.space(1.0))
                .children(pending.iter().map(|pending| {
                    h_flex()
                        .justify_between()
                        .gap(layout.space(3.0))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_title(typography)
                                .child(pending.name.clone()),
                        )
                        .child(
                            h_flex()
                                .flex_none()
                                .gap(layout.space(1.0))
                                .font_family(theme.mono_family.clone())
                                .text_size(typography.mono)
                                .text_color(theme.muted_foreground)
                                .child(pending.from.clone())
                                .child("→")
                                .child(
                                    div().text_color(theme.foreground).child(pending.to.clone()),
                                ),
                        )
                }))
        });
        div()
            .child(
                Dialog::new("install-package", self.install.is_some(), "Install package")
                    .width(px(WIDTH))
                    .when_some(install, |dialog, body| dialog.body(body))
                    .action(
                        Button::new("cancel-install", if installing { "Close" } else { "Cancel" })
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
                    .action(
                        Button::new("keep-edits", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.skip_edited(cx))),
                    )
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
                            let _ = this.update(cx, |this, cx| this.skip_edited(cx));
                        }
                    }),
            )
            .child(
                Dialog::new("uninstall-package", self.uninstall.is_some(), uninstall_title)
                    .alert()
                    .description(
                        "Mira stops listing and updating it, and its files go to the Trash.",
                    )
                    .when_some(uninstall_error, |dialog, error| dialog.body(error))
                    .action(
                        Button::new("cancel-uninstall", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close_uninstall(cx))),
                    )
                    .action(
                        Button::new(
                            "confirm-uninstall",
                            if removing { "Uninstalling…" } else { "Uninstall" },
                        )
                        .variant(Variant::Danger)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_uninstall(cx))),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.close_uninstall(cx));
                        }
                    }),
            )
            .child(
                Dialog::new("review-updates", self.review.is_some(), review_title)
                    .alert()
                    .description("Each package is replaced with its newest version from GitHub.")
                    .when_some(review, |dialog, body| dialog.body(body))
                    .action(Button::new("cancel-updates", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.review = None;
                            cx.notify();
                        },
                    )))
                    .action(
                        Button::new("confirm-updates", "Update")
                            .variant(Variant::Primary)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.update_all(cx))),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.review = None;
                                cx.notify();
                            });
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, io::Write as _, rc::Rc};

    use gpui::size;

    use super::*;
    use crate::packages::{
        PackagePath,
        fake::{Fake, Reply},
    };

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
                let mut dialogs = PackageDialogs::new(Toaster::new(window, cx), cx);
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

    #[gpui::test]
    fn update_all_updates_each_package_in_turn(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let root =
            std::env::temp_dir().join(format!("mira-update-all-{:x}", u64::from_ne_bytes(random)));
        let receipt = r#"{"repository":1,"owner":"o","repo":"r","path":"","branch":"main","commit":"1111111111111111111111111111111111111111","tree":"2222222222222222222222222222222222222222","files":{}}"#;
        for dir in ["first", "second"] {
            let package = root.join("packages").join(dir);
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(package.join("package.json"), r#"{"name":"P","mira":{}}"#).unwrap();
            std::fs::write(package.join(crate::packages::RECEIPT), receipt).unwrap();
        }
        let fake = Fake::start();
        cx.update(mira_ui::init);
        let (dialogs, cx) = cx.add_window_view(|window, cx| {
            packages::init_against(fake.url(), root.join("packages"), root.join("skua"), cx);
            PackageDialogs::new(Toaster::new(window, cx), cx)
        });
        let pending = |dir: &str| Pending {
            root: root.join("packages").join(dir),
            name: dir.to_owned().into(),
            from: "1111111".into(),
            to: "3333333".into(),
        };
        dialogs.update(cx, |dialogs, cx| {
            dialogs.review_updates(vec![pending("first"), pending("second")], cx);
            dialogs.update_all(cx);
        });
        cx.run_until_parked();
        let packages = cx.update(|_, cx| packages::packages(cx));
        packages.read_with(cx, |packages, _| {
            for dir in ["first", "second"] {
                let status = packages.status(&root.join("packages").join(dir));
                assert!(
                    matches!(
                        status,
                        Some(packages::Status::Failed(message)) if message.starts_with("Couldn't find o/r")
                    ),
                    "{dir}: {status:?}",
                );
            }
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn a_failed_uninstall_keeps_its_dialog_open_with_the_error(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let root =
            std::env::temp_dir().join(format!("mira-uninstall-{:x}", u64::from_ne_bytes(random)));
        std::fs::create_dir_all(root.join("packages")).unwrap();
        let fake = Fake::start();
        cx.update(mira_ui::init);
        let (dialogs, cx) = cx.add_window_view(|window, cx| {
            packages::init_against(fake.url(), root.join("packages"), root.join("skua"), cx);
            PackageDialogs::new(Toaster::new(window, cx), cx)
        });
        dialogs.update(cx, |dialogs, cx| {
            dialogs.open_uninstall(root.join("packages/gone"), "Gone".into(), cx);
            dialogs.confirm_uninstall(cx);
        });
        cx.run_until_parked();
        dialogs.read_with(cx, |dialogs, _| {
            let uninstall = dialogs.uninstall.as_ref().expect("the dialog stays open");
            assert!(
                uninstall
                    .error
                    .as_ref()
                    .is_some_and(|error| error.starts_with("Couldn't move it to the Trash")),
                "{:?}",
                uninstall.error
            );
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    fn serve_dailies(fake: &Fake) {
        let commit = "1".repeat(40);
        let manifest = r#"{"name":"Dailies","mira":{}}"#;
        fake.route(
            "/repos/o/r",
            Reply::json(r#"{"id":1,"name":"r","owner":{"login":"o"},"default_branch":"main"}"#),
        );
        fake.route(
            "/repos/o/r/branches/main",
            Reply::json(format!(
                r#"{{"commit":{{"sha":"{commit}","commit":{{"tree":{{"sha":"{commit}"}}}}}}}}"#
            )),
        );
        fake.route(
            &format!("/repos/o/r/contents/package.json?ref={commit}"),
            Reply::bytes(manifest.into()),
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in [
            ("o-r-1111111/package.json", manifest),
            ("o-r-1111111/scripts/spin.ts", ""),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        fake.route(
            &format!("/repos/o/r/zipball/{commit}"),
            Reply::bytes(zip.finish().unwrap().into_inner()),
        );
    }

    fn start_install(
        dialogs: &mut PackageDialogs,
        link: &str,
        close: bool,
        window: &mut Window,
        cx: &mut Context<PackageDialogs>,
    ) {
        dialogs.open_install(window, cx);
        let input = dialogs.install.as_ref().unwrap().link.clone();
        input.update(cx, |input, cx| input.set_text(link.to_owned(), cx));
        dialogs.install_link(cx);
        if close {
            dialogs.close_install(cx);
        }
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!("mira-{label}-{:x}", u64::from_ne_bytes(random)))
    }

    struct Host {
        dialogs: Entity<PackageDialogs>,
        toasts: Entity<Toaster>,
    }

    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.dialogs.clone())
                .child(Toaster::layer(&self.toasts, window, cx))
        }
    }

    #[gpui::test]
    fn install_results_reach_the_user_after_the_dialog_closes(cx: &mut gpui::TestAppContext) {
        let root = scratch("install");
        let fake = Fake::start();
        serve_dailies(&fake);
        cx.update(mira_ui::init);
        let (host, cx) = cx.add_window_view(|window, cx| {
            packages::init_against(fake.url(), root.join("packages"), root.join("skua"), cx);
            let toasts = Toaster::new(window, cx);
            Host {
                dialogs: cx.new(|cx| PackageDialogs::new(toasts.clone(), cx)),
                toasts,
            }
        });
        let dialogs = host.read_with(cx, |host, _| host.dialogs.clone());
        let events = Rc::new(RefCell::new(Vec::new()));
        let _events = cx.update(|_, cx| {
            let events = events.clone();
            cx.subscribe(&dialogs, move |_, event: &PackageEvent, _| {
                events.borrow_mut().push(match event {
                    PackageEvent::Installed(root) => format!("show {}", root.display()),
                    PackageEvent::Changed => "rescan".to_owned(),
                });
            })
        });
        let loading = "toast: Installing package…";
        let install = |link: &str, close: bool, cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                dialogs.update(cx, |dialogs, cx| {
                    start_install(dialogs, link, close, window, cx)
                });
                window.draw(cx).clear(cx);
            });
            assert_eq!(cx.debug_bounds(loading).is_some(), close, "{link}");
            cx.run_until_parked();
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            assert!(cx.debug_bounds(loading).is_none(), "{link}");
        };

        install("github.com/o/missing", false, cx);
        dialogs.read_with(cx, |dialogs, _| {
            let error = dialogs
                .install
                .as_ref()
                .and_then(|install| install.error.clone());
            assert!(
                error
                    .as_ref()
                    .is_some_and(|error| error.starts_with("Couldn't find o/missing")),
                "{error:?}"
            );
        });
        assert!(cx.debug_bounds("toast: Couldn't install package").is_none());
        dialogs.update(cx, |dialogs, cx| dialogs.close_install(cx));

        install("github.com/o/missing", true, cx);
        assert!(cx.debug_bounds("toast: Couldn't install package").is_some());

        install("github.com/o/r", true, cx);
        assert!(cx.debug_bounds("toast: Installed Dailies").is_some());
        assert!(root.join("packages/o-r/scripts/spin.ts").exists());
        assert_eq!(*events.borrow(), ["rescan"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_review_updates() {
        dialogs("review-package-updates", |dialogs, _, cx| {
            let pending = |dir: &str, name: &str, from: &str, to: &str| Pending {
                root: dir.into(),
                name: name.to_owned().into(),
                from: from.to_owned().into(),
                to: to.to_owned().into(),
            };
            dialogs.review_updates(
                vec![
                    pending("toommyliu-mira-dailies", "Dailies", "4e1c2f0", "9a8b7c6"),
                    pending("farming", "Farming", "1d2e3f4", "5a6b7c8"),
                ],
                cx,
            );
        });
    }

    #[test]
    fn snapshot_uninstall_package() {
        dialogs("uninstall-package", |dialogs, _, cx| {
            dialogs.open_uninstall("toommyliu-mira-dailies".into(), "Dailies".into(), cx);
        });
        dialogs("uninstall-package-error", |dialogs, _, cx| {
            dialogs.open_uninstall("toommyliu-mira-dailies".into(), "Dailies".into(), cx);
            if let Some(uninstall) = &mut dialogs.uninstall {
                uninstall.error =
                    Some("Couldn't move it to the Trash: Operation not permitted.".into());
            }
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
