use std::{collections::HashMap, future::Future, path::PathBuf, sync::Arc};

use gpui::{App, AppContext as _, Context, Entity, Global, Task};

use super::{
    Sha,
    bundled::{self, Recommended},
    github::{API, GitHub, Locator},
    service::{self, Failure, Updated},
    store::{self, Store},
    token,
};

const BUSY: &str = "Wait for the current package action to finish.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Installing,
    Checking,
    Updating(String),
    Removing(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Available(Sha),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) available: usize,
    pub(crate) failed: usize,
}

pub(crate) struct Packages {
    api: String,
    resources: Option<PathBuf>,
    store: Store,
    token_path: PathBuf,
    token: Option<String>,
    activity: Option<Activity>,
    status: HashMap<String, Status>,
}

struct Shared(Entity<Packages>);

impl Global for Shared {}

pub(crate) fn init(root: PathBuf, token_path: PathBuf, cx: &mut App) {
    let token = token::load(&token_path).unwrap_or_else(|error| {
        tracing::warn!(%error, path = %token_path.display(), "couldn't read the GitHub token");
        None
    });
    let store = Store::new(root, Arc::new(store::move_to_trash));
    let packages = cx.new(|_| Packages::new(API.into(), store, token_path, token));
    cx.set_global(Shared(packages));
}

#[cfg(test)]
pub(crate) fn init_against(api: String, root: PathBuf, cx: &mut App) {
    let store = Store::new(root, Arc::new(store::move_to_trash));
    let packages = cx.new(|_| Packages::new(api, store, PathBuf::new(), None));
    cx.set_global(Shared(packages));
}

pub(crate) fn start(cx: &mut App) {
    let resources = bundled::resources();
    packages(cx).update(cx, |packages, cx| {
        packages.resources = resources.clone();
        packages.activity = Some(Activity::Checking);
        cx.notify();
        let store = packages.store.clone();
        let prepared = cx.background_spawn(async move {
            if let Some(resources) = &resources
                && let Err(error) = bundled::seed(resources, &store)
            {
                tracing::warn!(%error, "couldn't add the bundled packages");
            }
            store.sweep()
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = prepared.await {
                tracing::warn!(%error, "couldn't clean up unfinished package downloads");
            }
            let Ok(checked) = this.update(cx, |this, cx| {
                this.activity = None;
                this.check(true, cx)
            }) else {
                return;
            };
            if let Err(message) = checked.await {
                tracing::info!(%message, "couldn't check packages for updates");
            }
        })
        .detach();
    });
}

pub(crate) fn packages(cx: &App) -> Entity<Packages> {
    cx.global::<Shared>().0.clone()
}

impl Packages {
    fn new(api: String, store: Store, token_path: PathBuf, token: Option<String>) -> Self {
        Self {
            api,
            resources: None,
            store,
            token_path,
            token,
            activity: None,
            status: HashMap::new(),
        }
    }

    pub(crate) fn activity(&self) -> Option<&Activity> {
        self.activity.as_ref()
    }

    pub(crate) fn status(&self, dir: &str) -> Option<&Status> {
        self.status.get(dir)
    }

    #[cfg(test)]
    pub(crate) fn mark(&mut self, dir: &str, status: Status) {
        self.status.insert(dir.to_owned(), status);
    }

    pub(crate) fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub(crate) fn set_token(&mut self, text: &str, cx: &mut Context<Self>) -> std::io::Result<()> {
        token::save(&self.token_path, text)?;
        self.token = Some(text.trim().to_owned()).filter(|token| !token.is_empty());
        cx.notify();
        Ok(())
    }

    pub(crate) fn recommended(&self) -> Vec<Recommended> {
        self.resources
            .as_ref()
            .map(|resources| bundled::recommended(resources, &self.store))
            .unwrap_or_default()
    }

