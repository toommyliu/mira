use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{
    Manifest, PackagePath, Sha,
    archive::{self, Files},
    github::Locator,
    store::{Receipt, Store},
};

const REPOSITORY: u64 = 1_448_817_579;
const OWNER: &str = "toommyliu";
const REPO: &str = "mira";
const BRANCH: &str = "main";
const FOLDER: &str = "script-packages";
const INDEX: &str = "script-packages.json";

#[derive(Debug, Deserialize)]
struct Index {
    commit: Sha,
    trees: BTreeMap<String, Sha>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recommended {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) locator: Locator,
}

pub(crate) fn resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.join("Resources"))
}

fn index(resources: &Path) -> Option<Index> {
    let bytes = fs::read(resources.join(INDEX)).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|error| tracing::warn!(%error, "the bundled package index is damaged"))
        .ok()
}

fn path(name: &str) -> Option<PackagePath> {
    PackagePath::parse(FOLDER)?.join(name)
}

pub(crate) fn seed(resources: &Path, store: &Store) -> io::Result<()> {
    if store.root().exists() {
        return Ok(());
    }
    let Some(index) = index(resources) else {
        return Ok(());
    };
    fs::create_dir_all(store.root())?;
    for (name, tree) in index.trees {
        let Some(path) = path(&name) else {
            continue;
        };
        let staged = store.stage()?;
        let mut files = Files::new();
        copy(
            &resources.join(FOLDER).join(&name),
            &staged.unpacked(),
            &PackagePath::default(),
            &mut files,
        )?;
        let receipt = Receipt {
            repository: REPOSITORY,
            owner: OWNER.into(),
            repo: REPO.into(),
            path,
            branch: BRANCH.into(),
            commit: index.commit.clone(),
            tree,
            files,
        };
        if let Err(error) = store.publish(staged, &format!("{OWNER}-{REPO}-{name}"), &receipt) {
            tracing::warn!(%error, %name, "couldn't add a bundled package");
        }
    }
    Ok(())
}

fn copy(from: &Path, to: &Path, at: &PackagePath, files: &mut Files) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for item in fs::read_dir(from)? {
        let item = item?;
        let name = item.file_name();
        let Some(path) = name.to_str().and_then(|name| at.join(name)) else {
            continue;
        };
        let target = to.join(&name);
        if item.file_type()?.is_dir() {
            copy(&item.path(), &target, &path, files)?;
        } else {
            fs::copy(item.path(), &target)?;
            files.insert(path, archive::hash(&target)?);
        }
    }
    Ok(())
}

pub(crate) fn recommended(resources: &Path, store: &Store) -> Vec<Recommended> {
    let Some(index) = index(resources) else {
        return Vec::new();
    };
    let installed: Vec<PackagePath> = store
        .receipts()
        .into_iter()
        .filter(|(_, receipt)| receipt.repository == REPOSITORY)
        .map(|(_, receipt)| receipt.path)
        .collect();
    index
        .trees
        .keys()
        .filter_map(|name| {
            let path = path(name)?;
            if installed.contains(&path) {
                return None;
            }
            let bytes = fs::read(resources.join(FOLDER).join(name).join("package.json")).ok()?;
            let manifest = Manifest::parse(&bytes).ok()?;
            Some(Recommended {
                name: manifest.name,
                description: manifest.description,
                locator: Locator {
                    owner: OWNER.into(),
                    repo: REPO.into(),
                    branch: Some(BRANCH.into()),
                    path,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::{archive::tests::scratch, store::tests::Bin};

    fn bundle(root: &Path) -> PathBuf {
        let resources = root.join("Resources");
        for (file, text) in [
            (
                "dailies/package.json",
                r#"{"name":"Dailies","description":"Daily runs.","mira":{}}"#,
            ),
            ("dailies/scripts/spin.ts", "spin"),
            ("weekly/package.json", r#"{"name":"Weekly","mira":{}}"#),
            ("weekly/scripts/boss.ts", "boss"),
        ] {
            let path = resources.join(FOLDER).join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let commit = "1".repeat(40);
        let (dailies, weekly) = ("2".repeat(40), "3".repeat(40));
        fs::write(
            resources.join(INDEX),
            format!(
                r#"{{"commit":"{commit}","trees":{{"dailies":"{dailies}","weekly":"{weekly}"}}}}"#
            ),
        )
        .unwrap();
        resources
    }

    #[test]
    fn bundled_packages_are_added_on_first_run_only() {
        let root = scratch("bundled");
        let resources = bundle(&root);
        let (_bin, trash) = Bin::new(&root);
        let store = Store::new(root.join("packages"), trash);
        assert_eq!(recommended(&resources, &store).len(), 2);

        seed(&resources, &store).unwrap();
        let installed = store.package("toommyliu-mira-dailies");
        assert_eq!(
            fs::read_to_string(installed.join("scripts/spin.ts")).unwrap(),
            "spin"
        );
        let receipt = Receipt::read(&installed).unwrap().unwrap();
        assert_eq!(receipt.path.as_str(), "script-packages/dailies");
        assert_eq!(receipt.tree.as_str(), "2".repeat(40));
        assert_eq!(receipt.files.len(), 2);
        assert!(
            super::super::store::Edits::find(&installed, &receipt.files)
                .unwrap()
                .is_empty()
        );
        assert!(recommended(&resources, &store).is_empty());

        store.uninstall("toommyliu-mira-weekly").unwrap();
        seed(&resources, &store).unwrap();
        assert!(!store.package("toommyliu-mira-weekly").exists());
        let left = recommended(&resources, &store);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "Weekly");
        assert_eq!(left[0].locator.path.as_str(), "script-packages/weekly");

        let empty = scratch("bundled");
        seed(&empty, &store).unwrap();
        assert!(recommended(&empty, &store).is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(empty).unwrap();
    }
}
