use std::{
    ffi::CString,
    fs,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context as _, Result, bail, ensure};
use objc2_foundation::{
    NSBundle, NSFileManager, NSFileManagerItemReplacementOptions, NSSearchPathDirectory,
    NSSearchPathDomainMask, NSString, NSURL,
};
use semver::Version;

pub(super) const APP: &str = "Mira.app";
pub(super) const ID: &str = "com.mira.app";

pub(super) fn packaged() -> Option<PathBuf> {
    let bundle = NSBundle::mainBundle();
    (!cfg!(debug_assertions)
        && bundle
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == ID))
    .then(|| PathBuf::from(bundle.bundlePath().to_string()))
}

pub(super) fn translocated(app: &Path) -> bool {
    app.components()
        .any(|c| c.as_os_str() == "AppTranslocation")
}

pub(super) fn locate() -> Result<PathBuf, Option<String>> {
    let app = packaged().ok_or(None)?;
    if translocated(&app) {
        return Err(Some("Move Mira to Applications to get updates.".into()));
    }
    let folder = app.parent().ok_or(None)?;
    if !writable(folder) {
        return Err(Some(format!(
            "Mira can’t replace itself in {}.",
            folder.display()
        )));
    }
    Ok(app)
}

pub(super) fn writable(path: &Path) -> bool {
    CString::new(path.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

pub(super) fn url(path: &Path) -> objc2::rc::Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

pub(super) fn file_error(error: objc2::rc::Retained<objc2_foundation::NSError>) -> anyhow::Error {
    anyhow::anyhow!(error.localizedDescription().to_string())
}

#[derive(Clone)]
pub(crate) struct Staged {
    pub(crate) version: Version,
    pub(super) target: PathBuf,
    dir: PathBuf,
}

impl Staged {
    pub(super) fn workspace(target: &Path) -> Result<PathBuf> {
        let dir = NSFileManager::defaultManager()
            .URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::ItemReplacementDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                Some(&url(target)),
                true,
            )
            .map_err(file_error)?;
        dir.path()
            .map(|path| PathBuf::from(path.to_string()))
            .context("macOS gave no folder for the update")
    }

    pub(super) fn unpack(
        target: PathBuf,
        dir: PathBuf,
        zip: &Path,
        version: Version,
    ) -> Result<Self> {
        let staged = Self {
            version,
            target,
            dir,
        };
        run(Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(zip)
            .arg(&staged.dir))
        .context("Couldn’t unpack the update")?;
        fs::remove_file(zip)?;
        let app = staged.dir.join(APP);
        ensure!(
            app.join("Contents/MacOS/mira").is_file()
                && info(&app, "CFBundleIdentifier")? == info(&staged.target, "CFBundleIdentifier")?
                && release_version(&app)? == staged.version,
            "The download isn’t Mira {}",
            staged.version
        );
        Ok(staged)
    }

    pub(super) fn install(&self) -> Result<()> {
        replace(&self.target, &self.dir.join(APP))
            .with_context(|| format!("Couldn’t install Mira {}", self.version))?;
        self.discard();
        Ok(())
    }

    pub(super) fn discard(&self) {
        if let Err(error) = fs::remove_dir_all(&self.dir) {
            tracing::warn!(%error, dir = %self.dir.display(), "couldn't remove the update folder");
        }
    }

    #[cfg(test)]
    pub(crate) fn preview(version: Version) -> Self {
        Self {
            version,
            target: PathBuf::new(),
            dir: PathBuf::new(),
        }
    }
}

pub(super) fn replace(target: &Path, with: &Path) -> Result<()> {
    NSFileManager::defaultManager()
        .replaceItemAtURL_withItemAtURL_backupItemName_options_resultingItemURL_error(
            &url(target),
            &url(with),
            None,
            NSFileManagerItemReplacementOptions::empty(),
            None,
        )
        .map_err(file_error)?;
    Ok(())
}

pub(super) fn reopen(app: &Path) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while /bin/kill -0 "$1" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Couldn’t schedule Mira to reopen")?;
    Ok(())
}

pub(super) fn release_version(app: &Path) -> Result<Version> {
    let version =
        info(app, "MiraReleaseVersion").or_else(|_| info(app, "CFBundleShortVersionString"))?;
    Ok(Version::parse(&version)?)
}

pub(super) fn info(app: &Path, key: &str) -> Result<String> {
    let output = run(Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist")))?;
    Ok(String::from_utf8(output)?.trim().to_owned())
}

pub(super) fn run(command: &mut Command) -> Result<Vec<u8>> {
    let output = command.stdin(Stdio::null()).output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(root: &Path, version: &str, marker: &str) -> PathBuf {
        let app = root.join(APP);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/MacOS/mira"), marker).unwrap();
        std::os::unix::fs::symlink("MacOS/mira", app.join("Contents/link")).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{ID}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        app
    }

    fn zip(root: &Path, version: &str) -> PathBuf {
        let source = root.join(format!("build-{version}"));
        let app = app(&source, version, version);
        if let Some((base, _)) = version.split_once('-') {
            let plist = app.join("Contents/Info.plist");
            run(Command::new("/usr/bin/plutil")
                .args(["-replace", "CFBundleShortVersionString", "-string", base])
                .arg(&plist))
            .unwrap();
            run(Command::new("/usr/bin/plutil")
                .args(["-insert", "MiraReleaseVersion", "-string", version])
                .arg(plist))
            .unwrap();
        }
        let zip = root.join(format!("{version}.zip"));
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(&zip))
        .unwrap();
        zip
    }

    #[test]
    fn the_packaging_script_stamps_the_id_mira_checks_for() {
        let script = include_str!("../../scripts/bundle-app");
        assert!(
            script
                .lines()
                .any(|line| line == format!("identifier={ID}"))
        );
    }

    #[test]
    fn installs_a_downloaded_zip_over_the_running_app() {
        let root = crate::storyboard::scratch("update-install");
        let target = app(&root.join("Applications"), "0.1.0", "old");
        let new = Version::new(0, 2, 0);

        let wrong = Staged::unpack(
            target.clone(),
            Staged::workspace(&target).unwrap(),
            &zip(&root, "0.1.9"),
            new.clone(),
        );
        assert!(wrong.is_err());

        let staged = Staged::unpack(
            target.clone(),
            Staged::workspace(&target).unwrap(),
            &zip(&root, "0.2.0"),
            new,
        )
        .unwrap();
        assert_eq!(
            info(&target, "CFBundleShortVersionString").unwrap(),
            "0.1.0"
        );
        staged.install().unwrap();
        assert_eq!(
            info(&target, "CFBundleShortVersionString").unwrap(),
            "0.2.0"
        );
        assert_eq!(
            fs::read_to_string(target.join("Contents/link")).unwrap(),
            "0.2.0"
        );
        assert!(!staged.dir.exists());

        for run in [41, 42] {
            let version = format!("0.2.1-nightly.20261127.{run}");
            let staged = Staged::unpack(
                target.clone(),
                Staged::workspace(&target).unwrap(),
                &zip(&root, &version),
                Version::parse(&version).unwrap(),
            )
            .unwrap();
            staged.install().unwrap();
            assert_eq!(
                info(&target, "CFBundleShortVersionString").unwrap(),
                "0.2.1"
            );
            assert_eq!(release_version(&target).unwrap().to_string(), version);
        }
        let mismatch = Staged::unpack(
            target.clone(),
            Staged::workspace(&target).unwrap(),
            &zip(&root, "0.2.1-nightly.20261127.43"),
            Version::parse("0.2.1-nightly.20261127.44").unwrap(),
        );
        assert!(mismatch.is_err());
    }
}
