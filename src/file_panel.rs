use std::{cell::Cell, future::Future, path::Path, path::PathBuf};

use block2::RcBlock;
use gpui::{App, Window};
use objc2::{class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSView, NSWindow,
};
use objc2_foundation::{NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tokio::sync::oneshot;

#[derive(Clone, Copy)]
pub(crate) enum Pick {
    File,
    Folder,
}

pub(crate) fn open(
    pick: Pick,
    prompt: &'static str,
    window: &Window,
    cx: &App,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    let (done, chosen) = oneshot::channel();
    if let Some(parent) = ns_window(window) {
        cx.foreground_executor()
            .spawn(async move {
                if parent.attachedSheet().is_some() {
                    return;
                }
                let panel: Option<Retained<NSOpenPanel>> =
                    unsafe { msg_send![class!(NSOpenPanel), openPanel] };
                let Some(panel) = panel else {
                    tracing::warn!("macOS couldn't create the open panel");
                    return;
                };
                panel.setCanChooseFiles(matches!(pick, Pick::File));
                panel.setCanChooseDirectories(matches!(pick, Pick::Folder));
                panel.setAllowsMultipleSelection(false);
                panel.setCanCreateDirectories(true);
                panel.setResolvesAliases(false);
                panel.setPrompt(Some(&NSString::from_str(prompt)));
                let done = Cell::new(Some(done));
                let handler = RcBlock::new({
                    let panel = panel.clone();
                    move |response: NSModalResponse| {
                        let path = (response == NSModalResponseOK)
                            .then(|| panel.URL())
                            .flatten()
                            .and_then(|url| url.to_file_path());
                        if let Some(done) = done.take() {
                            let _ = done.send(path);
                        }
                    }
                });
                panel.beginSheetModalForWindow_completionHandler(&parent, &handler);
            })
            .detach();
    }
    async move { chosen.await.ok().flatten() }
}

pub(crate) fn save(
    directory: &Path,
    name: &str,
    window: &Window,
    cx: &App,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    let (done, chosen) = oneshot::channel();
    if let Some(parent) = ns_window(window) {
        let directory = NSURL::from_directory_path(directory);
        let name = NSString::from_str(name);
        cx.foreground_executor()
            .spawn(async move {
                if parent.attachedSheet().is_some() {
                    return;
                }
                let panel: Option<Retained<NSSavePanel>> =
                    unsafe { msg_send![class!(NSSavePanel), savePanel] };
                let Some(panel) = panel else {
                    tracing::warn!("macOS couldn't create the save panel");
                    return;
                };
                panel.setDirectoryURL(directory.as_deref());
                panel.setNameFieldStringValue(&name);
                let done = Cell::new(Some(done));
                let handler = RcBlock::new({
                    let panel = panel.clone();
                    move |response: NSModalResponse| {
                        let path = (response == NSModalResponseOK)
                            .then(|| panel.URL())
                            .flatten()
                            .and_then(|url| url.to_file_path())
                            .map(without_doubled_extension);
                        if let Some(done) = done.take() {
                            let _ = done.send(path);
                        }
                    }
                });
                panel.beginSheetModalForWindow_completionHandler(&parent, &handler);
            })
            .detach();
    }
    async move { chosen.await.ok().flatten() }
}

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    unsafe { handle.ns_view.cast::<NSView>().as_ref() }.window()
}

// macOS 15 can append part of the extension again when saving, turning `a.json` into `a.json.j`.
fn without_doubled_extension(mut path: PathBuf) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path;
    };
    let parts: Vec<_> = name.split('.').collect();
    let doubled = matches!(parts[..], [_, second, third] if second.starts_with(third));
    if doubled {
        path.set_extension("");
    }
    path
}
