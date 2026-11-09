use std::{fs, io, path::PathBuf};

pub(super) struct BoolPreference {
    path: PathBuf,
}

impl BoolPreference {
    pub(super) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(super) fn load(&self) -> io::Result<bool> {
        match fs::read_to_string(&self.path) {
            Ok(value) => value
                .trim()
                .parse()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn save(&self, enabled: bool) -> io::Result<()> {
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, if enabled { "true\n" } else { "false\n" })?;
        fs::rename(temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_preferences_default_off_and_survive_restarts() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        for name in ["smooth-motion", "show-stats"] {
            let path =
                std::env::temp_dir().join(format!("mira-{name}-{:x}", u64::from_ne_bytes(random)));
            let preferences = BoolPreference::new(path.clone());
            assert!(!preferences.load().unwrap());
            for enabled in [true, false] {
                preferences.save(enabled).unwrap();
                assert_eq!(BoolPreference::new(path.clone()).load().unwrap(), enabled);
            }
            fs::write(&path, "invalid").unwrap();
            assert_eq!(
                preferences.load().unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            fs::remove_file(path).unwrap();
        }
    }
}
