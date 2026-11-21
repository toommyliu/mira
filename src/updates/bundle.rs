use std::{
    ffi::CString,
    fs,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context as _, Result, bail, ensure};
use objc2_foundation::{
    NSFileManager, NSFileManagerItemReplacementOptions, NSSearchPathDirectory,
    NSSearchPathDomainMask, NSString, NSURL,
};
use semver::Version;

const APP: &str = "Mira.app";

pub(super) fn locate() -> Result<PathBuf, Option<String>> {
    let exe = std::env::current_exe().map_err(|error| Some(error.to_string()))?;
    let app = exe
        .ancestors()
        .nth(3)
        .filter(|app| !cfg!(debug_assertions) && app.extension().is_some_and(|e| e == "app"))
        .ok_or(None)?;
    if app
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Err(Some("Move Mira to Applications to get updates.".into()));
    }
    let folder = app.parent().ok_or(None)?;
    if !writable(folder) {
        return Err(Some(format!(
            "Mira can’t replace itself in {}.",
            folder.display()
        )));
    }
    Ok(app.to_path_buf())
}

fn writable(path: &Path) -> bool {
    CString::new(path.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

fn url(path: &Path) -> objc2::rc::Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn file_error(error: objc2::rc::Retained<objc2_foundation::NSError>) -> anyhow::Error {
    anyhow::anyhow!(error.localizedDescription().to_string())
}

#[derive(Clone)]
pub(crate) struct Staged {
    pub(crate) version: Version,
    target: PathBuf,
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
                && info(&app, "CFBundleShortVersionString")? == staged.version.to_string(),
            "The download isn’t Mira {}",
            staged.version
        );
        Ok(staged)
    }

    pub(super) fn install(&self) -> Result<()> {
        NSFileManager::defaultManager()
            .replaceItemAtURL_withItemAtURL_backupItemName_options_resultingItemURL_error(
                &url(&self.target),
                &url(&self.dir.join(APP)),
                None,
                NSFileManagerItemReplacementOptions::empty(),
                None,
            )
            .map_err(file_error)
            .with_context(|| format!("Couldn’t install Mira {}", self.version))?;
        self.discard();
        Ok(())
    }

    pub(super) fn discard(&self) {
        if let Err(error) = fs::remove_dir_all(&self.dir) {
            tracing::warn!(%error, dir = %self.dir.display(), "couldn't remove the update folder");
        }
    }

    pub(super) fn relaunch(&self) -> Result<()> {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"while /bin/kill -0 "$1" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open "$2""#)
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(&self.target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Couldn’t schedule Mira to reopen")?;
        Ok(())
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

fn info(app: &Path, key: &str) -> Result<String> {
    let output = run(Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist")))?;
    Ok(String::from_utf8(output)?.trim().to_owned())
}

fn run(command: &mut Command) -> Result<Vec<u8>> {
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
<key>CFBundleIdentifier</key><string>local.mira.app</string>
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
        let zip = root.join(format!("{version}.zip"));
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(&zip))
        .unwrap();
        zip
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
    }
}
