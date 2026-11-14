use std::{fs, path::Path, time::SystemTime};

use super::{
    Manifest, PackagePath,
    archive::{self, Limits},
    github::{GitHub, GitHubError, Head, Locator},
    store::{Edits, Receipt, Staged, Store, StoreError},
};

pub(crate) const LIMITS: Limits = Limits {
    download: 25_000_000,
    unpacked: 100_000_000,
    file: 10_000_000,
    files: 10_000,
};
const MANIFEST_LIMIT: u64 = 256 * 1024;
const NO_SCRIPTS: &str =
    "This package has no scripts. Add a .ts, .mts, .js or .mjs file under scripts/.";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Failure {
    #[error("This package is already installed.")]
    AlreadyInstalled(String),
    #[error("{0}")]
    Other(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
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
) -> Result<String, Failure> {
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
        return Err(Failure::AlreadyInstalled(dir));
    }
    let source = Source {
        owner: repository.owner,
        repo: repository.name,
        branch: locator.branch.clone().unwrap_or(repository.default_branch),
        path: locator.path.clone(),
    };
    let head = head(github, &source).await?;
    let (staged, files) = fetch(github, store, &source, &head).await?;
    let mut dir = format!("{}-{}", source.owner, source.repo);
    if let Some((_, last)) = source.path.split_last() {
        dir = format!("{dir}-{last}");
    }
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
    store
        .publish(staged, &dir, &receipt)
        .map_err(|error| match error {
            StoreError::Exists(dir) => Failure::Other(format!(
                "A folder named {dir} is already in your packages folder."
            )),
            StoreError::Io(error) => Failure::Other(format!("Couldn't install it: {error}.")),
        })?;
    Ok(dir)
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
    let (staged, files) = fetch(github, store, &source, &head)
        .await
        .map_err(|failure| failure.to_string())?;
    if !overwrite_edits {
        let edits = Edits::find(&package, &receipt.files)
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
) -> Result<(Staged, archive::Files), Failure> {
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
    Manifest::parse(&manifest).map_err(|error| Failure::Other(error.to_string()))?;
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
            LIMITS.download,
        )
        .await
        .map_err(|error| match error {
            GitHubError::TooLarge(_) => format!("{label} is too large to install: {error}."),
            error => explain(error, github, &label),
        })?;
    let files = archive::extract(&download, &source.path, &staged.unpacked(), LIMITS)
        .map_err(|error| Failure::Other(format!("Couldn't install it: {error}.")))?;
    let unpacked = staged.unpacked();
    if fs::read(unpacked.join("package.json")).ok().as_deref() != Some(&manifest[..]) {
        return Err(Failure::Other(
            "The download didn't match what GitHub reported. Try again.".into(),
        ));
    }
    if !has_scripts(&unpacked.join("scripts")) {
        return Err(Failure::Other(NO_SCRIPTS.into()));
    }
    Ok((staged, files))
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
            let root = scratch("service");
            let (bin, trash) = Bin::new(&root);
            let store = Store::new(root.join("packages"), trash);
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
            Ok("o-r-dailies".into())
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
            Err(Failure::AlreadyInstalled("o-r-dailies".into()))
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
        let dir = install(&github, &world.store, &dailies()).await.unwrap();

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
            update(&github, &world.store, &dir, false).await,
            Ok(Updated::Replaced)
        );
        let installed = world.store.package(&dir);
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
        let dir = install(&github, &world.store, &dailies()).await.unwrap();
        let installed = world.store.package(&dir);
        let receipt = || Receipt::read(&installed).unwrap().unwrap();
        assert_eq!(check(&github, &receipt()).await, Ok(None));
        assert_eq!(
            update(&github, &world.store, &dir, false).await,
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
        let Ok(Updated::Edited(edits)) = update(&github, &world.store, &dir, false).await else {
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
            update(&github, &world.store, &dir, true).await,
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
}
