use std::{
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result};
use gpui::AsyncApp;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle};
use objc2_foundation::{NSFileManager, NSString, NSURL, ns_string};
use semver::Version;

use super::bundle::{self, Staged};

const SYSTEM: &str = "/Applications";

pub(crate) fn offer(cx: &AsyncApp) -> bool {
    let Some(running) = bundle::packaged() else {
        return false;
    };
    let translocated = bundle::translocated(&running);
    let original = if translocated {
        untranslocated(&running)
    } else {
        Some(running.clone())
    };
    let user = dirs::home_dir().map(|home| home.join("Applications"));
    let installed = original
        .as_deref()
        .filter(|app| in_applications(app, user.as_deref()));
    if installed.is_some() && !translocated {
        return false;
    }
    let target =
        installed.map_or_else(|| folder(user.as_deref()).join(bundle::APP), Path::to_owned);
    let existing = if installed.is_some() {
        Existing::Missing
    } else {
        Existing::at(&target)
    };
    let Some(main) = MainThreadMarker::new() else {
        return false;
    };
    cx.update(|cx| cx.activate(true));
    let (moving, failure) = match &existing {
        Existing::Other => {
            alert(
                main,
                NSAlertStyle::Warning,
                "Mira can’t move to your Applications folder.",
                Some("Another app named Mira.app is already there."),
                &["OK"],
            );
            return false;
        }
        Existing::Current(version) => {
            let message = format!(
                "Mira {version} is already in your Applications folder. Would you like to open it instead?"
            );
            if !alert(
                main,
                NSAlertStyle::Informational,
                &message,
                None,
                &["Open Installed Mira", "Not Now"],
            ) {
                return false;
            }
            (false, "Couldn’t open Mira from Applications.")
        }
        Existing::Missing | Existing::Older => {
            if !alert(
                main,
                NSAlertStyle::Informational,
                "Mira works best in your Applications folder. Would you like to move it now?",
                (existing == Existing::Older)
                    .then_some("This replaces the older Mira that’s already there."),
                &["Move to Applications", "Not Now"],
            ) {
                return false;
            }
            (true, "Couldn’t move Mira to Applications.")
        }
    };
    let ready = if installed.is_none() && Existing::at(&target) != existing {
        Err(anyhow::anyhow!(
            "Mira.app in your Applications folder changed while this alert was open."
        ))
    } else if moving {
        place(&running, &target)
    } else {
        Ok(())
    };
    let moved = ready.and_then(|()| bundle::reopen(&target));
    if moved.is_ok()
        && let Some(original) = original.filter(|original| *original != target)
    {
        trash(&original);
    }
    match moved {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(error = format!("{error:#}"), "{failure}");
            alert(
                main,
                NSAlertStyle::Warning,
                failure,
                Some(&format!("{error:#}")),
                &["OK"],
            );
            false
        }
    }
}

#[derive(Debug, PartialEq)]
enum Existing {
    Missing,
    Older,
    Current(Version),
    Other,
}

impl Existing {
    fn at(target: &Path) -> Self {
        if !target.exists() {
            return Self::Missing;
        }
        if bundle::info(target, "CFBundleIdentifier").ok().as_deref() != Some(bundle::ID) {
            return Self::Other;
        }
        match bundle::info(target, "CFBundleShortVersionString")
            .ok()
            .and_then(|version| Version::parse(&version).ok())
        {
            Some(version) if version >= *super::CURRENT => Self::Current(version),
            _ => Self::Older,
        }
    }
}

fn in_applications(app: &Path, user: Option<&Path>) -> bool {
    app.starts_with(SYSTEM) || user.is_some_and(|user| app.starts_with(user))
}

fn folder(user: Option<&Path>) -> PathBuf {
    match user {
        Some(user) if !bundle::writable(Path::new(SYSTEM)) => user.to_owned(),
        _ => PathBuf::from(SYSTEM),
    }
}

fn alert(
    main: MainThreadMarker,
    style: NSAlertStyle,
    message: &str,
    detail: Option<&str>,
    buttons: &[&str],
) -> bool {
    let alert = NSAlert::new(main);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(message));
    if let Some(detail) = detail {
        alert.setInformativeText(&NSString::from_str(detail));
    }
    for (index, title) in buttons.iter().enumerate() {
        let button = alert.addButtonWithTitle(&NSString::from_str(title));
        if index > 0 && index == buttons.len() - 1 {
            button.setKeyEquivalent(ns_string!("\u{1b}"));
        }
    }
    alert.runModal() == NSAlertFirstButtonReturn
}

fn place(app: &Path, target: &Path) -> Result<()> {
    let folder = target.parent().context("Applications has no parent")?;
    fs::create_dir_all(folder)?;
    let dir = Staged::workspace(folder)?;
    let copy = dir.join(bundle::APP);
    let placed = bundle::run(Command::new("/usr/bin/ditto").arg(app).arg(&copy))
        .and_then(|_| {
            bundle::run(
                Command::new("/usr/bin/xattr")
                    .args(["-dr", "com.apple.quarantine"])
                    .arg(&copy),
            )
        })
        .context("Couldn’t copy Mira")
        .and_then(|_| {
            if target.exists() {
                bundle::replace(target, &copy)
            } else {
                Ok(fs::rename(&copy, target)?)
            }
        });
    let _ = fs::remove_dir_all(&dir);
    placed
}