    pub(crate) fn install_link(
        &mut self,
        link: &str,
        branch: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, Failure>> {
        match Locator::parse(link, branch) {
            Ok(locator) => self.install(locator, cx),
            Err(message) => Task::ready(Err(Failure::Other(message))),
        }
    }

    pub(crate) fn install(
        &mut self,
        locator: Locator,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, Failure>> {
        self.start(Activity::Installing, cx, async move |github, store| {
            service::install(&github, &store, &locator).await
        })
    }

    pub(crate) fn check(
        &mut self,
        quiet: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Checked, String>> {
        let work = self.start(Activity::Checking, cx, async move |github, store| {
            let mut results = Vec::new();
            for (dir, receipt) in store.receipts() {
                results.push((dir, service::check(&github, &receipt).await));
            }
            Ok::<_, String>(results)
        });
        cx.spawn(async move |this, cx| {
            let results = work.await?;
            this.update(cx, |this, cx| {
                let mut checked = Checked::default();
                for (dir, result) in results {
                    match result {
                        Ok(Some(head)) => {
                            checked.available += 1;
                            this.status.insert(dir, Status::Available(head.commit));
                        }
                        Ok(None) => {
                            this.status.remove(&dir);
                        }
                        Err(message) if quiet => {
                            tracing::info!(%dir, %message, "couldn't check a package for updates");
                        }
                        Err(message) => {
                            checked.failed += 1;
                            this.status.insert(dir, Status::Failed(message));
                        }
                    }
                }
                cx.notify();
                checked
            })
            .map_err(|error| error.to_string())
        })
    }

    pub(crate) fn update(
        &mut self,
        dir: String,
        overwrite_edits: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Updated, String>> {
        let target = dir.clone();
        let work = self.start(
            Activity::Updating(dir.clone()),
            cx,
            async move |github, store| {
                service::update(&github, &store, &target, overwrite_edits).await
            },
        );
        let settled = self.settle(
            dir.clone(),
            work,
            |updated| !matches!(updated, Updated::Edited(_)),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = settled.await;
            if let Err(message) = &result {
                let _ = this.update(cx, |this, cx| {
                    this.status.insert(dir, Status::Failed(message.clone()));
                    cx.notify();
                });
            }
            result
        })
    }

    pub(crate) fn uninstall(
        &mut self,
        dir: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let target = dir.clone();
        let work = self.start(
            Activity::Removing(dir.clone()),
            cx,
            async move |_, store| {
                store
                    .uninstall(&target)
                    .map_err(|error| format!("Couldn't move it to the Trash: {error}."))
            },
        );
        self.settle(dir, work, |_| true, cx)
    }

    fn settle<T: 'static>(
        &mut self,
        dir: String,
        work: Task<Result<T, String>>,
        clears: fn(&T) -> bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<T, String>> {
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Ok(value) = &result
                && clears(value)
            {
                let _ = this.update(cx, |this, cx| {
                    this.status.remove(&dir);
                    cx.notify();
                });
            }
            result
        })
    }

    fn start<T, E, F, Fut>(
        &mut self,
        activity: Activity,
        cx: &mut Context<Self>,
        work: F,
    ) -> Task<Result<T, E>>
    where
        T: Send + 'static,
        E: From<String> + Send + 'static,
        F: FnOnce(GitHub, Store) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>>,
    {
        if self.activity.is_some() {
            return Task::ready(Err(E::from(BUSY.to_owned())));
        }
        self.activity = Some(activity);
        cx.notify();
        let store = self.store.clone();
        let token = self.token.clone();
        let api = self.api.clone();
        let work = cx.background_spawn(async move {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| E::from(format!("Couldn't start the download: {error}.")))?;
            let github = GitHub::at(&api, token).map_err(|error| E::from(format!("{error}.")))?;
            runtime.block_on(work(github, store))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.activity = None;
                cx.notify();
            });
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::TestAppContext;

    use super::*;
    use crate::packages::{
        archive::tests::scratch,
        fake::{Fake, Reply},
        store::tests::{Bin, receipt, stage_files},
    };

    #[gpui::test]
    async fn package_actions_wait_for_startup_cleanup(cx: &mut TestAppContext) {
        let root = scratch("state");
        let (_bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        let fake = Fake::start();
        let packages = cx.new(|_| Packages::new(fake.url(), store, root.join("token"), None));
        cx.update(|cx| {
            cx.set_global(Shared(packages.clone()));
            start(cx);
        });
        let early = packages
            .update(cx, |packages, cx| {
                packages.install_link("github.com/o/r", "", cx)
            })
            .await;
        assert_eq!(early, Err(Failure::Other(BUSY.into())));
        cx.run_until_parked();
        packages.read_with(cx, |packages, _| assert_eq!(packages.activity(), None));
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    async fn uninstall_clears_update_status_and_skips_the_package_in_future_checks(
        cx: &mut TestAppContext,
    ) {
        let root = scratch("state");
        let (_bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        let (staged, files) = stage_files(&store, &[("package.json", "{}"), ("scripts/a.ts", "a")]);
        store.publish(staged, "o-r", &receipt(files)).unwrap();
        let fake = Fake::start();
        let packages = cx.new(|_| Packages::new(fake.url(), store, root.join("token"), None));
        let (uninstall, check) = packages.update(cx, |packages, cx| {
            packages.mark(
                "o-r",
                Status::Available(Sha::parse(&"c".repeat(40)).unwrap()),
            );
            let uninstall = packages.uninstall("o-r".into(), cx);
            assert_eq!(packages.activity(), Some(&Activity::Removing("o-r".into())));
            (uninstall, packages.check(false, cx))
        });
        assert_eq!(check.await, Err(BUSY.into()));
        assert_eq!(uninstall.await, Ok(()));
        assert!(!root.join("packages/o-r").exists());
        packages.read_with(cx, |packages, _| {
            assert_eq!(packages.activity(), None);
            assert_eq!(packages.status("o-r"), None);
        });
        assert_eq!(
            packages
                .update(cx, |packages, cx| packages.check(false, cx))
                .await,
            Ok(Checked::default())
        );
        assert!(fake.requests().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    async fn launch_checks_only_log_failures_but_manual_checks_report_them(
        cx: &mut TestAppContext,
    ) {
        let root = scratch("state");
        let (_bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        let (staged, files) = stage_files(&store, &[("scripts/a.ts", "a")]);
        store
            .publish(staged, "o-r", &receipt(files.clone()))
            .unwrap();
        let (staged, _) = stage_files(&store, &[("scripts/a.ts", "a")]);
        let gone = super::store::Receipt {
            repository: 8,
            repo: "gone".into(),
            ..receipt(files)
        };
        store.publish(staged, "o-gone", &gone).unwrap();
        let fake = Fake::start();
        fake.route(
            "/repositories/7",
            Reply::json(r#"{"id":7,"name":"r","owner":{"login":"o"},"default_branch":"main"}"#),
        );
        let newer = "c".repeat(40);
        fake.route(
            "/repos/o/r/branches/main",
            Reply::json(format!(
                r#"{{"commit":{{"sha":"{newer}","commit":{{"tree":{{"sha":"{newer}"}}}}}}}}"#
            )),
        );
        let packages = cx.new(|_| Packages::new(fake.url(), store, root.join("token"), None));

        let quiet = packages
            .update(cx, |packages, cx| packages.check(true, cx))
            .await;
        assert_eq!(
            quiet,
            Ok(Checked {
                available: 1,
                failed: 0
            })
        );
        packages.read_with(cx, |packages, _| {
            assert_eq!(
                packages.status("o-r"),
                Some(&Status::Available(Sha::parse(&newer).unwrap()))
            );
            assert_eq!(packages.status("o-gone"), None);
            assert_eq!(packages.activity(), None);
        });

        let manual = packages
            .update(cx, |packages, cx| packages.check(false, cx))
            .await;
        assert_eq!(
            manual,
            Ok(Checked {
                available: 1,
                failed: 1
            })
        );
        packages.read_with(cx, |packages, _| {
            assert_eq!(
                packages.status("o-gone"),
                Some(&Status::Failed(
                    "Couldn't find o/gone. If it's private, add a GitHub token in Settings.".into()
                ))
            );
        });

        let updated = packages
            .update(cx, |packages, cx| packages.update("o-r".into(), false, cx))
            .await;
        let Err(message) = updated else {
            panic!("the update can't download o/r, so it fails");
        };
        packages.read_with(cx, |packages, _| {
            assert_eq!(packages.status("o-r"), Some(&Status::Failed(message)));
        });
        fs::remove_dir_all(root).unwrap();
    }
}
