use std::{fs, io, path::Path};

use super::{PackagePath, bundled::Recommended, github::Locator};

pub(crate) const FOLDER: &str = "Scripts";
const OWNER: &str = "auqw";
const REPO: &str = "Scripts";
const BRANCH: &str = "Skua";

pub(crate) fn listed(root: &Path) -> io::Result<Vec<PackagePath>> {
    #[derive(serde::Deserialize)]
    struct Listed {
        name: Option<String>,
        path: String,
    }
    let listed: Vec<Listed> =
        serde_json::from_slice(&fs::read(root.join("scripts.json"))?).map_err(io::Error::other)?;
    Ok(listed
        .into_iter()
        .filter(|listed| listed.name.as_deref().is_some_and(|name| name != "null"))
        .filter_map(|listed| PackagePath::parse(&listed.path))
        .filter(|path| {
            let file = root.join(path.as_str());
            crate::scripting::is_skua(&file) && file.is_file()
        })
        .collect())
}

pub(super) fn is(locator: &Locator) -> bool {
    locator.owner.eq_ignore_ascii_case(OWNER)
        && locator.repo.eq_ignore_ascii_case(REPO)
        && locator.path.is_root()
        && locator
            .branch
            .as_deref()
            .is_none_or(|branch| branch == BRANCH)
}

pub(super) fn recommended() -> Recommended {
    Recommended {
        name: "Skua scripts".into(),
        description: Some("Experimental. Skua's library of C# scripts. Needs .NET 10.".into()),
        locator: Locator {
            owner: OWNER.into(),
            repo: REPO.into(),
            branch: Some(BRANCH.into()),
            path: PackagePath::default(),
        },
    }
}
