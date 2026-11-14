use std::{
    fs::File,
    io::Write as _,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    Response, StatusCode,
    header::{self, HeaderMap},
};
use serde::Deserialize;
use url::Url;

use super::{PackagePath, Sha};

pub(crate) const API: &str = "https://api.github.com";
const USER_AGENT: &str = concat!("Mira/", env!("CARGO_PKG_VERSION"));
const JSON: &str = "application/vnd.github+json";
const RAW: &str = "application/vnd.github.raw";
const SECONDARY_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Locator {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) branch: Option<String>,
    pub(crate) path: PackagePath,
}

impl Locator {
    pub(crate) fn parse(link: &str, branch: &str) -> Result<Self, String> {
        let unusable = || "Use a link like github.com/owner/repo.".to_owned();
        let link = link.trim();
        let link = link.split(['?', '#']).next().unwrap_or_default();
        let link = link
            .strip_prefix("https://")
            .or_else(|| link.strip_prefix("http://"))
            .unwrap_or(link);
        let link = link.strip_prefix("www.").unwrap_or(link);
        let rest = match link.split_once('/') {
            Some(("github.com", rest)) => rest,
            Some((host, _)) if host.contains('.') => return Err(unusable()),
            _ => link,
        };
        let mut segments = rest.trim_end_matches('/').split('/');
        let owner = segments
            .next()
            .filter(|owner| name(owner))
            .ok_or_else(unusable)?;
        let repo = segments.next().ok_or_else(unusable)?;
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        if !name(repo) {
            return Err(unusable());
        }
        let tree: Vec<&str> = match segments.next() {
            None => Vec::new(),
            Some("tree") => segments.collect(),
            Some("blob") => {
                return Err("That's a link to a file. Link to the package's folder.".into());
            }
            Some(_) => return Err(unusable()),
        };
        let typed = branch.trim();
        let (branch, path) = match (typed.is_empty(), tree.as_slice()) {
            (true, []) => (None, String::new()),
            (true, [branch, path @ ..]) => (Some((*branch).to_owned()), path.join("/")),
            (false, []) => (Some(typed.to_owned()), String::new()),
            (false, tree) => {
                let tree = tree.join("/");
                let path = tree
                    .strip_prefix(typed)
                    .and_then(|rest| rest.strip_prefix('/').or(rest.is_empty().then_some("")))
                    .ok_or("The link's branch doesn't match the branch you entered.")?;
                (Some(typed.to_owned()), path.to_owned())
            }
        };
        if let Some(branch) = &branch
            && !valid_branch(branch)
        {
            return Err(format!(
                "\u{201c}{branch}\u{201d} isn't a valid branch name."
            ));
        }
        let path = PackagePath::parse(&path).ok_or("That link points outside the repository.")?;
        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            branch,
            path,
        })
    }
}

fn name(text: &str) -> bool {
    !text.is_empty()
        && !matches!(text, "." | "..")
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn valid_branch(branch: &str) -> bool {
    !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Repository {
    pub(crate) id: u64,
    pub(crate) owner: String,
    pub(crate) name: String,
    pub(crate) default_branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    pub(crate) commit: Sha,
    pub(crate) tree: Sha,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GitHubError {
    #[error("not found")]
    NotFound,
    #[error("the branch doesn't exist")]
    NoBranch,
    #[error("the folder doesn't exist on that branch")]
    NoFolder,
    #[error("the GitHub token expired or was revoked")]
    Unauthorized,
    #[error("GitHub's rate limit was reached")]
    RateLimited { retry_at: SystemTime },
    #[error("it's larger than {} MB", .0 / 1_000_000)]
    TooLarge(u64),
    #[error("GitHub answered {0}")]
    Status(StatusCode),
    #[error("GitHub sent a response Mira couldn't read")]
    Unexpected,
    #[error("{0}")]
    Network(String),
}

impl From<reqwest::Error> for GitHubError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Network("GitHub took too long to answer".into())
        } else {
            Self::Network(format!("couldn't reach GitHub: {error}"))
        }
    }
}

impl From<std::io::Error> for GitHubError {
    fn from(error: std::io::Error) -> Self {
        Self::Network(format!("couldn't save the download: {error}"))
    }
}

#[derive(Clone)]
pub(crate) struct GitHub {
    client: reqwest::Client,
    api: Url,
    token: Option<String>,
}

