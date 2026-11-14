use std::{fs, io::Write, path::Path, time::Duration};

use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    time::Instant,
};
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

const VERSION_CHECK: &[u8] =
    b"<msg t='sys'><body action='verChk' r='0'><ver v='157' /></body></msg>\0";
const PING_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Server {
    pub(crate) name: String,
    pub(crate) players: u32,
    pub(crate) capacity: u32,
    pub(crate) online: bool,
    pub(crate) members: bool,
    pub(crate) ping: Option<Duration>,
    address: Option<(GameHost, u16)>,
}

pub(crate) fn servers(device: HeaderValue) -> anyhow::Result<Vec<Server>> {
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
        let mut servers = parse_servers(&body)?;
        let mut pings = tokio::task::JoinSet::new();
        for (index, server) in servers.iter().enumerate() {
            if server.online
                && let Some((host, port)) = server.address.clone()
            {
                pings.spawn(async move { (index, ping(&host, port).await) });
            }
        }
        while let Some(Ok((index, ping))) = pings.join_next().await {
            servers[index].ping = ping;
        }
        Ok(servers)
    })
}

fn parse_servers(json: &[u8]) -> serde_json::Result<Vec<Server>> {
    #[derive(Deserialize)]
    struct Row {
        #[serde(rename = "sName")]
        name: String,
        #[serde(rename = "sIP")]
        host: String,
        #[serde(rename = "iPort")]
        port: u16,
        #[serde(rename = "iCount")]
        players: u32,
        #[serde(rename = "iMax")]
        capacity: u32,
        #[serde(rename = "bOnline")]
        online: u8,
        #[serde(rename = "bUpg")]
        members: u8,
    }
    Ok(serde_json::from_slice::<Vec<Row>>(json)?
        .into_iter()
        .filter_map(|row| {
            let name = row.name.trim();
            (!name.is_empty()).then(|| Server {
                name: name.to_owned(),
                players: row.players,
                capacity: row.capacity,
                online: row.online != 0,
                members: row.members != 0,
                ping: None,
                address: GameHost::parse(&row.host).map(|host| (host, row.port)),
            })
        })
        .collect())
}

async fn ping(host: &GameHost, port: u16) -> Option<Duration> {
    let probe = async {
        let mut stream = dial(host, port).await?;
        round_trip(&mut stream).await
    };
    tokio::time::timeout(PING_TIMEOUT, probe).await.ok()?.ok()
}

async fn round_trip(stream: impl AsyncRead + AsyncWrite + Unpin) -> std::io::Result<Duration> {
    let mut stream = BufReader::new(stream);
    let mut reply = Vec::new();
    let mut fastest = Duration::MAX;
    for exchange in 0..3 {
        let sent = Instant::now();
        stream.write_all(VERSION_CHECK).await?;
        reply.clear();
        stream.read_until(0, &mut reply).await?;
        if reply.last() != Some(&0) {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if exchange > 0 {
            fastest = fastest.min(sent.elapsed());
        }
    }
    Ok(fastest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_list_keeps_each_servers_population_in_order() {
        let json = br#"[
            {"sName":"Twilly","sIP":"sock7.aq.com","iPort":5593,"iCount":949,"iMax":1000,"bOnline":1,"bUpg":0},
            {"sName":" ","sIP":"sock7.aq.com","iPort":5594,"iCount":0,"iMax":1000,"bOnline":0,"bUpg":0},
            {"sName":"Sir Ver","sIP":"example.com","iPort":5589,"iCount":12,"iMax":1200,"bOnline":0,"bUpg":1}
        ]"#;
        let servers = parse_servers(json).unwrap();
        let summary: Vec<_> = servers
            .iter()
            .map(|s| (s.name.as_str(), s.players, s.capacity, s.online, s.members))
            .collect();
        assert_eq!(
            summary,
            [
                ("Twilly", 949, 1000, true, false),
                ("Sir Ver", 12, 1200, false, true)
            ]
        );
        assert_eq!(
            servers[0].address,
            Some((GameHost::parse("sock7.aq.com").unwrap(), 5593))
        );
        assert_eq!(servers[1].address, None);
        assert!(parse_servers(b"<html>").is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn ping_skips_the_first_exchange_and_keeps_the_fastest_of_the_rest() {
        async fn serve(mut server: tokio::io::DuplexStream, delays: &[u64]) {
            let mut server = BufReader::new(&mut server);
            let mut request = Vec::new();
            for &delay in delays {
                request.clear();
                server.read_until(0, &mut request).await.unwrap();
                assert_eq!(request, VERSION_CHECK);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                server
                    .write_all(b"<msg t='sys'><body action='apiOK' r='0'></body></msg>\0")
                    .await
                    .unwrap();
            }
        }

        let (client, server) = tokio::io::duplex(1024);
        let (rtt, ()) = tokio::join!(round_trip(client), serve(server, &[300, 80, 50]));
        assert_eq!(rtt.unwrap(), Duration::from_millis(50));

        let (client, server) = tokio::io::duplex(1024);
        let (rtt, ()) = tokio::join!(round_trip(client), serve(server, &[300]));
        assert!(rtt.is_err());
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
