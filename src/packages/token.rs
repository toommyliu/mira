use std::{fs, io, path::Path};

pub(crate) fn load(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text.trim().to_owned()).filter(|token| !token.is_empty())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn save(path: &Path, token: &str) -> io::Result<()> {
    let token = token.trim();
    if token.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    crate::data_file::write_secret(path, token.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;
    use crate::packages::archive::tests::scratch;

    #[test]
    fn a_saved_token_comes_back_from_a_file_only_the_owner_can_read() {
        let folder = scratch("token");
        let path = folder.join("github-token");
        assert_eq!(load(&path).unwrap(), None);
        save(&path, " github_pat_123 \n").unwrap();
        assert_eq!(load(&path).unwrap(), Some("github_pat_123".into()));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        save(&path, "ghp_456").unwrap();
        assert_eq!(load(&path).unwrap(), Some("ghp_456".into()));
        save(&path, "  ").unwrap();
        assert!(!path.exists());
        assert_eq!(load(&path).unwrap(), None);
        fs::remove_dir_all(folder).unwrap();
    }
}