impl GitHub {
    pub(crate) fn at(api: &str, token: Option<String>) -> Result<Self, GitHubError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()?;
        let api = Url::parse(api).map_err(|_| GitHubError::Unexpected)?;
        Ok(Self { client, api, token })
    }

    pub(crate) fn has_token(&self) -> bool {
        self.token.is_some()
    }

    pub(crate) async fn repository(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Repository, GitHubError> {
        #[derive(Deserialize)]
        struct Owner {
            login: String,
        }
        #[derive(Deserialize)]
        struct Payload {
            id: u64,
            name: String,
            owner: Owner,
            default_branch: String,
        }
        let payload: Payload = self.json(&["repos", owner, repo], None).await?;
        Ok(Repository {
            id: payload.id,
            owner: payload.owner.login,
            name: payload.name,
            default_branch: payload.default_branch,
        })
    }

    pub(crate) async fn head(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        path: &PackagePath,
    ) -> Result<Head, GitHubError> {
        #[derive(Deserialize)]
        struct Tree {
            sha: Sha,
        }
        #[derive(Deserialize)]
        struct Details {
            tree: Tree,
        }
        #[derive(Deserialize)]
        struct Commit {
            sha: Sha,
            commit: Details,
        }
        #[derive(Deserialize)]
        struct Branch {
            commit: Commit,
        }
        #[derive(Deserialize)]
        struct Entry {
            name: String,
            r#type: String,
            sha: Sha,
        }
        let mut segments = vec!["repos", owner, repo, "branches"];
        segments.extend(branch.split('/'));
        let branch: Branch = match self.json(&segments, None).await {
            Err(GitHubError::NotFound) => return Err(GitHubError::NoBranch),
            result => result?,
        };
        let commit = branch.commit.sha;
        let Some((parent, last)) = path.split_last() else {
            return Ok(Head {
                commit,
                tree: branch.commit.commit.tree.sha,
            });
        };
        let mut segments = vec!["repos", owner, repo, "contents"];
        segments.extend(parent.segments());
        let listing: serde_json::Value =
            match self.json(&segments, Some(("ref", commit.as_str()))).await {
                Err(GitHubError::NotFound) => return Err(GitHubError::NoFolder),
                result => result?,
            };
        let entries: Vec<Entry> = match listing {
            serde_json::Value::Array(_) => {
                serde_json::from_value(listing).map_err(|_| GitHubError::Unexpected)?
            }
            _ => return Err(GitHubError::NoFolder),
        };
        entries
            .into_iter()
            .find(|entry| entry.name == last && entry.r#type == "dir")
            .map(|entry| Head {
                commit,
                tree: entry.sha,
            })
            .ok_or(GitHubError::NoFolder)
    }

    pub(crate) async fn file(
        &self,
        owner: &str,
        repo: &str,
        commit: &Sha,
        path: &PackagePath,
        max: u64,
    ) -> Result<Vec<u8>, GitHubError> {
        let mut segments = vec!["repos", owner, repo, "contents"];
        segments.extend(path.segments());
        let mut response = self
            .send(&segments, Some(("ref", commit.as_str())), RAW)
            .await?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > max {
                return Err(GitHubError::TooLarge(max));
            }
        }
        Ok(body)
    }

    pub(crate) async fn download(
        &self,
        owner: &str,
        repo: &str,
        commit: &Sha,
        to: &Path,
        max: u64,
    ) -> Result<(), GitHubError> {
        let mut response = self
            .send(
                &["repos", owner, repo, "zipball", commit.as_str()],
                None,
                JSON,
            )
            .await?;
        if response.content_length().is_some_and(|length| length > max) {
            return Err(GitHubError::TooLarge(max));
        }
        let mut file = File::create(to)?;
        let mut written = 0;
        while let Some(chunk) = response.chunk().await? {
            written += chunk.len() as u64;
            if written > max {
                return Err(GitHubError::TooLarge(max));
            }
            file.write_all(&chunk)?;
        }
        file.sync_all()?;
        Ok(())
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        segments: &[&str],
        query: Option<(&str, &str)>,
    ) -> Result<T, GitHubError> {
        let response = self.send(segments, query, JSON).await?;
        let body = response.bytes().await?;
        serde_json::from_slice(&body).map_err(|_| GitHubError::Unexpected)
    }

    async fn send(
        &self,
        segments: &[&str],
        query: Option<(&str, &str)>,
        accept: &str,
    ) -> Result<Response, GitHubError> {
        let mut url = self.api.clone();
        url.path_segments_mut()
            .map_err(|_| GitHubError::Unexpected)?
            .pop_if_empty()
            .extend(segments);
        if let Some((key, value)) = query {
            url.query_pairs_mut().append_pair(key, value);
        }
        let mut request = self
            .client
            .get(url)
            .header(header::ACCEPT, accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await?;
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::NOT_FOUND => Err(GitHubError::NotFound),
            StatusCode::UNAUTHORIZED => Err(GitHubError::Unauthorized),
            status @ (StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS) => {
                match retry_at(response.headers(), SystemTime::now()) {
                    Some(retry_at) => Err(GitHubError::RateLimited { retry_at }),
                    None if status == StatusCode::TOO_MANY_REQUESTS => {
                        Err(GitHubError::RateLimited {
                            retry_at: SystemTime::now() + SECONDARY_WAIT,
                        })
                    }
                    None => Err(GitHubError::Status(status)),
                }
            }
            status => Err(GitHubError::Status(status)),
        }
    }
}

