use std::{fs::File, io, path::Path, sync::LazyLock};

use anyhow::{Context as _, Result, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::packages::github::GitHub;

pub(super) const ZIP: &str = "Mira.zip";
static REPOSITORY: LazyLock<(&str, &str)> = LazyLock::new(|| {
    env!("CARGO_PKG_REPOSITORY")
        .strip_prefix("https://github.com/")
        .and_then(|path| path.split_once('/'))
        .expect("the package repository is on GitHub")
});

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Update {
    pub(crate) version: Version,
    id: u64,
    size: u64,
    sha256: String,
}

impl Update {
    #[cfg(test)]
    pub(crate) fn preview(version: Version) -> Self {
        Self {
            version,
            id: 0,
            size: 1,
            sha256: String::new(),
        }
    }
}

impl Release {
    fn update(self, current: &Version) -> Result<Option<Update>> {
        let Some(version) = self
            .tag_name
            .strip_prefix('v')
            .and_then(|v| Version::parse(v).ok())
            .filter(|v| v.pre.is_empty() && v > current)
        else {
            return Ok(None);
        };
        let asset = self
            .assets
            .into_iter()
            .find(|a| a.name == ZIP)
            .with_context(|| format!("Mira {version} has no {ZIP} to download"))?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .context("GitHub didn’t provide a checksum for the download")?
            .to_ascii_lowercase();
        ensure!(asset.size > 0, "The download for Mira {version} is empty");
        Ok(Some(Update {
            version,
            id: asset.id,
            size: asset.size,
            sha256,
        }))
    }
}

pub(super) async fn latest(github: &GitHub, current: &Version) -> Result<Option<Update>> {
    let release: Option<Release> = github
        .latest_release(REPOSITORY.0, REPOSITORY.1)
        .await
        .context("Couldn’t check GitHub for updates")?;
    release.map_or(Ok(None), |release| release.update(current))
}

pub(super) async fn download(
    github: &GitHub,
    update: &Update,
    to: &Path,
    mut progress: impl FnMut(f32),
) -> Result<()> {
    github
        .download_asset(
            REPOSITORY.0,
            REPOSITORY.1,
            update.id,
            to,
            update.size,
            |bytes| progress(bytes as f32 / update.size as f32),
        )
        .await
        .with_context(|| format!("Couldn’t download Mira {}", update.version))?;
    let mut hash = Sha256::new();
    let size = io::copy(&mut File::open(to)?, &mut hash)?;
    ensure!(
        size == update.size && hex::encode(hash.finalize()) == update.sha256,
        "The download didn’t match GitHub’s checksum"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            assets: vec![Asset {
                id: 17,
                name: ZIP.into(),
                size: 512,
                digest: Some(format!("sha256:{}", "AB".repeat(32))),
            }],
        }
    }

    #[test]
    fn offers_only_a_newer_stable_zip_with_a_github_checksum() {
        let current = Version::new(0, 1, 1);
        let update = release("v0.1.2").update(&current).unwrap().unwrap();
        assert_eq!(update.version, Version::new(0, 1, 2));
        assert_eq!(update.sha256, "ab".repeat(32));
        for tag in ["v0.1.0", "v0.1.1", "v0.1.2-beta.1", "0.1.2", "unversioned"] {
            assert!(release(tag).update(&current).unwrap().is_none(), "{tag}");
        }
        let mut unverified = release("v0.1.2");
        unverified.assets[0].digest = None;
        assert!(unverified.update(&current).is_err());
        let mut missing = release("v0.1.2");
        missing.assets[0].name = "Mira-osx-arm64-Portable.zip".into();
        assert!(missing.update(&current).is_err());
    }
}
