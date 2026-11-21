use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{
    Kind, Manifest, PackagePath,
    archive::{self, Limits},
    github::{GitHub, GitHubError, Head, Locator},
    skua,
    store::{Edits, Receipt, Staged, Store, StoreError},
};

pub(crate) const LIMITS: Limits = Limits {
    download: 25_000_000,
    unpacked: 100_000_000,
    file: 10_000_000,
    files: 10_000,
};
const SKUA_LIMITS: Limits = Limits {
    file: 32_000_000,
    ..LIMITS
};
const MANIFEST_LIMIT: u64 = 256 * 1024;
const NO_SCRIPTS: &str =
    "This package has no scripts. Add a .ts, .mts, .js or .mjs file under scripts/.";
const NOT_SKUA: &str = "This isn't Skua's script repository. It has no CoreBots.cs.";
const NO_SKUA_LIST: &str =
    "This version of Skua's scripts has no readable scripts.json, so Mira can't list its scripts.";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Failure {
    #[error("This package is already installed.")]
    AlreadyInstalled(PathBuf),
    #[error("{0}")]
    Other(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) root: PathBuf,
    pub(crate) name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Updated {
    Current,
    Edited(Edits),
    Replaced,
}

pub(crate) async fn install(
    github: &GitHub,
    store: &Store,
    locator: &Locator,
) -> Result<Installed, Failure> {
    let label = format!("{}/{}", locator.owner, locator.repo);
    let repository = github
        .repository(&locator.owner, &locator.repo)
        .await
        .map_err(|error| explain(error, github, &label))?;
    if let Some(dir) = store
        .receipts()
        .into_iter()
        .find(|(_, receipt)| receipt.repository == repository.id && receipt.path == locator.path)
        .map(|(dir, _)| dir)
    {
        return Err(Failure::AlreadyInstalled(store.package(&dir)));
    }
    let source = Source {
        owner: repository.owner,
        repo: repository.name,
        branch: locator.branch.clone().unwrap_or(repository.default_branch),
        path: locator.path.clone(),
    };
    let head = head(github, &source).await?;
    let (staged, files, name) = fetch(github, store, &source, &head).await?;
    let dir = match store.kind() {
        Kind::Mira => match source.path.split_last() {
            Some((_, last)) => format!("{}-{}-{last}", source.owner, source.repo),
            None => format!("{}-{}", source.owner, source.repo),
        },
        Kind::Skua => skua::FOLDER.to_owned(),
    };
    let receipt = Receipt {
        repository: repository.id,
        owner: source.owner,
        repo: source.repo,
        path: source.path,
        branch: source.branch,
        commit: head.commit,
        tree: head.tree,
        files,
    };
    let root = store
        .publish(staged, &dir, &receipt)
        .map_err(|error| match error {
            StoreError::Exists(dir) => Failure::Other(format!(
                "A folder named {dir} is already in your packages folder."
            )),
            StoreError::Io(error) => Failure::Other(format!("Couldn't install it: {error}.")),
        })?;
    Ok(Installed { root, name })
}

pub(crate) async fn check(github: &GitHub, receipt: &Receipt) -> Result<Option<Head>, String> {
    Ok(latest(github, receipt).await?.map(|(_, head)| head))
}

async fn latest(github: &GitHub, receipt: &Receipt) -> Result<Option<(Source, Head)>, String> {
    let repository = github
        .repository_by_id(receipt.repository)
        .await
        .map_err(|error| {
            explain(
                error,
                github,
                &format!("{}/{}", receipt.owner, receipt.repo),
            )
        })?;
    let source = Source {
        owner: repository.owner,
        repo: repository.name,
        branch: receipt.branch.clone(),
        path: receipt.path.clone(),
    };
    let head = head(github, &source).await?;
    Ok((head.tree != receipt.tree).then_some((source, head)))
}

pub(crate) async fn update(
    github: &GitHub,
    store: &Store,
    dir: &str,
    overwrite_edits: bool,
) -> Result<Updated, String> {
    let package = store.package(dir);
    let receipt = Receipt::read(&package)?
        .ok_or("This package wasn't installed from GitHub, so Mira can't update it.")?;
    let Some((source, head)) = latest(github, &receipt).await? else {
        return Ok(Updated::Current);
    };
    let (staged, files, _) = fetch(github, store, &source, &head)
        .await
        .map_err(|failure| failure.to_string())?;
    if !overwrite_edits {
        let edits = Edits::find(&package, &receipt.files, store.kind())
            .map_err(|error| format!("Couldn't check the package for edits: {error}."))?;
        if !edits.is_empty() {
            return Ok(Updated::Edited(edits));
        }
    }
    let next = Receipt {
        owner: source.owner,
        repo: source.repo,
        commit: head.commit,
        tree: head.tree,
        files,
        ..receipt
    };
    store
        .replace(staged, dir, &next)
        .map_err(|error| format!("Couldn't update it: {error}."))?;
    Ok(Updated::Replaced)
}

struct Source {
    owner: String,
    repo: String,
    branch: String,
    path: PackagePath,
}

impl Source {
    fn label(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

async fn head(github: &GitHub, source: &Source) -> Result<Head, String> {
    let label = source.label();
    github
        .head(&source.owner, &source.repo, &source.branch, &source.path)
        .await
        .map_err(|error| match error {
            GitHubError::NoBranch => format!("{label} has no {} branch.", source.branch),
            GitHubError::NoFolder => {
                format!(
                    "{label} has no {} folder on {}.",
                    source.path, source.branch
                )
            }
            error => explain(error, github, &label),
        })
}

async fn fetch(
    github: &GitHub,
    store: &Store,
    source: &Source,
    head: &Head,
) -> Result<(Staged, archive::Files, String), Failure> {
    let label = source.label();
    let (manifest, limits) = match store.kind() {
        Kind::Mira => (Some(manifest(github, source, head).await?), LIMITS),
        Kind::Skua => (None, SKUA_LIMITS),
    };
    let staged = store
        .stage()
        .map_err(|error| Failure::Other(format!("Couldn't prepare the download: {error}.")))?;
    let download = staged.file("download.zip");
    github
        .download(
            &source.owner,
            &source.repo,
            &head.commit,
            &download,
            limits.download,
        )
        .await
        .map_err(|error| match error {
            GitHubError::TooLarge(_) => format!("{label} is too large to install: {error}."),
            error => explain(error, github, &label),
        })?;
    let files = archive::extract(&download, &source.path, &staged.unpacked(), limits)
        .map_err(|error| Failure::Other(format!("Couldn't install it: {error}.")))?;
    let unpacked = staged.unpacked();
    let name = match manifest {
        Some((manifest, parsed)) => {
            if fs::read(unpacked.join("package.json")).ok().as_deref() != Some(&manifest[..]) {
                return Err(Failure::Other(
                    "The download didn't match what GitHub reported. Try again.".into(),
                ));
            }
            if !has_scripts(&unpacked.join("scripts")) {
                return Err(Failure::Other(NO_SCRIPTS.into()));
            }
            parsed.name
        }
        None => {
            if !unpacked.join("CoreBots.cs").is_file() {
                return Err(Failure::Other(NOT_SKUA.into()));
            }
            if !skua::listed(&unpacked).is_ok_and(|scripts| !scripts.is_empty()) {
                return Err(Failure::Other(NO_SKUA_LIST.into()));
            }
            skua::NAME.to_owned()
        }
    };
    Ok((staged, files, name))
}

async fn manifest(
    github: &GitHub,
    source: &Source,
    head: &Head,
) -> Result<(Vec<u8>, Manifest), Failure> {
    let label = source.label();
    let manifest_path = source
        .path
        .join("package.json")
        .expect("package.json is a safe segment");
    let manifest = match github
        .file(
            &source.owner,
            &source.repo,
            &head.commit,
            &manifest_path,
            MANIFEST_LIMIT,
        )
        .await
    {
        Ok(manifest) => manifest,
        Err(GitHubError::NotFound) => {
            return Err(Failure::Other(
                "This isn't a Mira script package. It has no package.json.".into(),
            ));
        }
        Err(error) => return Err(Failure::Other(explain(error, github, &label))),
    };
    let parsed = Manifest::parse(&manifest).map_err(|error| Failure::Other(error.to_string()))?;
    Ok((manifest, parsed))
}

fn has_scripts(dir: &Path) -> bool {
    let Ok(items) = fs::read_dir(dir) else {
        return false;
    };
    items.flatten().any(|item| {
        let name = item.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            return false;
        }
        match item.file_type() {
            Ok(kind) if kind.is_dir() => has_scripts(&item.path()),
            Ok(_) => crate::scripting::runnable(&name),
            Err(_) => false,
        }
    })
}

fn explain(error: GitHubError, github: &GitHub, label: &str) -> String {
    match error {
        GitHubError::NotFound | GitHubError::NoBranch | GitHubError::NoFolder => {
            if github.has_token() {
                format!(
                    "Couldn't find {label}. Check the link, and that your GitHub token can see it."
                )
            } else {
                format!("Couldn't find {label}. If it's private, add a GitHub token in Settings.")
            }
        }
        GitHubError::Unauthorized => {
            "Your GitHub token expired or was revoked. Update it in Settings.".into()
        }
        GitHubError::RateLimited { retry_at } => {
            let wait = retry_at
                .duration_since(SystemTime::now())
                .unwrap_or_default();
            let minutes = wait.as_secs().div_ceil(60).max(1);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("GitHub's rate limit was reached. Try again in {minutes} {unit}.")
        }
        GitHubError::Network(message) => {
            let mut message = message;
            if let Some(first) = message.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            format!("{message}.")
        }
        error => format!("Couldn't install {label}: {error}."),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::packages::{
        RECEIPT,
        archive::tests::{Item, scratch, write_zip},
        fake::{Fake, Reply},
        store::tests::Bin,
    };

    const FIRST: (&str, &str) = (
        "1111111111111111111111111111111111111111",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    const SECOND: (&str, &str) = (
        "2222222222222222222222222222222222222222",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    const MANIFEST: &str = r#"{"name":"Dailies","mira":{}}"#;

    struct World {
        root: PathBuf,
        fake: Fake,
        bin: Bin,
        store: Store,
    }

    impl World {
        fn new() -> Self {
            Self::of(Kind::Mira)
        }

        fn of(kind: Kind) -> Self {
            let root = scratch("service");
            let (bin, trash) = Bin::new(&root);
            let store = Store::new(root.join("packages"), trash, kind);
            let fake = Fake::start();
            let repository =
                r#"{"id":42,"name":"r","owner":{"login":"o"},"default_branch":"main"}"#;
            fake.route("/repos/o/r", Reply::json(repository));
            fake.route("/repositories/42", Reply::json(repository));
            Self {
                root,
                fake,
                bin,
                store,
            }
        }

        fn github(&self, token: Option<&str>) -> GitHub {
            GitHub::at(&self.fake.url(), token.map(Into::into)).unwrap()
        }

        fn serve(&self, release: (&str, &str), manifest: Option<&str>, files: &[(&str, &str)]) {
            self.serve_at("r", release, manifest, files);
        }

        fn serve_at(
            &self,
            repo: &str,
            (commit, tree): (&str, &str),
            manifest: Option<&str>,
            files: &[(&str, &str)],
        ) {
            self.fake.route(
                &format!("/repos/o/{repo}/branches/main"),
                Reply::json(format!(
                    r#"{{"commit":{{"sha":"{commit}","commit":{{"tree":{{"sha":"{commit}"}}}}}}}}"#
                )),
            );
            self.fake.route(
                &format!("/repos/o/{repo}/contents/pkgs?ref={commit}"),
                Reply::json(format!(
                    r#"[{{"name":"dailies","type":"dir","sha":"{tree}"}}]"#
                )),
            );
            if let Some(manifest) = manifest {
                self.fake.route(
                    &format!("/repos/o/{repo}/contents/pkgs/dailies/package.json?ref={commit}"),
                    Reply::bytes(manifest.as_bytes().to_vec()),
                );
            }
            let top = format!("o-{repo}-{}", &commit[..7]);
            let names: Vec<String> = files
                .iter()
                .map(|(path, _)| format!("{top}/pkgs/dailies/{path}"))
                .collect();
            let mut items = Vec::new();
            let readme = format!("{top}/README.md");
            items.push(Item::File(&readme, b"repo"));
            for ((_, text), name) in files.iter().zip(&names) {
                items.push(Item::File(name, text.as_bytes()));
            }
            let zip = self.root.join(format!("{commit}.zip"));
            write_zip(&zip, &items);
            self.fake.route(
                &format!("/repos/o/{repo}/zipball/{commit}"),
                Reply::bytes(fs::read(&zip).unwrap()),
            );
        }

        fn serve_root(&self, (commit, _): (&str, &str), files: &[(&str, &str)]) {
            self.fake.route(
                "/repos/o/r/branches/main",
                Reply::json(format!(
                    r#"{{"commit":{{"sha":"{commit}","commit":{{"tree":{{"sha":"{commit}"}}}}}}}}"#
                )),
            );
            let top = format!("o-r-{}", &commit[..7]);
            let names: Vec<String> = files
                .iter()
                .map(|(path, _)| format!("{top}/{path}"))
                .collect();
            let items: Vec<Item> = files
                .iter()
                .zip(&names)
                .map(|((_, text), name)| Item::File(name, text.as_bytes()))
                .collect();
            let zip = self.root.join(format!("{commit}.zip"));
            write_zip(&zip, &items);
            self.fake.route(
                &format!("/repos/o/r/zipball/{commit}"),
                Reply::bytes(fs::read(&zip).unwrap()),
            );
        }

        fn packages(&self) -> Vec<String> {
            let Ok(items) = fs::read_dir(self.store.root()) else {
                return Vec::new();
            };
            let mut names: Vec<String> = items
                .flatten()
                .map(|item| item.file_name().to_string_lossy().into_owned())
                .filter(|name| name != ".staging")
                .collect();
            names.sort();
            names
        }

        fn downloads(&self) -> usize {
            self.fake
                .requests()
                .iter()
                .filter(|request| request.contains("/zipball/"))
                .count()
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    type Case = (
        Option<&'static str>,
        &'static [(&'static str, &'static str)],
        &'static str,
    );

    fn dailies() -> Locator {
        Locator::parse("github.com/o/r/tree/main/pkgs/dailies", "").unwrap()
    }

    #[tokio::test]
    async fn a_package_folder_installs_once_with_its_receipt() {
        let world = World::new();
        world.serve(
            FIRST,
            Some(MANIFEST),
            &[("package.json", MANIFEST), ("scripts/spin.ts", "spin")],
        );
        let github = world.github(None);
        assert_eq!(
            install(&github, &world.store, &dailies()).await,
            Ok(Installed {
                root: world.store.package("o-r-dailies"),
                name: "Dailies".into()
            })
        );
        let installed = world.store.package("o-r-dailies");
        assert_eq!(
            fs::read_to_string(installed.join("scripts/spin.ts")).unwrap(),
            "spin"
        );
        assert!(!installed.join("README.md").exists());
        let receipt = Receipt::read(&installed).unwrap().unwrap();
        assert_eq!(
            (
                receipt.repository,
                receipt.branch.as_str(),
                receipt.path.as_str()
            ),
            (42, "main", "pkgs/dailies")
        );
        assert_eq!((receipt.commit.as_str(), receipt.tree.as_str()), FIRST);
        assert_eq!(receipt.files.len(), 2);

        assert_eq!(
            install(&github, &world.store, &dailies()).await,
            Err(Failure::AlreadyInstalled(
                world.store.package("o-r-dailies")
            ))
        );
        assert_eq!(world.packages(), ["o-r-dailies"]);
        assert_eq!(world.downloads(), 1);
    }

    #[tokio::test]
    async fn repositories_that_arent_usable_packages_are_refused_before_anything_is_kept() {
        let cases: [Case; 5] = [
            (
                Some(r#"{"name":"left-pad"}"#),
                &[],
                "This isn't a Mira script package. Its package.json needs a \"mira\" field.",
            ),
            (
                None,
                &[],
                "This isn't a Mira script package. It has no package.json.",
            ),
            (
                Some(r#"{"name":"x","mira":{"requires":"999.0.0"}}"#),
                &[],
                "Needs Mira 999.0.0 or newer.",
            ),
            (
                Some(MANIFEST),
                &[("package.json", MANIFEST), ("lib/a.ts", "a")],
                NO_SCRIPTS,
            ),
            (
                Some(MANIFEST),
                &[
                    ("package.json", "{\"changed\":true}"),
                    ("scripts/a.ts", "a"),
                ],
                "The download didn't match what GitHub reported. Try again.",
            ),
        ];
        for (manifest, files, message) in cases {
            let world = World::new();
            world.serve(FIRST, manifest, files);
            let failure = install(&world.github(None), &world.store, &dailies()).await;
            assert_eq!(failure, Err(Failure::Other(message.into())));
            assert!(world.packages().is_empty(), "{message}");
            assert_eq!(
                world.downloads(),
                usize::from(!files.is_empty()),
                "{message}"
            );
            assert!(
                fs::read_dir(world.store.root().join(".staging"))
                    .map(|mut items| items.next().is_none())
                    .unwrap_or(true)
            );
        }

        let world = World::new();
        let missing = Locator::parse("github.com/o/private", "").unwrap();
        assert_eq!(
            install(&world.github(None), &world.store, &missing).await,
            Err(Failure::Other(
                "Couldn't find o/private. If it's private, add a GitHub token in Settings.".into()
            ))
        );
        assert_eq!(
            install(&world.github(Some("t")), &world.store, &missing).await,
            Err(Failure::Other(
                "Couldn't find o/private. Check the link, and that your GitHub token can see it."
                    .into()
            ))
        );
        let branch = Locator::parse("github.com/o/r", "gone").unwrap();
        assert_eq!(
            install(&world.github(None), &world.store, &branch).await,
            Err(Failure::Other("o/r has no gone branch.".into()))
        );
    }

    #[tokio::test]
    async fn updates_follow_the_installed_repository_not_its_old_name() {
        let world = World::new();
        world.serve(
            FIRST,
            Some(MANIFEST),
            &[("package.json", MANIFEST), ("scripts/spin.ts", "spin")],
        );
        let github = world.github(None);
        install(&github, &world.store, &dailies()).await.unwrap();
        let dir = "o-r-dailies";

        world.fake.route(
            "/repositories/42",
            Reply::json(
                r#"{"id":42,"name":"renamed","owner":{"login":"o"},"default_branch":"main"}"#,
            ),
        );
        world.serve_at(
            "renamed",
            SECOND,
            Some(MANIFEST),
            &[("package.json", MANIFEST), ("scripts/spin.ts", "spin v2")],
        );
        world.serve(
            (
                "3333333333333333333333333333333333333333",
                "cccccccccccccccccccccccccccccccccccccccc",
            ),
            Some(MANIFEST),
            &[
                ("package.json", MANIFEST),
                ("scripts/spin.ts", "someone else's script"),
            ],
        );
        assert_eq!(
            update(&github, &world.store, dir, false).await,
            Ok(Updated::Replaced)
        );
        let installed = world.store.package(dir);
        assert_eq!(
            fs::read_to_string(installed.join("scripts/spin.ts")).unwrap(),
            "spin v2"
        );
        let receipt = Receipt::read(&installed).unwrap().unwrap();
        assert_eq!((receipt.repository, receipt.repo.as_str()), (42, "renamed"));
    }

    #[tokio::test]
    async fn an_update_waits_for_new_files_and_asks_before_replacing_edits() {
        let world = World::new();
        world.serve(
            FIRST,
            Some(MANIFEST),
            &[("package.json", MANIFEST), ("scripts/spin.ts", "spin")],
        );
        let github = world.github(None);
        install(&github, &world.store, &dailies()).await.unwrap();
        let dir = "o-r-dailies";
        let installed = world.store.package(dir);
        let receipt = || Receipt::read(&installed).unwrap().unwrap();
        assert_eq!(check(&github, &receipt()).await, Ok(None));
        assert_eq!(
            update(&github, &world.store, dir, false).await,
            Ok(Updated::Current)
        );

        world.serve(
            SECOND,
            Some(MANIFEST),
            &[("package.json", MANIFEST), ("scripts/spin.ts", "spin v2")],
        );
        let available = check(&github, &receipt()).await.unwrap().unwrap();
        assert_eq!(available.tree.as_str(), SECOND.1);

        fs::write(installed.join("scripts/spin.ts"), "my tweak").unwrap();
        let Ok(Updated::Edited(edits)) = update(&github, &world.store, dir, false).await else {
            panic!("an edited package should ask first");
        };
        assert_eq!(
            edits.changed,
            [PackagePath::parse("scripts/spin.ts").unwrap()]
        );
        assert_eq!(
            fs::read_to_string(installed.join("scripts/spin.ts")).unwrap(),
            "my tweak"
        );

        assert_eq!(
            update(&github, &world.store, dir, true).await,
            Ok(Updated::Replaced)
        );
        assert_eq!(
            fs::read_to_string(installed.join("scripts/spin.ts")).unwrap(),
            "spin v2"
        );
        assert_eq!((receipt().commit.as_str(), receipt().tree.as_str()), SECOND);
        assert!(world.bin.contents().contains(&b"my tweak".to_vec()));
        assert_eq!(check(&github, &receipt()).await, Ok(None));
        assert!(installed.join(RECEIPT).exists());

        world
            .fake
            .route("/repos/o/r/branches/main", Reply::status(404));
        assert_eq!(
            check(&github, &receipt()).await,
            Err("o/r has no main branch.".into())
        );
    }

    #[tokio::test]
    async fn skua_scripts_install_without_a_manifest_and_tolerate_the_quest_cache() {
        let world = World::of(Kind::Skua);
        let root = Locator::parse("github.com/o/r", "").unwrap();
        world.serve_root(FIRST, &[("Story/Intro.cs", "class Intro {}")]);
        let github = world.github(None);
        assert_eq!(
            install(&github, &world.store, &root).await,
            Err(Failure::Other(NOT_SKUA.into()))
        );
        assert!(world.packages().is_empty());

        world.serve_root(
            FIRST,
            &[
                ("CoreBots.cs", "class CoreBots {}"),
                ("Story/Intro.cs", "class Intro {}"),
            ],
        );
        assert_eq!(
            install(&github, &world.store, &root).await,
            Err(Failure::Other(NO_SKUA_LIST.into()))
        );
        assert!(world.packages().is_empty());

        world.serve_root(
            FIRST,
            &[
                ("CoreBots.cs", "class CoreBots {}"),
                ("QuestData.json", "[]"),
                (
                    "scripts.json",
                    r#"[{"name":"Intro","path":"Story/Intro.cs"}]"#,
                ),
                ("Story/Intro.cs", "class Intro {}"),
            ],
        );
        let Installed {
            root: installed,
            name,
        } = install(&github, &world.store, &root).await.unwrap();
        assert_eq!(installed, world.store.package("Scripts"));
        assert_eq!(name, "Skua scripts");
        assert_eq!(
            fs::read_to_string(installed.join("Story/Intro.cs")).unwrap(),
            "class Intro {}"
        );

        world.serve_root(
            SECOND,
            &[
                ("CoreBots.cs", "class CoreBots { }"),
                ("README.md", "Skua scripts"),
                ("scripts.json", r#"[{"name":"Read me","path":"README.md"}]"#),
                ("Story/Intro.cs", "class Intro {}"),
            ],
        );
        assert_eq!(
            update(&github, &world.store, "Scripts", false).await,
            Err(NO_SKUA_LIST.into())
        );
        assert_eq!(
            fs::read_to_string(installed.join("CoreBots.cs")).unwrap(),
            "class CoreBots {}"
        );

        world.serve_root(
            SECOND,
            &[
                ("CoreBots.cs", "class CoreBots { }"),
                ("QuestData.json", "[]"),
                (
                    "scripts.json",
                    r#"[{"name":"Intro","path":"Story/Intro.cs"}]"#,
                ),
                ("Story/Intro.cs", "class Intro {}"),
            ],
        );
        fs::write(installed.join("QuestData.json"), "[{\"id\":1}]").unwrap();
        assert_eq!(
            update(&github, &world.store, "Scripts", false).await,
            Ok(Updated::Replaced)
        );
        assert_eq!(
            fs::read_to_string(installed.join("CoreBots.cs")).unwrap(),
            "class CoreBots { }"
        );
    }
}
