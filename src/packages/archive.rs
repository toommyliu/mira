use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use super::{PackagePath, RECEIPT};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) download: u64,
    pub(crate) unpacked: u64,
    pub(crate) file: u64,
    pub(crate) files: usize,
}

pub(crate) type Files = BTreeMap<PackagePath, String>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArchiveError {
    #[error("the download isn't a zip file Mira can read")]
    Unreadable,
    #[error("{0} isn't a safe path")]
    Unsafe(String),
    #[error("{0} is a symbolic link, and packages can't contain links")]
    Link(PackagePath),
    #[error("{0} appears more than once")]
    Duplicate(PackagePath),
    #[error("{path} is larger than {} MB", .limit / 1_000_000)]
    FileTooLarge { path: PackagePath, limit: u64 },
    #[error("it unpacks to more than {} MB", .0 / 1_000_000)]
    TooLarge(u64),
    #[error("it has more than {0} files")]
    TooManyFiles(usize),
    #[error("{RECEIPT} is reserved for Mira")]
    Reserved,
    #[error("the folder has no files")]
    Empty,
    #[error("couldn't unpack it: {0}")]
    Io(#[from] io::Error),
}

pub(crate) fn extract(
    zip: &Path,
    folder: &PackagePath,
    into: &Path,
    limits: Limits,
) -> Result<Files, ArchiveError> {
    let mut archive = ZipArchive::new(File::open(zip)?).map_err(|_| ArchiveError::Unreadable)?;
    let mut top: Option<String> = None;
    let mut files = Files::new();
    let mut unpacked = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ArchiveError::Unreadable)?;
        let name = entry.name().to_owned();
        let (first, rest) = name.split_once('/').unwrap_or((&name, ""));
        match &top {
            None => top = Some(first.to_owned()),
            Some(top) if top != first => return Err(ArchiveError::Unreadable),
            Some(_) => {}
        }
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.is_empty() {
            continue;
        }
        let path = PackagePath::parse(rest).ok_or_else(|| ArchiveError::Unsafe(rest.to_owned()))?;
        let Some(path) = path.strip_prefix(folder).filter(|path| !path.is_root()) else {
            continue;
        };
        if entry.is_symlink() {
            return Err(ArchiveError::Link(path));
        }
        let target = into.join(path.as_str());
        if entry.is_dir() {
            fs::create_dir_all(target)?;
            continue;
        }
        if path.as_str() == RECEIPT {
            return Err(ArchiveError::Reserved);
        }
        if files.len() == limits.files {
            return Err(ArchiveError::TooManyFiles(limits.files));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ArchiveError::Duplicate(path));
            }
            opened => opened?,
        };
        let mut hasher = Sha256::new();
        let mut size = 0;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = entry
                .read(&mut buffer)
                .map_err(|_| ArchiveError::Unreadable)?;
            if read == 0 {
                break;
            }
            size += read as u64;
            unpacked += read as u64;
            if size > limits.file {
                return Err(ArchiveError::FileTooLarge {
                    path,
                    limit: limits.file,
                });
            }
            if unpacked > limits.unpacked {
                return Err(ArchiveError::TooLarge(limits.unpacked));
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])?;
        }
        file.sync_all()?;
        files.insert(path, hex::encode(hasher.finalize()));
    }
    if files.is_empty() {
        return Err(ArchiveError::Empty);
    }
    Ok(files)
}

