use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{App, AppContext as _, Context, Entity, Global, Task};

use super::{
    Kind, Sha,
    bundled::{self, Recommended},
    github::{API, GitHub, Locator},
    service::{self, Failure, Installed, Updated},
    skua,
    store::{self, Store},
    token,
};

const BUSY: &str = "Wait for the current package action to finish.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Installing,
    Checking,
    Updating(PathBuf),
    Removing(PathBuf),
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
    mira: Store,
    skua: Store,
    token_path: PathBuf,
    token: Option<String>,
    activity: Option<Activity>,
    status: HashMap<PathBuf, Status>,
}

struct Shared(Entity<Packages>);

impl Global for Shared {}

pub(crate) fn init(root: PathBuf, skua: PathBuf, token_path: PathBuf, cx: &mut App) {
    let token = token::load(&token_path).unwrap_or_else(|error| {
        tracing::warn!(%error, path = %token_path.display(), "couldn't read the GitHub token");
        None
    });
    let trash: store::Trash = Arc::new(store::move_to_trash);
    let mira = Store::new(root, trash.clone(), Kind::Mira);
    let skua = Store::new(skua, trash, Kind::Skua);
    let packages = cx.new(|_| Packages::new(API.into(), mira, skua, token_path, token));
    cx.set_global(Shared(packages));
}

#[cfg(test)]
pub(crate) fn init_against(api: String, root: PathBuf, skua: PathBuf, cx: &mut App) {
    let trash: store::Trash = Arc::new(store::move_to_trash);
    let mira = Store::new(root, trash.clone(), Kind::Mira);
    let skua = Store::new(skua, trash, Kind::Skua);
    let packages = cx.new(|_| Packages::new(api, mira, skua, PathBuf::new(), None));
    cx.set_global(Shared(packages));
}