fn retry_at(headers: &HeaderMap, now: SystemTime) -> Option<SystemTime> {
    let number = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    if let Some(seconds) = number("retry-after") {
        return Some(now + Duration::from_secs(seconds));
    }
    if number("x-ratelimit-remaining") == Some(0) {
        let reset = number("x-ratelimit-reset")
            .map(|epoch| UNIX_EPOCH + Duration::from_secs(epoch))
            .unwrap_or(now + SECONDARY_WAIT);
        return Some(reset.max(now));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::fake::{Fake, Reply};

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const ROOT_TREE: &str = "1111111111111111111111111111111111111111";
    const FOLDER_TREE: &str = "2222222222222222222222222222222222222222";

    fn locator(link: &str, branch: &str) -> Result<Locator, String> {
        Locator::parse(link, branch)
    }

    fn located(owner: &str, repo: &str, branch: Option<&str>, path: &str) -> Locator {
        Locator {
            owner: owner.into(),
            repo: repo.into(),
            branch: branch.map(Into::into),
            path: PackagePath::parse(path).unwrap(),
        }
    }

    #[test]
    fn links_name_a_repository_and_optionally_a_branch_and_folder() {
        for link in [
            "https://github.com/toommyliu/mira",
            "github.com/toommyliu/mira/",
            "https://www.github.com/toommyliu/mira.git",
            "toommyliu/mira",
            "https://github.com/toommyliu/mira?tab=readme#install",
        ] {
            assert_eq!(
                locator(link, ""),
                Ok(located("toommyliu", "mira", None, "")),
                "{link}"
            );
        }
        assert_eq!(
            locator(
                "https://github.com/toommyliu/mira/tree/main/script-packages/dailies",
                ""
            ),
            Ok(located(
                "toommyliu",
                "mira",
                Some("main"),
                "script-packages/dailies"
            ))
        );
        assert_eq!(
            locator(
                "github.com/toommyliu/mira/tree/feat/x/script-packages/dailies",
                "feat/x"
            ),
            Ok(located(
                "toommyliu",
                "mira",
                Some("feat/x"),
                "script-packages/dailies"
            ))
        );
        assert_eq!(
            locator("github.com/toommyliu/mira", "dev"),
            Ok(located("toommyliu", "mira", Some("dev"), ""))
        );
        for (link, branch, problem) in [
            (
                "https://gitlab.com/a/b",
                "",
                "Use a link like github.com/owner/repo.",
            ),
            (
                "github.com/toommyliu",
                "",
                "Use a link like github.com/owner/repo.",
            ),
            (
                "github.com/a/b/issues",
                "",
                "Use a link like github.com/owner/repo.",
            ),
            (
                "github.com/a/b/blob/main/package.json",
                "",
                "That's a link to a file. Link to the package's folder.",
            ),
            (
                "github.com/a/b/tree/main/x",
                "dev",
                "The link's branch doesn't match the branch you entered.",
            ),
            (
                "github.com/a/b/tree/main/../x",
                "",
                "That link points outside the repository.",
            ),
            (
                "github.com/a/b",
                "bad..name",
                "\u{201c}bad..name\u{201d} isn't a valid branch name.",
            ),
        ] {
            assert_eq!(locator(link, branch), Err(problem.into()), "{link}");
        }
    }

    fn branch_reply() -> Reply {
        Reply::json(format!(
            r#"{{"name":"main","commit":{{"sha":"{COMMIT}","commit":{{"tree":{{"sha":"{ROOT_TREE}"}}}}}}}}"#
        ))
    }

    #[tokio::test]
    async fn a_head_names_the_commit_and_the_tree_of_the_package_folder() {
        let fake = Fake::start();
        fake.route("/repos/o/r/branches/feat/x", branch_reply());
        fake.route(
            &format!("/repos/o/r/contents/script-packages?ref={COMMIT}"),
            Reply::json(format!(
                r#"[{{"name":"dailies","type":"dir","sha":"{FOLDER_TREE}"}},{{"name":"notes.md","type":"file","sha":"{ROOT_TREE}"}}]"#
            )),
        );
        let github = GitHub::at(&fake.url(), None).unwrap();
        let head = |path: &str| {
            let github = github.clone();
            let path = PackagePath::parse(path).unwrap();
            async move { github.head("o", "r", "feat/x", &path).await }
        };
        let commit = Sha::parse(COMMIT).unwrap();
        assert_eq!(
            head("").await.unwrap(),
            Head {
                commit: commit.clone(),
                tree: Sha::parse(ROOT_TREE).unwrap()
            }
        );
        assert_eq!(
            head("script-packages/dailies").await.unwrap(),
            Head {
                commit,
                tree: Sha::parse(FOLDER_TREE).unwrap()
            }
        );
        assert!(matches!(
            head("script-packages/notes.md").await,
            Err(GitHubError::NoFolder)
        ));
        assert!(matches!(
            head("missing/weekly").await,
            Err(GitHubError::NoFolder)
        ));
        assert!(matches!(
            github.head("o", "r", "gone", &PackagePath::default()).await,
            Err(GitHubError::NoBranch)
        ));
    }

    #[tokio::test]
    async fn github_failures_say_what_went_wrong() {
        let fake = Fake::start();
        fake.route("/repos/o/private", Reply::status(404));
        fake.route("/repos/o/expired", Reply::status(401));
        fake.route(
            "/repos/o/limited",
            Reply::status(403)
                .header("x-ratelimit-remaining", "0")
                .header("x-ratelimit-reset", "4102444800"),
        );
        fake.route(
            "/repos/o/secondary",
            Reply::status(403).header("retry-after", "30"),
        );
        fake.route("/repos/o/forbidden", Reply::status(403));
        fake.route("/repos/o/garbled", Reply::json("{"));
        let github = GitHub::at(&fake.url(), Some("token".into())).unwrap();
        assert!(matches!(
            github.repository("o", "private").await,
            Err(GitHubError::NotFound)
        ));
        assert!(matches!(
            github.repository("o", "expired").await,
            Err(GitHubError::Unauthorized)
        ));
        assert!(matches!(
            github.repository("o", "limited").await,
            Err(GitHubError::RateLimited { retry_at }) if retry_at == UNIX_EPOCH + Duration::from_secs(4_102_444_800)
        ));
        let before = SystemTime::now();
        assert!(matches!(
            github.repository("o", "secondary").await,
            Err(GitHubError::RateLimited { retry_at }) if retry_at >= before + Duration::from_secs(30)
        ));
        assert!(matches!(
            github.repository("o", "forbidden").await,
            Err(GitHubError::Status(StatusCode::FORBIDDEN))
        ));
        assert!(matches!(
            github.repository("o", "garbled").await,
            Err(GitHubError::Unexpected)
        ));
    }

    #[tokio::test]
    async fn the_token_goes_to_github_but_not_to_the_archive_host() {
        let archive = Fake::start();
        archive.route("/archive.zip", Reply::bytes(b"zip bytes".to_vec()));
        let api = Fake::start();
        api.route(
            &format!("/repos/o/r/zipball/{COMMIT}"),
            Reply::status(302).header("location", &format!("{}/archive.zip", archive.url())),
        );
        api.route(
            "/repos/o/r",
            Reply::json(r#"{"id":7,"name":"R","owner":{"login":"O"},"default_branch":"trunk"}"#),
        );
        let github = GitHub::at(&api.url(), Some("secret".into())).unwrap();
        assert_eq!(
            github.repository("o", "r").await.unwrap(),
            Repository {
                id: 7,
                owner: "O".into(),
                name: "R".into(),
                default_branch: "trunk".into(),
            }
        );
        let folder = tempfile();
        let to = folder.join("download.zip");
        github
            .download("o", "r", &Sha::parse(COMMIT).unwrap(), &to, 1_000)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"zip bytes");
        assert!(
            api.requests()
                .iter()
                .all(|request| request.contains("authorization: Bearer secret\r\n"))
        );
        assert!(
            archive
                .requests()
                .iter()
                .all(|request| !request.to_lowercase().contains("authorization"))
        );
        let small = github
            .download("o", "r", &Sha::parse(COMMIT).unwrap(), &to, 4)
            .await;
        assert!(matches!(small, Err(GitHubError::TooLarge(4))));
        std::fs::remove_dir_all(folder).unwrap();
    }

    fn tempfile() -> std::path::PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder =
            std::env::temp_dir().join(format!("mira-github-{:x}", u64::from_ne_bytes(random)));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }
}
