use reqwest::cookie::CookieStore;
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};

use super::*;

const REDIRECT_LIMIT: usize = 10;

fn http_url(url: &Url) -> Result<(), NetError> {
    if matches!(url.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(NetError::Http(
            "only HTTP and HTTPS URLs are allowed".into(),
        ))
    }
}

fn headers(values: Vec<(String, String)>) -> Result<HeaderMap, NetError> {
    let mut headers = HeaderMap::new();
    for (name, value) in values {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|e| NetError::Http(e.to_string()))?;
        let value = HeaderValue::from_str(&value).map_err(|e| NetError::Http(e.to_string()))?;
        headers.append(name, value);
    }
    Ok(headers)
}

impl Net {
    pub(crate) fn fetch(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, NetError>> + 'static {
        let http = self.http.clone();
        let cookies = self.cookies.clone();
        let device = self.device.clone();
        let deadline = tokio::time::Instant::now() + self.deadline;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.spawn(async move {
            let mut response =
                match tokio::time::timeout_at(deadline, send(&http, &cookies, &device, request))
                    .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                    Err(_) => {
                        let _ = tx.send(Err(NetError::TimedOut));
                        return;
                    }
                };
            let (chunks_tx, chunks_rx) = async_channel::bounded(16);
            let failure = Arc::new(OnceLock::new());
            let head = HttpResponse {
                url: response.url().clone(),
                status: response.status().as_u16(),
                content_type: response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
                length: response.content_length(),
                body: HttpBody {
                    chunks: chunks_rx,
                    failure: failure.clone(),
                },
            };
            if tx.send(Ok(head)).is_err() {
                return;
            }
            loop {
                let chunk = tokio::time::timeout_at(deadline, response.chunk()).await;
                let chunk = match chunk {
                    Ok(Ok(Some(chunk))) => chunk.to_vec(),
                    Ok(Ok(None)) => break,
                    Ok(Err(error)) => {
                        let _ = failure.set(error.into());
                        break;
                    }
                    Err(_) => {
                        let _ = failure.set(NetError::TimedOut);
                        break;
                    }
                };
                match tokio::time::timeout_at(deadline, chunks_tx.send(chunk)).await {
                    Ok(Ok(())) => {}
                    Ok(_) => break,
                    Err(_) => {
                        let _ = failure.set(NetError::TimedOut);
                        break;
                    }
                }
            }
        });
        async move { rx.await.unwrap_or(Err(NetError::Shutdown)) }
    }
}

