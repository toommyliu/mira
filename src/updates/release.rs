use std::{fs::File, io, path::Path, sync::LazyLock};

use anyhow::{Context as _, Result, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::Channel;
use crate::packages::github::{GitHub, GitHubError};

pub(super) const ZIP: &str = "Mira.zip";
static REPOSITORY: LazyLock<(&str, &str)> = LazyLock::new(|| {
    env!("CARGO_PKG_REPOSITORY")
        .strip_prefix("https://github.com/")
        .and_then(|path| path.split_once('/'))
        .expect("the package repository is on GitHub")
});

#[derive(Deserialize)]
struct Release {
    draft: bool,
    prerelease: bool,
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
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
    pub(crate) notes: Option<String>,
    pub(crate) page: String,
    id: u64,
    size: u64,
    sha256: String,
}

impl Update {
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    #[cfg(test)]
    pub(crate) fn preview(version: Version) -> Self {
        Self {
            notes: Some(
                "- **Sheets for file panels.** Open and save panels attach to the window that asked for them.\n- **Update from the sidebar.** Download and restart Mira without opening Settings.\n- Fixes a crash when closing a tab mid-login."
                    .into(),
            ),
            page: format!(
                "{}/releases/tag/v{version}",
                env!("CARGO_PKG_REPOSITORY")
            ),
            version,
            id: 0,
            size: 18_000_000,
            sha256: String::new(),
        }
    }
}

impl Release {
    fn version(&self, channel: Channel) -> Option<Version> {
        if self.draft || self.prerelease != (channel == Channel::Nightly) {
            return None;
        }
        let version = Version::parse(self.tag_name.strip_prefix('v')?).ok()?;
        (Channel::of(&version) == Some(channel)).then_some(version)
    }

    fn update(self, version: Version) -> Result<Update> {
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
        Ok(Update {
            version,
            notes: self
                .body
                .map(|b| b.trim().to_owned())
                .filter(|b| !b.is_empty()),
            page: self.html_url,
            id: asset.id,
            size: asset.size,
            sha256,
        })
    }
}

pub(super) async fn latest(
    github: &GitHub,
    current: &Version,
    channel: Channel,
) -> Result<Option<Update>> {
    let newest = match channel {
        Channel::Stable => github
            .latest_release::<Release>(REPOSITORY.0, REPOSITORY.1)
            .await
            .map(|release| release.and_then(|r| Some((r.version(channel)?, r)))),
        Channel::Nightly => newest_nightly(github).await,
    }
    .context("Couldn’t check GitHub for updates")?;
    newest
        .filter(|(v, _)| v > current)
        .map(|(v, r)| r.update(v))
        .transpose()
}

async fn newest_nightly(github: &GitHub) -> Result<Option<(Version, Release)>, GitHubError> {
    let mut page = 1;
    loop {
        let releases: Vec<Release> = github.releases(REPOSITORY.0, REPOSITORY.1, page).await?;
        let last_page = releases.len() < 30;
        let newest = releases
            .into_iter()
            .filter_map(|r| Some((r.version(Channel::Nightly)?, r)))
            .max_by(|(a, _), (b, _)| a.cmp(b));
        if newest.is_some() || last_page {
            return Ok(newest);
        }
        page += 1;
    }
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
            draft: false,
            prerelease: false,
            tag_name: tag.into(),
            html_url: format!("https://github.com/toommyliu/mira/releases/tag/{tag}"),
            body: None,
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
        let version = release("v0.1.2").version(Channel::Stable).unwrap();
        let update = release("v0.1.2").update(version.clone()).unwrap();
        assert_eq!(update.version, Version::new(0, 1, 2));
        assert_eq!(update.sha256, "ab".repeat(32));
        for tag in ["v0.1.0", "v0.1.1", "v0.1.2-beta.1", "0.1.2", "unversioned"] {
            assert!(
                release(tag)
                    .version(Channel::Stable)
                    .is_none_or(|v| v <= current),
                "{tag}"
            );
        }
        let mut unverified = release("v0.1.2");
        unverified.assets[0].digest = None;
        assert!(unverified.update(version.clone()).is_err());
        let mut missing = release("v0.1.2");
        missing.assets[0].name = "Mira-osx-arm64-Portable.zip".into();
        assert!(missing.update(version.clone()).is_err());
    }

    #[tokio::test]
    async fn each_channel_offers_only_its_own_newest_release() {
        use crate::packages::fake::{Fake, Reply};
        use serde_json::json;
        let api = Fake::start();
        let github = GitHub::at(&api.url(), None).unwrap();
        let path = format!("/repos/{}/{}/releases", REPOSITORY.0, REPOSITORY.1);
        let release = |tag: &str, prerelease: bool, draft: bool| {
            json!({
                "tag_name": tag, "prerelease": prerelease, "draft": draft, "html_url": "https://example.com",
                "assets": [{"id": 1, "name": ZIP, "size": 2, "digest": format!("sha256:{}", "aa".repeat(32))}]
            })
        };
        let version = |text: &str| Version::parse(text).unwrap();
        let offered = async |current: &str, channel: Channel| {
            latest(&github, &version(current), channel)
                .await
                .unwrap()
                .map(|update| update.version.to_string())
        };
        api.route(
            &format!("{path}/latest"),
            Reply::json(release("v0.2.1", false, false).to_string()),
        );
        api.route(
            &format!("{path}?page=1"),
            Reply::json(
                serde_json::to_string(&vec![release("v0.2.2-nightly.20261128.50", true, true); 30])
                    .unwrap(),
            ),
        );
        api.route(
            &format!("{path}?page=2"),
            Reply::json(
                json!([
                    release("v0.2.1-nightly.20261127.41", true, false),
                    release("v0.2.1", false, false),
                    release("v0.2.2-nightly.20261128.44", true, false),
                ])
                .to_string(),
            ),
        );
        assert_eq!(
            offered("0.2.0", Channel::Stable).await.as_deref(),
            Some("0.2.1")
        );
        assert_eq!(
            offered("0.2.0", Channel::Nightly).await.as_deref(),
            Some("0.2.2-nightly.20261128.44")
        );
        assert_eq!(
            offered("0.2.1-nightly.20261127.41", Channel::Stable)
                .await
                .as_deref(),
            Some("0.2.1")
        );
        assert_eq!(
            offered("0.2.2-nightly.20261128.44", Channel::Stable).await,
            None
        );
        api.route(
            &format!("{path}?page=2"),
            Reply::json(json!([release("v0.2.1-nightly.20261127.41", true, false)]).to_string()),
        );
        assert_eq!(
            offered("0.2.1-nightly.20261127.41", Channel::Nightly).await,
            None
        );
        assert_eq!(
            offered("0.2.0", Channel::Nightly).await.as_deref(),
            Some("0.2.1-nightly.20261127.41")
        );
    }

    #[test]
    fn only_published_nightly_tags_are_eligible_for_the_nightly_channel() {
        let mut r = release("v0.2.1-nightly.20261127.42");
        assert!(r.version(Channel::Nightly).is_none());
        r.prerelease = true;
        assert!(r.version(Channel::Stable).is_none());
        let nightly = r.version(Channel::Nightly).unwrap();
        assert!(nightly > Version::new(0, 2, 0) && nightly < Version::new(0, 2, 1));
        r.draft = true;
        assert!(r.version(Channel::Nightly).is_none());
        r.draft = false;
        for tag in [
            "v0.2.1-beta.1",
            "v0.2.1",
            "v0.2.1-nightly.20261127.x",
            "v0.2.1-nightly.2026112.42",
            "v0.2.1-nightly.20261127.42+abc",
        ] {
            r.tag_name = tag.into();
            assert!(r.version(Channel::Nightly).is_none(), "{tag}");
        }
    }
}
