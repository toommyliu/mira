use std::{fs, io::Write, path::Path, time::Duration};

use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use url::{Host, Url};

pub(crate) const USER_AGENT: &str = "Ruffle/0.7.1 (https://ruffle.rs)";
const SERVERS: &str = "https://game.aq.com/game/api/data/servers";

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

pub(crate) fn servers(device: HeaderValue) -> anyhow::Result<Vec<String>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let url = Url::parse(SERVERS)?;
        let mut headers = HeaderMap::new();
        identify(&url, &mut headers, &device);
        let body = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?
            .get(url)
            .headers(headers)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(server_names(&body)?)
    })
}

fn server_names(json: &[u8]) -> serde_json::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Row {
        #[serde(rename = "sName")]
        name: String,
    }
    Ok(serde_json::from_slice::<Vec<Row>>(json)?
        .into_iter()
        .map(|row| row.name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_list_names_every_server_in_order() {
        let json = br#"[
            {"sName":"Twilly","iCount":949,"iMax":1000,"bOnline":1,"bUpg":0},
            {"sName":" ","iCount":0,"iMax":1000,"bOnline":0,"bUpg":0},
            {"sName":"Yorumi","iCount":12,"iMax":500,"bOnline":1,"bUpg":1}
        ]"#;
        assert_eq!(server_names(json).unwrap(), ["Twilly", "Yorumi"]);
        assert!(server_names(b"<html>").is_err());
    }

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