async fn send(
    http: &reqwest::Client,
    cookies: &reqwest::cookie::Jar,
    device: &HeaderValue,
    request: HttpRequest,
) -> Result<reqwest::Response, NetError> {
    let HttpRequest {
        mut method,
        mut url,
        headers: values,
        mut body,
    } = request;
    let mut headers = headers(values)?;
    for hop in 0..=REDIRECT_LIMIT {
        http_url(&url)?;
        let mut hop_headers = headers.clone();
        let game = crate::net::identify(&url, &mut hop_headers, device);
        if !game {
            hop_headers.remove(header::COOKIE);
        } else if !hop_headers.contains_key(header::COOKIE)
            && let Some(value) = cookies.cookies(&url)
        {
            hop_headers.insert(header::COOKIE, value);
        }
        let mut builder = http
            .request(
                match method {
                    Method::Get => reqwest::Method::GET,
                    Method::Post => reqwest::Method::POST,
                },
                url.clone(),
            )
            .headers(hop_headers);
        if method == Method::Get {
            builder = builder.header(header::CONTENT_TYPE, "");
        }
        if let Some((bytes, mime)) = &body {
            builder = builder
                .header(header::CONTENT_TYPE, mime)
                .body(bytes.clone());
        }
        let response = builder.send().await?;
        if game {
            cookies.set_cookies(
                &mut response.headers().get_all(header::SET_COOKIE).iter(),
                &url,
            );
        }
        let status = response.status().as_u16();
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let Some(location) = response.headers().get(header::LOCATION) else {
            return Ok(response);
        };
        if hop == REDIRECT_LIMIT {
            return Err(NetError::Http("too many HTTP redirects".into()));
        }
        let location = location
            .to_str()
            .map_err(|e| NetError::Http(e.to_string()))?;
        let next = url
            .join(location)
            .map_err(|e| NetError::Http(e.to_string()))?;
        http_url(&next)?;
        if next.origin() != url.origin() {
            for name in [
                header::AUTHORIZATION,
                header::PROXY_AUTHORIZATION,
                header::COOKIE,
                header::HOST,
                header::ORIGIN,
                header::REFERER,
            ] {
                headers.remove(name);
            }
        }
        if status == 303 || (matches!(status, 301 | 302) && method == Method::Post) {
            method = Method::Get;
            body = None;
            for name in [
                header::CONTENT_TYPE,
                header::CONTENT_LENGTH,
                header::CONTENT_ENCODING,
                header::CONTENT_LANGUAGE,
                header::CONTENT_LOCATION,
                header::TRANSFER_ENCODING,
            ] {
                headers.remove(name);
            }
        }
        url = next;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
    };

    fn receive(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            request.push_str(&line.to_ascii_lowercase());
            if line == "\r\n" {
                break;
            }
        }
        request
    }

    #[tokio::test]
    async fn aqw_requests_use_player_headers_and_keep_game_cookies() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(receive(&mut stream));
                let headers = if index == 0 {
                    "Set-Cookie: __cflb=fixture; Path=/\r\n"
                } else {
                    ""
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\n{headers}Content-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
            requests
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .resolve("game.aq.com", address)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let cookies = reqwest::cookie::Jar::default();
        let device = HeaderValue::from_static("0123456789abcdef0123456789abcdef");
        for method in [Method::Get, Method::Post] {
            send(
                &client,
                &cookies,
                &device,
                HttpRequest {
                    method,
                    url: Url::parse(&format!(
                        "http://game.aq.com:{}/game/api/test",
                        address.port()
                    ))
                    .unwrap(),
                    headers: if method == Method::Post {
                        vec![("artixmode".into(), "launcher".into())]
                    } else {
                        vec![]
                    },
                    body: (method == Method::Post).then(|| {
                        (
                            b"user=fixture&pass=fixture&option=1".to_vec(),
                            "application/x-www-form-urlencoded".into(),
                        )
                    }),
                },
            )
            .await
            .unwrap();
        }
        let requests = server.join().unwrap();
        for request in &requests {
            assert!(request.contains("user-agent: ruffle/0.7.1 (https://ruffle.rs)\r\n"));
            assert!(request.contains("accept: */*\r\n"));
            assert!(request.contains("x-device-id: 0123456789abcdef0123456789abcdef\r\n"));
            for absent in ["origin:", "referer:", "accept-encoding:"] {
                assert!(!request.contains(absent), "Unexpected header: {absent}");
            }
        }
        assert!(!requests[0].contains("artixmode:"));
        assert!(requests[1].contains("artixmode: launcher\r\n"));
        assert!(requests[0].contains("content-type: \r\n"));
        assert!(requests[1].contains("content-type: application/x-www-form-urlencoded\r\n"));
        assert!(requests[1].contains("cookie: __cflb=fixture\r\n"));
    }

    #[test]
    fn device_id_is_removed_when_a_request_leaves_game_hosts() {
        let device = HeaderValue::from_static("0123456789abcdef0123456789abcdef");
        let mut headers = HeaderMap::new();
        assert!(crate::net::identify(
            &Url::parse("https://game.aq.com/").unwrap(),
            &mut headers,
            &device
        ));
        assert!(headers.contains_key("x-device-id"));
        assert!(!crate::net::identify(
            &Url::parse("https://aq.com.example.org/").unwrap(),
            &mut headers,
            &device
        ));
        assert!(!headers.contains_key("x-device-id"));
    }
}
