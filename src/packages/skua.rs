use std::{fs, io, path::Path};

use super::{PackagePath, bundled::Recommended, github::Locator};

pub(crate) const FOLDER: &str = "Scripts";
pub(crate) const NAME: &str = "Skua";
const OWNER: &str = "auqw";
const REPO: &str = "Scripts";
const BRANCH: &str = "Skua";

pub(crate) struct Listed {
    pub(crate) path: PackagePath,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
}

pub(crate) fn listed(root: &Path) -> io::Result<Vec<Listed>> {
    #[derive(serde::Deserialize)]
    struct Raw {
        name: Option<String>,
        description: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
        path: String,
    }
    let raw: Vec<Raw> =
        serde_json::from_slice(&fs::read(root.join("scripts.json"))?).map_err(io::Error::other)?;
    Ok(raw
        .into_iter()
        .filter_map(|raw| {
            let name = known(raw.name)?;
            let path = PackagePath::parse(&raw.path)?;
            let file = root.join(path.as_str());
            if !(crate::scripting::is_skua(&file) && file.is_file()) {
                return None;
            }
            let mut tags: Vec<String> = Vec::new();
            for tag in raw
                .tags
                .into_iter()
                .flatten()
                .filter_map(|tag| known(Some(tag)))
            {
                if !tags.iter().any(|kept| kept.eq_ignore_ascii_case(&tag)) {
                    tags.push(tag);
                }
            }
            Some(Listed {
                path,
                name,
                description: known(raw.description),
                tags,
            })
        })
        .collect())
}

fn known(text: Option<String>) -> Option<String> {
    let text = text?.trim().to_owned();
    (!text.is_empty() && !text.eq_ignore_ascii_case("null")).then_some(text)
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
        name: NAME.into(),
        description: Some("Run Skua scripts in Mira. Experimental, and needs .NET 10.".into()),
        locator: Locator {
            owner: OWNER.into(),
            repo: REPO.into(),
            branch: Some(BRANCH.into()),
            path: PackagePath::default(),
        },
    }
}
