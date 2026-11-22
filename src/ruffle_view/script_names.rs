use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use gpui::{App, Global, SharedString};

use super::account::file_name;
use crate::scripting;

#[derive(Default)]
pub(crate) struct Names(RefCell<HashMap<PathBuf, Option<SharedString>>>);

impl Global for Names {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Names::default());
}

pub(crate) fn name(path: &Path, cx: &App) -> SharedString {
    let Some(names) = cx.try_global::<Names>() else {
        return file_name(path).into();
    };
    let mut known = names.0.borrow_mut();
    match known.get(path) {
        Some(Some(name)) => return name.clone(),
        Some(None) => {}
        None => {
            known.insert(path.to_owned(), None);
            resolve(path.to_owned(), cx);
        }
    }
    file_name(path).into()
}

pub(crate) fn refresh(path: &Path, cx: &App) {
    if cx.has_global::<Names>() {
        resolve(path.to_owned(), cx);
    }
}

pub(crate) fn learn(names: impl IntoIterator<Item = (PathBuf, SharedString)>, cx: &mut App) {
    let Some(current) = cx.try_global::<Names>() else {
        return;
    };
    let changed: Vec<_> = {
        let known = current.0.borrow();
        names
            .into_iter()
            .filter(|(path, name)| known.get(path) != Some(&Some(name.clone())))
            .collect()
    };
    if changed.is_empty() {
        return;
    }
    let known = cx.global_mut::<Names>().0.get_mut();
    for (path, name) in changed {
        known.insert(path, Some(name));
    }
}

fn resolve(path: PathBuf, cx: &App) {
    let read = cx.background_executor().spawn(async move {
        let name = fs::read_to_string(&path)
            .ok()
            .and_then(|text| scripting::display_name(&path, &text))
            .unwrap_or_else(|| file_name(&path));
        (path, SharedString::from(name))
    });
    cx.spawn(async move |cx| {
        let named = read.await;
        cx.update(|cx| learn([named], cx));
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[gpui::test]
    fn names_come_from_meta_once_read(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-names-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir_all(&temp).unwrap();
        let hunt = temp.join("hunt.ts");
        let plain = temp.join("plain.ts");
        fs::write(&hunt, "export const meta = { name: \"Hunt\" };").unwrap();
        fs::write(&plain, "export default function* () {}").unwrap();
        cx.update(init);
        let changes = Rc::new(Cell::new(0));
        let _watch = cx.update(|cx| {
            let changes = changes.clone();
            cx.observe_global::<Names>(move |_| changes.set(changes.get() + 1))
        });
        let name = |path: &Path, cx: &mut gpui::TestAppContext| cx.update(|cx| name(path, cx));

        assert_eq!(name(&hunt, cx), "hunt.ts");
        cx.run_until_parked();
        assert_eq!(name(&hunt, cx), "Hunt");
        assert_eq!(name(&plain, cx), "plain.ts");
        cx.run_until_parked();
        assert_eq!(name(&plain, cx), "plain.ts");
        assert_eq!(name(&temp.join("missing.ts"), cx), "missing.ts");
        let seen = changes.get();
        assert!(seen > 0, "observers hear about resolved names");

        fs::write(&hunt, "export const meta = { name: \"Hunt Ultra\" };").unwrap();
        cx.update(|cx| refresh(&hunt, cx));
        assert_eq!(
            name(&hunt, cx),
            "Hunt",
            "the old name stays until the new one is read"
        );
        cx.run_until_parked();
        assert_eq!(name(&hunt, cx), "Hunt Ultra");

        let seen = changes.get();
        cx.update(|cx| learn([(hunt.clone(), "Hunt Ultra".into())], cx));
        assert_eq!(changes.get(), seen, "learning a known name changes nothing");
        cx.update(|cx| learn([(plain.clone(), "Plain".into())], cx));
        assert_eq!(name(&plain, cx), "Plain");
        fs::remove_dir_all(temp).unwrap();
    }
}
