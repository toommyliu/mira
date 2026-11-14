use std::{cell::OnceCell, collections::BTreeMap, error::Error as _, fmt::Write, time::Duration};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: usize = 8 * 1024 * 1024;
const USER_AGENT: &str = concat!("Mira/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub(super) url: String,
    #[serde(default)]
    pub(super) method: Method,
    #[serde(default)]
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Option<String>,
    pub(super) timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(super) enum Method {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl From<Method> for reqwest::Method {
    fn from(method: Method) -> Self {
        match method {
            Method::Get => Self::GET,
            Method::Head => Self::HEAD,
            Method::Post => Self::POST,
            Method::Put => Self::PUT,
            Method::Patch => Self::PATCH,
            Method::Delete => Self::DELETE,
            Method::Options => Self::OPTIONS,
        }
    }
}

#[derive(Default)]
pub(crate) struct Client(OnceCell<reqwest::Client>);

impl Client {
    fn get(&self) -> Result<&reqwest::Client, Failure> {
        if let Some(client) = self.0.get() {
            return Ok(client);
        }
        let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
        Ok(self.0.get_or_init(|| client))
    }
}

#[derive(Debug, thiserror::Error)]
enum Failure {
    #[error("{0}")]
    Invalid(String),
    #[error("the request timed out")]
    Timeout,
    #[error("the response is larger than 8 MiB")]
    TooLarge,
    #[error("{0}")]
    Network(String),
}

impl From<reqwest::Error> for Failure {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            return Self::Timeout;
        }
        let error = error.without_url();
        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            let _ = write!(message, ": {cause}");
            source = cause.source();
        }
        Self::Network(message)
    }
}

impl Failure {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalidRequest",
            Self::Timeout => "timeout",
            Self::TooLarge => "tooLarge",
            Self::Network(_) => "network",
        }
    }
}

pub(super) async fn serve(client: &Client, request: Request) -> Value {
    send(client, request).await.unwrap_or_else(|failure| {
        json!({ "ok": false, "reason": failure.reason(), "message": failure.to_string() })
    })
}

async fn send(client: &Client, request: Request) -> Result<Value, Failure> {
    let url = Url::parse(&request.url)
        .map_err(|error| Failure::Invalid(format!("{} isn't a URL: {error}", request.url)))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Failure::Invalid(format!(
            "{} isn't an http or https URL",
            request.url
        )));
    }
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        let invalid = || Failure::Invalid(format!("{name}: {value} isn't a valid header"));
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?,
            HeaderValue::from_str(value).map_err(|_| invalid())?,
        );
    }
    let mut builder = client
        .get()?
        .request(request.method.into(), url)
        .headers(headers)
        .timeout(
            request
                .timeout_ms
                .map_or(DEFAULT_TIMEOUT, Duration::from_millis),
        );
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let mut response = builder.send().await?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(Failure::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY {
            return Err(Failure::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    let mut headers = Map::new();
    for (name, value) in response.headers() {
        let value = String::from_utf8_lossy(value.as_bytes());
        headers
            .entry(name.as_str())
            .and_modify(|joined| {
                if let Value::String(joined) = joined {
                    joined.push_str(", ");
                    joined.push_str(&value);
                }
            })
            .or_insert_with(|| Value::from(value.as_ref()));
    }
    Ok(json!({
        "ok": true,
        "status": response.status().as_u16(),
        "url": response.url().as_str(),
        "headers": headers,
        "text": String::from_utf8_lossy(&body),
    }))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener},
        thread,
    };

    use super::*;

    fn serve_once(response: impl Into<Vec<u8>>) -> (SocketAddr, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = response.into();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut buffer = [0; 4096];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                received.extend_from_slice(&buffer[..read]);
            }
            let text = String::from_utf8(received).unwrap();
            let length = text
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .map_or(0, |length| length.parse::<usize>().unwrap());
            let head = text.find("\r\n\r\n").unwrap() + 4;
            let mut body = text.as_bytes()[head..].to_vec();
            while body.len() < length {
                let read = stream.read(&mut buffer).unwrap();
                body.extend_from_slice(&buffer[..read]);
            }
            let _ = stream.write_all(&response);
            format!("{}{}", &text[..head], String::from_utf8(body).unwrap())
        });
        (address, server)
    }

    fn request(url: String) -> Request {
        Request {
            url,
            method: Method::Get,
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: None,
        }
    }

    #[tokio::test]
    async fn a_response_returns_its_status_headers_and_text() {
        let (address, server) = serve_once(
            "HTTP/1.1 404 Not Found\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nContent-Length: 5\r\nConnection: close\r\n\r\nmissy",
        );
        let reply = serve(
            &Client::default(),
            Request {
                method: Method::Post,
                headers: BTreeMap::from([("X-Token".into(), "secret".into())]),
                body: Some("{\"a\":1}".into()),
                ..request(format!("http://{address}/path?q=1"))
            },
        )
        .await;
        assert_eq!(
            reply,
            json!({
                "ok": true,
                "status": 404,
                "url": format!("http://{address}/path?q=1"),
                "headers": {
                    "set-cookie": "a=1, b=2",
                    "content-length": "5",
                    "connection": "close",
                },
                "text": "missy",
            })
        );
        let received = server.join().unwrap();
        assert!(received.starts_with("POST /path?q=1 HTTP/1.1\r\n"));
        assert!(received.contains(&format!("user-agent: {USER_AGENT}\r\n")));
        assert!(received.contains("x-token: secret\r\n"));
        assert!(received.ends_with("\r\n\r\n{\"a\":1}"));
    }

    #[tokio::test]
    async fn failures_name_their_reason() {
        let reason =
            async |request: Request| serve(&Client::default(), request).await["reason"].clone();
        assert_eq!(reason(request("not a url".into())).await, "invalidRequest");
        assert_eq!(
            reason(request("file:///etc/hosts".into())).await,
            "invalidRequest"
        );
        assert_eq!(
            reason(Request {
                headers: BTreeMap::from([("bad header".into(), "x".into())]),
                ..request("http://127.0.0.1/".into())
            })
            .await,
            "invalidRequest"
        );

        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        assert_eq!(
            reason(request(format!("http://{closed}/"))).await,
            "network"
        );

        let (address, _server) = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        ));
        assert_eq!(
            reason(request(format!("http://{address}/"))).await,
            "tooLarge"
        );
        let mut streamed = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        streamed.resize(streamed.len() + MAX_BODY + 1, b'a');
        let (address, _server) = serve_once(streamed);
        assert_eq!(
            reason(request(format!("http://{address}/"))).await,
            "tooLarge"
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let silent = listener.local_addr().unwrap();
        assert_eq!(
            reason(Request {
                timeout_ms: Some(50),
                ..request(format!("http://{silent}/"))
            })
            .await,
            "timeout"
        );
    }
}
