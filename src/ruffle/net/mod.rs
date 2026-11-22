mod http;

use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::Instrument as _;
use url::Url;

use crate::net::GameHost;

pub(crate) struct Timeouts {
    pub connect: Duration,
    pub read: Duration,
    pub request: Duration,
}

const TIMEOUTS: Timeouts = Timeouts {
    connect: Duration::from_secs(30),
    read: Duration::from_secs(60),
    request: Duration::from_secs(300),
};

pub(crate) struct Net {
    rt: tokio::runtime::Handle,
    http: reqwest::Client,
    cookies: std::sync::Arc<reqwest::cookie::Jar>,
    deadline: Duration,
    device: reqwest::header::HeaderValue,
    tasks: Mutex<Vec<tokio::task::AbortHandle>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Post,
}

pub(crate) struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<(Vec<u8>, String)>,
}

pub(crate) struct HttpResponse {
    pub url: Url,
    pub status: u16,
    pub content_type: Option<String>,
    pub length: Option<u64>,
    pub body: HttpBody,
}

#[derive(Clone)]
pub(crate) struct HttpBody {
    chunks: async_channel::Receiver<Vec<u8>>,
    failure: Arc<OnceLock<NetError>>,
}

impl HttpBody {
    pub(crate) async fn next_chunk(&self) -> Result<Option<Vec<u8>>, NetError> {
        match self.chunks.recv().await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(_) => match self.failure.get() {
                Some(error) => Err(error.clone()),
                None => Ok(None),
            },
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub(crate) enum NetError {
    #[error("{0}")]
    Http(String),
    #[error("the request timed out")]
    TimedOut,
    #[error("the net runtime is gone")]
    Shutdown,
}

impl From<reqwest::Error> for NetError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::TimedOut
        } else {
            Self::Http(error.to_string())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SocketEvent {
    Connected,
    Failed,
    TimedOut,
    Data(Vec<u8>),
    Closed,
}

impl Net {
    pub(crate) fn new(
        rt: tokio::runtime::Handle,
        device: reqwest::header::HeaderValue,
    ) -> Result<Net, NetError> {
        Self::build(rt, TIMEOUTS, reqwest::Client::builder(), device)
    }

    fn build(
        rt: tokio::runtime::Handle,
        timeouts: Timeouts,
        builder: reqwest::ClientBuilder,
        device: reqwest::header::HeaderValue,
    ) -> Result<Net, NetError> {
        let http = builder
            .redirect(reqwest::redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(timeouts.connect)
            .read_timeout(timeouts.read)
            .build()?;
        Ok(Net {
            rt,
            http,
            cookies: Default::default(),
            deadline: timeouts.request,
            device,
            tasks: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn connect(
        &self,
        host: String,
        port: u16,
        timeout: Duration,
        outgoing: async_channel::Receiver<Vec<u8>>,
        mut deliver: impl FnMut(SocketEvent) -> bool + Send + 'static,
    ) {
        let Some(host) = GameHost::parse(&host) else {
            tracing::warn!(%host, port, "socket host rejected");
            deliver(SocketEvent::Failed);
            return;
        };
        self.spawn(async move {
            let connecting = async { crate::net::dial(&host, port).await };
            let stream = match tokio::time::timeout(timeout, connecting).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    tracing::warn!(host = %host.as_str(), port, %error, "socket connection failed");
                    deliver(SocketEvent::Failed);
                    return;
                }
                Err(_) => {
                    deliver(SocketEvent::TimedOut);
                    return;
                }
            };
            if !deliver(SocketEvent::Connected) {
                return;
            }
            let (mut read, mut write) = stream.into_split();
            let reading = async {
                let mut bytes = [0; 4096];
                loop {
                    match read.read(&mut bytes).await {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            if !deliver(SocketEvent::Data(bytes[..count].to_vec())) {
                                break;
                            }
                        }
                    }
                }
            };
            let writing = async {
                while let Ok(bytes) = outgoing.recv().await {
                    if write.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                let _ = write.shutdown().await;
            };
            tokio::select! { _ = reading => {}, _ = writing => {} }
            deliver(SocketEvent::Closed);
        });
    }

    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        let task = self.rt.spawn(task.in_current_span());
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(task.abort_handle());
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().unwrap().drain(..) {
            task.abort();
        }
    }
}
