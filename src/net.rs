use std::{fs, io::Write, path::Path};

use reqwest::header::{self, HeaderMap, HeaderValue};
use url::{Host, Url};

pub(crate) const USER_AGENT: &str = "Ruffle/0.7.1 (https://ruffle.rs)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GameHost(String);

impl GameHost {
    pub(crate) fn parse(host: &str) -> Option<Self> {
        let Host::Domain(host) = Host::parse(host).ok()? else {
            return None;
        };
        let host = host.strip_suffix('.').unwrap_or(&host);
        ["aq.com", "artix.com"]
            .into_iter()
            .any(|root| host == root || host.strip_suffix(root).is_some_and(|p| p.ends_with('.')))
            .then(|| Self(host.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn device_id(path: &Path) -> anyhow::Result<HeaderValue> {
    if !path.exists() {
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => writeln!(file, "{id}")?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let text = fs::read_to_string(path)?;
    let id = text.trim();
    anyhow::ensure!(
        id.len() == 32
            && id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Invalid device ID in {}. Delete the file to generate a new ID.",
        path.display()
    );
    Ok(HeaderValue::from_str(id)?)
}

pub(crate) fn identify(url: &Url, headers: &mut HeaderMap, device: &HeaderValue) -> bool {
    headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
    headers.remove(header::ACCEPT_ENCODING);
    headers.remove("x-device-id");
    let game = url.host_str().and_then(GameHost::parse).is_some();
    if game {
        headers.insert("x-device-id", device.clone());
    }
    game
}

pub(crate) async fn dial(host: &GameHost, port: u16) -> std::io::Result<tokio::net::TcpStream> {
    tokio::net::TcpStream::connect((host.as_str(), port)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_stable_and_invalid_files_are_reported() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let path =
            std::env::temp_dir().join(format!("mira-device-{:x}", u64::from_ne_bytes(random)));
        let first = device_id(&path).unwrap();
        assert_eq!(first, device_id(&path).unwrap());
        fs::write(&path, "invalid").unwrap();
        assert!(device_id(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
