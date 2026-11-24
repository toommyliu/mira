use std::{
    fs, io,
    path::{Path, PathBuf},
};

const FOLDER: &str = "shared-objects";
const SEEDING: &str = ".shared-objects";

pub(crate) fn seed(data: &Path) -> PathBuf {
    data.join(FOLDER)
}

pub(crate) fn of_character(seed: &Path, character: &Path) -> io::Result<PathBuf> {
    let folder = character.join(FOLDER);
    if folder.try_exists()? {
        return Ok(folder);
    }
    let seeding = character.join(SEEDING);
    remove(&seeding)?;
    copy(seed, &seeding)?;
    fs::rename(&seeding, &folder)?;
    Ok(folder)
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let entries = match fs::read_dir(from) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy(&entry.path(), &to.join(name))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(name))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard::scratch;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_character_starts_from_the_shared_settings_once() {
        let data = scratch("shared-objects-seed");
        let seed = seed(&data);
        let options = seed.join("localhost/AQLite_Data.sol");
        write(&options, "keybinds");
        write(&seed.join(".DS_Store"), "finder");
        let alice = data.join("characters/alice");
        write(
            &alice.join(".shared-objects/localhost/stale.sol"),
            "interrupted",
        );

        let folder = of_character(&seed, &alice).unwrap();
        assert_eq!(read(&folder.join("localhost/AQLite_Data.sol")), "keybinds");
        assert_eq!(names(&folder), ["localhost"]);
        assert_eq!(names(&folder.join("localhost")), ["AQLite_Data.sol"]);
        assert_eq!(names(&alice), ["shared-objects"]);

        write(&options, "changed later");
        write(&folder.join("localhost/AQLite_Data.sol"), "alice's own");
        let again = of_character(&seed, &alice).unwrap();
        assert_eq!(again, folder);
        assert_eq!(
            read(&again.join("localhost/AQLite_Data.sol")),
            "alice's own"
        );

        fs::remove_dir_all(&seed).unwrap();
        let bob = of_character(&seed, &data.join("characters/bob")).unwrap();
        assert!(names(&bob).is_empty());
        fs::remove_dir_all(data).unwrap();
    }
}