pub(crate) fn start(cx: &mut App) {
    let resources = bundled::resources();
    packages(cx).update(cx, |packages, cx| {
        packages.resources = resources.clone();
        packages.activity = Some(Activity::Checking);
        cx.notify();
        let stores = [packages.mira.clone(), packages.skua.clone()];
        let prepared = cx.background_spawn(async move {
            if let Some(resources) = &resources
                && let Err(error) = bundled::seed(resources, &stores[0])
            {
                tracing::warn!(%error, "couldn't add the bundled packages");
            }
            stores.iter().try_for_each(Store::sweep)
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
    fn new(
        api: String,
        mira: Store,
        skua: Store,
        token_path: PathBuf,
        token: Option<String>,
    ) -> Self {
        Self {
            api,
            resources: None,
            mira,
            skua,
            token_path,
            token,
            activity: None,
            status: HashMap::new(),
        }
    }

    fn store(&self, kind: Kind) -> &Store {
        match kind {
            Kind::Mira => &self.mira,
            Kind::Skua => &self.skua,
        }
    }

    fn locate(&self, root: &Path) -> Option<(Store, String)> {
        let dir = root.file_name()?.to_str()?;
        [&self.mira, &self.skua]
            .into_iter()
            .find(|store| root.parent() == Some(store.root()))
            .map(|store| (store.clone(), dir.to_owned()))
    }

    pub(crate) fn skua_scripts(&self) -> PathBuf {
        self.skua.package(skua::FOLDER)
    }

    pub(crate) fn activity(&self) -> Option<&Activity> {
        self.activity.as_ref()
    }

    pub(crate) fn status(&self, root: &Path) -> Option<&Status> {
        self.status.get(root)
    }

    #[cfg(test)]
    pub(crate) fn mark(&mut self, root: PathBuf, status: Status) {
        self.status.insert(root, status);
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
        let mut recommended = self
            .resources
            .as_ref()
            .map(|resources| bundled::recommended(resources, &self.mira))
            .unwrap_or_default();
        if self.skua.receipts().is_empty() {
            recommended.push(skua::recommended());
        }
        recommended
    }

    pub(crate) fn install_link(
        &mut self,
        link: &str,
        branch: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Installed, Failure>> {
        match Locator::parse(link, branch) {
            Ok(locator) => self.install(locator, cx),
            Err(message) => Task::ready(Err(Failure::Other(message))),
        }
    }

    pub(crate) fn install(
        &mut self,
        locator: Locator,
        cx: &mut Context<Self>,
    ) -> Task<Result<Installed, Failure>> {
        let store = self.store(Kind::of(&locator)).clone();
        self.start(Activity::Installing, cx, async move |github| {
            service::install(&github, &store, &locator).await
        })
    }

    pub(crate) fn check(
        &mut self,
        quiet: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Checked, String>> {
        let stores = [self.mira.clone(), self.skua.clone()];
        let work = self.start(Activity::Checking, cx, async move |github| {
            let mut results = Vec::new();
            for store in &stores {
                for (dir, receipt) in store.receipts() {
                    results.push((store.package(&dir), service::check(&github, &receipt).await));
                }
            }
            Ok::<_, String>(results)
        });
        cx.spawn(async move |this, cx| {
            let results = work.await?;
            this.update(cx, |this, cx| {
                let mut checked = Checked::default();
                for (root, result) in results {
                    match result {
                        Ok(Some(head)) => {
                            checked.available += 1;
                            this.status.insert(root, Status::Available(head.commit));
                        }
                        Ok(None) => {
                            this.status.remove(&root);
                        }
                        Err(message) if quiet => {
                            tracing::info!(root = %root.display(), %message, "couldn't check a package for updates");
                        }
                        Err(message) => {
                            checked.failed += 1;
                            this.status.insert(root, Status::Failed(message));
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
        root: PathBuf,
        overwrite_edits: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Updated, String>> {
        let Some((store, dir)) = self.locate(&root) else {
            return Task::ready(Err(NOT_INSTALLED.into()));
        };
        let work = self.start(Activity::Updating(root.clone()), cx, async move |github| {
            service::update(&github, &store, &dir, overwrite_edits).await
        });
        let settled = self.settle(
            root.clone(),
            work,
            |updated| !matches!(updated, Updated::Edited(_)),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = settled.await;
            if let Err(message) = &result {
                let _ = this.update(cx, |this, cx| {
                    this.status.insert(root, Status::Failed(message.clone()));
                    cx.notify();
                });
            }
            result
        })
    }

    pub(crate) fn uninstall(
        &mut self,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some((store, dir)) = self.locate(&root) else {
            return Task::ready(Err(NOT_INSTALLED.into()));
        };
        let work = self.start(Activity::Removing(root.clone()), cx, async move |_| {
            store
                .uninstall(&dir)
                .map_err(|error| format!("Couldn't move it to the Trash: {error}."))
        });
        self.settle(root, work, |_| true, cx)
    }

    fn settle<T: 'static>(
        &mut self,
        root: PathBuf,
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
                    this.status.remove(&root);
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
        F: FnOnce(GitHub) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>>,
    {
        if self.activity.is_some() {
            return Task::ready(Err(E::from(BUSY.to_owned())));
        }
        self.activity = Some(activity);
        cx.notify();
        let token = self.token.clone();
        let api = self.api.clone();
        let work = cx.background_spawn(async move {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| E::from(format!("Couldn't start the download: {error}.")))?;
            let github = GitHub::at(&api, token).map_err(|error| E::from(format!("{error}.")))?;
            runtime.block_on(work(github))
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

const NOT_INSTALLED: &str = "This package isn't installed anymore.";

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
        let store = Store::new(root.join("packages"), trash.clone(), Kind::Mira);
        let skua = Store::new(root.join("skua"), trash, Kind::Skua);
        let fake = Fake::start();
        let packages =
            cx.new(|_| Packages::new(fake.url(), store.clone(), skua, root.join("token"), None));
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
        let store = Store::new(root.join("packages"), trash.clone(), Kind::Mira);
        let skua = Store::new(root.join("skua"), trash, Kind::Skua);
        let (staged, files) = stage_files(&store, &[("package.json", "{}"), ("scripts/a.ts", "a")]);
        store.publish(staged, "o-r", &receipt(files)).unwrap();
        let installed = store.package("o-r");
        let fake = Fake::start();
        let packages = cx.new(|_| Packages::new(fake.url(), store, skua, root.join("token"), None));
        let (uninstall, check) = packages.update(cx, |packages, cx| {
            packages.mark(
                installed.clone(),
                Status::Available(Sha::parse(&"c".repeat(40)).unwrap()),
            );
            let uninstall = packages.uninstall(installed.clone(), cx);
            assert_eq!(
                packages.activity(),
                Some(&Activity::Removing(installed.clone()))
            );
            (uninstall, packages.check(false, cx))
        });
        assert_eq!(check.await, Err(BUSY.into()));
        assert_eq!(uninstall.await, Ok(()));
        assert!(!installed.exists());
        packages.read_with(cx, |packages, _| {
            assert_eq!(packages.activity(), None);
            assert_eq!(packages.status(&installed), None);
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
        let store = Store::new(root.join("packages"), trash.clone(), Kind::Mira);
        let skua = Store::new(root.join("skua"), trash, Kind::Skua);
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
        let packages =
            cx.new(|_| Packages::new(fake.url(), store.clone(), skua, root.join("token"), None));

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
                packages.status(&store.package("o-r")),
                Some(&Status::Available(Sha::parse(&newer).unwrap()))
            );
            assert_eq!(packages.status(&store.package("o-gone")), None);
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
                packages.status(&store.package("o-gone")),
                Some(&Status::Failed(
                    "Couldn't find o/gone. If it's private, add a GitHub token in Settings.".into()
                ))
            );
        });

        let updated = packages
            .update(cx, |packages, cx| {
                packages.update(store.package("o-r"), false, cx)
            })
            .await;
        let Err(message) = updated else {
            panic!("the update can't download o/r, so it fails");
        };
        packages.read_with(cx, |packages, _| {
            assert_eq!(
                packages.status(&store.package("o-r")),
                Some(&Status::Failed(message))
            );
        });
        fs::remove_dir_all(root).unwrap();
    }
}
