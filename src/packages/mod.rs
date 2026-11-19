mod archive;
mod bundled;
#[cfg(test)]
mod fake;
mod github;
mod manifest;
mod service;
mod skua;
mod state;
mod store;
mod token;

use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

pub(crate) use bundled::{Recommended, resources};
#[cfg(test)]
pub(crate) use github::Locator;
pub(crate) use manifest::{Manifest, ManifestError};
pub(crate) use service::{Failure, Updated};
pub(crate) use skua::{FOLDER as SKUA_FOLDER, listed as skua_listed};
pub(crate) use state::{Activity, Checked, Status, init, packages, start};
pub(crate) use store::Edits;

pub(crate) const RECEIPT: &str = ".mira-install.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Mira,
    Skua,
}

impl Kind {
    fn of(locator: &github::Locator) -> Self {
        if skua::is(locator) {
            Self::Skua
        } else {
            Self::Mira
        }
    }

    fn generated(self) -> &'static [&'static str] {
        match self {
            Self::Mira => &["tsconfig.json"],
            Self::Skua => &[],
        }
    }

    fn rewritten(self) -> &'static [&'static str] {
        match self {
            Self::Mira => &[],
            Self::Skua => &["QuestData.json"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    pub(crate) url: String,
    pub(crate) commit: String,
}

pub(crate) fn origin(package: &Path) -> Result<Option<Origin>, String> {
    Ok(store::Receipt::read(package)?.map(|receipt| {
        let mut url = format!("https://github.com/{}/{}", receipt.owner, receipt.repo);
        if !receipt.path.is_root() {
            url = format!("{url}/tree/{}/{}", receipt.branch, receipt.path);
        }
        Origin {
            url,
            commit: receipt.commit.short().to_owned(),
        }
    }))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct PackagePath(String);

impl PackagePath {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let safe =
            |segment: &str| !matches!(segment, "" | "." | "..") && !segment.contains(['\\', '\0']);
        (text.is_empty() || text.split('/').all(safe)).then(|| Self(text.to_owned()))
    }

    pub(crate) fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }

    pub(crate) fn split_last(&self) -> Option<(Self, &str)> {
        match self.0.rsplit_once('/') {
            Some((parent, last)) => Some((Self(parent.to_owned()), last)),
            None if self.is_root() => None,
            None => Some((Self::default(), &self.0)),
        }
    }

    pub(crate) fn join(&self, segment: &str) -> Option<Self> {
        if self.is_root() {
            Self::parse(segment)
        } else {
            Self::parse(&format!("{}/{segment}", self.0))
        }
    }

    pub(crate) fn strip_prefix(&self, prefix: &Self) -> Option<Self> {
        if prefix.is_root() {
            return Some(self.clone());
        }
        let rest = self.0.strip_prefix(&prefix.0)?;
        match rest.strip_prefix('/') {
            Some(rest) => Some(Self(rest.to_owned())),
            None if rest.is_empty() => Some(Self::default()),
            None => None,
        }
    }
}

impl TryFrom<String> for PackagePath {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::parse(&text).ok_or_else(|| format!("{text} isn't a safe relative path"))
    }
}

impl From<PackagePath> for String {
    fn from(path: PackagePath) -> Self {
        path.0
    }
}

impl fmt::Display for PackagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Sha(String);

impl Sha {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        (text.len() == 40
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn short(&self) -> &str {
        &self.0[..7]
    }
}

impl TryFrom<String> for Sha {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::parse(&text).ok_or_else(|| format!("{text} isn't a full commit SHA"))
    }
}

impl From<Sha> for String {
    fn from(sha: Sha) -> Self {
        sha.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_paths_stay_inside_their_package() {
        for safe in ["", "scripts", "script-packages/dailies", "a b/c.ts"] {
            assert!(PackagePath::parse(safe).is_some(), "{safe}");
        }
        for unsafe_path in [
            "/", "/etc", "a//b", "a/", "..", "a/../b", "./a", "a\\b", "a\0",
        ] {
            assert!(PackagePath::parse(unsafe_path).is_none(), "{unsafe_path:?}");
        }
        let path = PackagePath::parse("script-packages/dailies/scripts").unwrap();
        let prefix = PackagePath::parse("script-packages/dailies").unwrap();
        assert_eq!(path.strip_prefix(&prefix).unwrap().as_str(), "scripts");
        assert_eq!(prefix.strip_prefix(&prefix), Some(PackagePath::default()));
        let sibling = PackagePath::parse("script-packages/dailies-old/x").unwrap();
        assert_eq!(sibling.strip_prefix(&prefix), None);
        assert_eq!(
            prefix.split_last(),
            Some((PackagePath::parse("script-packages").unwrap(), "dailies"))
        );
    }
}