fn untranslocated(app: &Path) -> Option<PathBuf> {
    type Original = unsafe extern "C" fn(*const NSURL, *mut *mut c_void) -> *mut NSURL;
    let symbol = unsafe {
        let security = libc::dlopen(
            c"/System/Library/Frameworks/Security.framework/Security".as_ptr(),
            libc::RTLD_LAZY,
        );
        if security.is_null() {
            return None;
        }
        libc::dlsym(security, c"SecTranslocateCreateOriginalPathForURL".as_ptr())
    };
    if symbol.is_null() {
        return None;
    }
    let original = unsafe { std::mem::transmute::<*mut c_void, Original>(symbol) };
    let url = unsafe {
        Retained::from_raw(original(
            Retained::as_ptr(&bundle::url(app)),
            std::ptr::null_mut(),
        ))
    }?;
    url.path().map(|path| PathBuf::from(path.to_string()))
}

fn trash(app: &Path) {
    if let Err(error) = NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&bundle::url(app), None)
    {
        tracing::info!(
            error = %error.localizedDescription(),
            path = %app.display(),
            "left the old copy of Mira in place"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_applications_folders_count_as_installed() {
        let user = Path::new("/Users/me/Applications");
        for (app, installed) in [
            ("/Applications/Mira.app", true),
            ("/Applications/Games/Mira.app", true),
            ("/Users/me/Applications/Mira.app", true),
            ("/Users/me/Downloads/Mira.app", false),
            ("/Applications Old/Mira.app", false),
            ("/Volumes/Mira/Mira.app", false),
            (
                "/private/var/folders/x/T/AppTranslocation/1234/d/Mira.app",
                false,
            ),
        ] {
            assert_eq!(
                in_applications(Path::new(app), Some(user)),
                installed,
                "{app}"
            );
        }
        assert!(!in_applications(
            Path::new("/Users/me/Applications/Mira.app"),
            None
        ));
    }

    fn app(root: &Path, marker: &str) -> PathBuf {
        bundled(root, marker, bundle::ID, "0.1.0")
    }

    fn bundled(root: &Path, marker: &str, id: &str, version: &str) -> PathBuf {
        let app = root.join(bundle::APP);
        let binary = app.join("Contents/MacOS/mira");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, marker).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{id}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        bundle::run(
            Command::new("/usr/bin/xattr")
                .args(["-w", "com.apple.quarantine", "0081;00000000;Safari;"])
                .arg(&binary),
        )
        .unwrap();
        app
    }

    fn quarantined(app: &Path) -> bool {
        bundle::run(
            Command::new("/usr/bin/xattr")
                .args(["-p", "com.apple.quarantine"])
                .arg(app.join("Contents/MacOS/mira")),
        )
        .is_ok()
    }

    #[test]
    fn moving_copies_without_quarantine_and_replaces_an_installed_copy() {
        let root = crate::storyboard::scratch("relocate");
        let target = root.join("Applications").join(bundle::APP);

        let downloaded = app(&root.join("Downloads"), "new");
        assert!(quarantined(&downloaded));
        place(&downloaded, &target).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("Contents/MacOS/mira")).unwrap(),
            "new"
        );
        assert!(!quarantined(&target));
        assert!(downloaded.exists());

        let newer = app(&root.join("Desktop"), "newer");
        place(&newer, &target).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("Contents/MacOS/mira")).unwrap(),
            "newer"
        );
        assert!(!quarantined(&target));
    }

    #[test]
    fn an_installed_copy_is_replaced_only_when_it_is_an_older_mira() {
        let root = crate::storyboard::scratch("relocate-existing");
        let target = root.join("Applications").join(bundle::APP);
        assert_eq!(Existing::at(&target), Existing::Missing);

        let current = super::super::CURRENT.to_string();
        for (id, version, expected) in [
            (bundle::ID, "0.0.0-0", Existing::Older),
            (bundle::ID, "not a version", Existing::Older),
            (
                bundle::ID,
                current.as_str(),
                Existing::Current(super::super::CURRENT.clone()),
            ),
            (
                bundle::ID,
                "999.0.0",
                Existing::Current(Version::new(999, 0, 0)),
            ),
            ("com.example.mira", current.as_str(), Existing::Other),
        ] {
            fs::remove_dir_all(root.join("Applications")).ok();
            bundled(&root.join("Applications"), "installed", id, version);
            assert_eq!(Existing::at(&target), expected, "{id} {version}");
        }

        fs::remove_dir_all(root.join("Applications")).unwrap();
        fs::create_dir_all(target.join("Contents")).unwrap();
        assert_eq!(Existing::at(&target), Existing::Other);
    }
}