pub(crate) fn hash(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    pub(crate) const ROOMY: Limits = Limits {
        download: 1_000_000,
        unpacked: 1_000_000,
        file: 1_000_000,
        files: 100,
    };

    pub(crate) enum Item<'a> {
        File(&'a str, &'a [u8]),
        Dir(&'a str),
        Link(&'a str, &'a str),
    }

    pub(crate) fn scratch(label: &str) -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder =
            std::env::temp_dir().join(format!("mira-{label}-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    pub(crate) fn write_zip(to: &Path, items: &[Item]) {
        let mut writer = ZipWriter::new(File::create(to).unwrap());
        let options = SimpleFileOptions::default();
        for item in items {
            match item {
                Item::File(name, bytes) => {
                    writer.start_file(*name, options).unwrap();
                    writer.write_all(bytes).unwrap();
                }
                Item::Dir(name) => writer.add_directory(*name, options).unwrap(),
                Item::Link(name, target) => writer.add_symlink(*name, *target, options).unwrap(),
            }
        }
        writer.finish().unwrap();
    }

    fn unpack(
        items: &[Item],
        folder: &str,
        limits: Limits,
    ) -> (PathBuf, Result<Files, ArchiveError>) {
        let scratch = scratch("archive");
        let zip = scratch.join("download.zip");
        write_zip(&zip, items);
        let into = scratch.join("unpacked");
        fs::create_dir(&into).unwrap();
        let result = extract(&zip, &PackagePath::parse(folder).unwrap(), &into, limits);
        (scratch, result)
    }

    fn sha256(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[test]
    fn a_download_unpacks_only_the_package_folder_and_records_each_files_hash() {
        let items = [
            Item::Dir("o-r-abc/"),
            Item::File("o-r-abc/README.md", b"repo"),
            Item::Dir("o-r-abc/pkgs/dailies/"),
            Item::File("o-r-abc/pkgs/dailies/package.json", b"{}"),
            Item::File("o-r-abc/pkgs/dailies/scripts/spin.ts", b"spin"),
            Item::File("o-r-abc/pkgs/dailies-old/scripts/old.ts", b"old"),
        ];
        let (scratch, files) = unpack(&items, "pkgs/dailies", ROOMY);
        let path = |text: &str| PackagePath::parse(text).unwrap();
        assert_eq!(
            files.unwrap(),
            Files::from([
                (path("package.json"), sha256(b"{}")),
                (path("scripts/spin.ts"), sha256(b"spin")),
            ])
        );
        let unpacked = scratch.join("unpacked");
        assert_eq!(fs::read(unpacked.join("scripts/spin.ts")).unwrap(), b"spin");
        assert!(!unpacked.join("README.md").exists());
        assert_eq!(
            hash(&unpacked.join("scripts/spin.ts")).unwrap(),
            sha256(b"spin")
        );
        fs::remove_dir_all(scratch).unwrap();

        let (scratch, files) = unpack(&items, "", ROOMY);
        assert_eq!(files.unwrap().len(), 4);
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn unsafe_or_oversized_downloads_are_refused() {
        let file = |name, bytes| Item::File(name, bytes);
        let cases: Vec<(Vec<Item>, &str, Limits, &str)> = vec![
            (
                vec![file("o-r/../escape.ts", b"x")],
                "",
                ROOMY,
                "../escape.ts isn't a safe path",
            ),
            (
                vec![
                    file("o-r/a.ts", b"x"),
                    Item::Link("o-r/link.ts", "/etc/passwd"),
                ],
                "",
                ROOMY,
                "link.ts is a symbolic link, and packages can't contain links",
            ),
            (
                vec![file("o-r/.mira-install.json", b"{}")],
                "",
                ROOMY,
                ".mira-install.json is reserved for Mira",
            ),
            (
                vec![file("o-r/big.ts", &[0; 2_000_001])],
                "",
                Limits {
                    file: 2_000_000,
                    unpacked: 10_000_000,
                    ..ROOMY
                },
                "big.ts is larger than 2 MB",
            ),
            (
                vec![
                    file("o-r/a.ts", &[0; 1_500_000]),
                    file("o-r/b.ts", &[0; 1_500_000]),
                ],
                "",
                Limits {
                    file: 2_000_000,
                    unpacked: 2_000_000,
                    ..ROOMY
                },
                "it unpacks to more than 2 MB",
            ),
            (
                vec![file("o-r/a.ts", b"a"), file("o-r/b.ts", b"b")],
                "",
                Limits { files: 1, ..ROOMY },
                "it has more than 1 files",
            ),
            (
                vec![file("o-r/a.ts", b"a"), file("other/b.ts", b"b")],
                "",
                ROOMY,
                "the download isn't a zip file Mira can read",
            ),
            (
                vec![file("o-r/a.ts", b"a")],
                "missing",
                ROOMY,
                "the folder has no files",
            ),
        ];
        for (items, folder, limits, message) in cases {
            let (scratch, result) = unpack(&items, folder, limits);
            assert_eq!(result.unwrap_err().to_string(), message);
            fs::remove_dir_all(scratch).unwrap();
        }
        let scratch = scratch("archive");
        fs::write(scratch.join("download.zip"), b"<html>not a zip</html>").unwrap();
        let result = extract(
            &scratch.join("download.zip"),
            &PackagePath::default(),
            &scratch,
            ROOMY,
        );
        assert!(matches!(result, Err(ArchiveError::Unreadable)));
        fs::remove_dir_all(scratch).unwrap();
    }
}
