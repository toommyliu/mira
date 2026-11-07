use std::borrow::Cow;
use std::time::Duration;
use std::{cell::RefCell, rc::Rc};

use async_channel::{Receiver, Sender};
use ruffle_core::backend::navigator::{
    ErrorResponse, NavigationMethod, NavigatorBackend, OwnedFuture, Request, SuccessResponse,
    get_encoding,
};
use ruffle_core::indexmap::IndexMap;
use ruffle_core::loader::Error;
use ruffle_core::socket::{ConnectionState, SocketAction, SocketHandle};
use url::{ParseError, Url};

use crate::ruffle::net::{HttpBody, HttpRequest, HttpResponse, Method, Net, SocketEvent};
use crate::ruffle::{Inbox, Msg};

pub(crate) type Tasks = Rc<RefCell<Vec<async_task::Task<()>>>>;

pub(crate) fn spawn(
    inbox: &Inbox,
    future: impl Future<Output = ()> + 'static,
) -> async_task::Task<()> {
    let inbox = inbox.clone();
    let (runnable, task) =
        async_task::spawn_local(future, move |runnable| inbox.post(Msg::Poll(runnable)));
    runnable.schedule();
    task
}

pub(crate) fn spawn_local(
    inbox: &Inbox,
    tasks: &Tasks,
    future: impl Future<Output = ()> + 'static,
) {
    let task = spawn(inbox, future);
    let mut tasks = tasks.borrow_mut();
    tasks.retain(|task| !task.is_finished());
    tasks.push(task);
}

pub(crate) struct Navigator {
    net: Net,
    inbox: Inbox,
    tasks: Tasks,
}

impl Navigator {
    pub(crate) fn new(net: Net, inbox: Inbox, tasks: Tasks) -> Navigator {
        Navigator { net, inbox, tasks }
    }
}

impl NavigatorBackend for Navigator {
    fn navigate_to_url(
        &self,
        url: &str,
        _target: &str,
        _vars_method: Option<(NavigationMethod, IndexMap<String, String>)>,
    ) {
        tracing::info!(url, "link not opened");
    }

    fn fetch(&self, request: Request) -> OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse> {
        let url = match self.resolve_url(request.url()) {
            Ok(url) => url,
            Err(error) => {
                return Box::pin(async move {
                    Err(ErrorResponse {
                        url: request.url().to_owned(),
                        error: Error::FetchError(error.to_string()),
                    })
                });
            }
        };
        let response = self.net.fetch(HttpRequest {
            method: match request.method() {
                NavigationMethod::Get => Method::Get,
                NavigationMethod::Post => Method::Post,
            },
            url: url.clone(),
            headers: request
                .headers()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            body: request.body().clone(),
        });
        Box::pin(async move {
            let head = response.await.map_err(|error| ErrorResponse {
                url: url.to_string(),
                error: Error::FetchError(error.to_string()),
            })?;
            if !(200..300).contains(&head.status) {
                return Err(ErrorResponse {
                    url: head.url.to_string(),
                    error: Error::HttpNotOk(
                        "HTTP request failed".into(),
                        head.status,
                        head.url != url,
                        head.length.unwrap_or(0),
                    ),
                });
            }
            Ok(Box::new(Response::new(&url, head)) as Box<dyn SuccessResponse>)
        })
    }

    fn resolve_url(&self, url: &str) -> Result<Url, ParseError> {
        Url::parse(url)
    }

    fn spawn_future(&mut self, future: OwnedFuture<(), Error>) {
        spawn_local(&self.inbox, &self.tasks, async move {
            if let Err(error) = future.await {
                tracing::error!(%error, "player task failed");
            }
        });
    }

    fn pre_process_url(&self, url: Url) -> Url {
        url
    }

    fn connect_socket(
        &mut self,
        host: String,
        port: u16,
        timeout: Duration,
        handle: SocketHandle,
        receiver: Receiver<Vec<u8>>,
        sender: Sender<SocketAction>,
    ) {
        let inbox = self.inbox.clone();
        self.net
            .connect(host, port, timeout, receiver, move |event| {
                let action = match event {
                    SocketEvent::Connected => {
                        SocketAction::Connect(handle, ConnectionState::Connected)
                    }
                    SocketEvent::Failed => SocketAction::Connect(handle, ConnectionState::Failed),
                    SocketEvent::TimedOut => {
                        SocketAction::Connect(handle, ConnectionState::TimedOut)
                    }
                    SocketEvent::Data(bytes) => SocketAction::Data(handle, bytes),
                    SocketEvent::Closed => SocketAction::Close(handle),
                };
                let delivered = sender.try_send(action).is_ok();
                inbox.post(Msg::Wake);
                delivered
            });
    }
}

struct Response {
    url: String,
    status: u16,
    redirected: bool,
    encoding: Option<&'static encoding_rs::Encoding>,
    length: Option<u64>,
    body: HttpBody,
}

impl Response {
    fn new(requested: &Url, response: HttpResponse) -> Response {
        Response {
            redirected: response.url != *requested,
            url: response.url.to_string(),
            status: response.status,
            encoding: response.content_type.as_deref().and_then(get_encoding),
            length: response.length,
            body: response.body,
        }
    }
}

impl SuccessResponse for Response {
    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }
    fn set_url(&mut self, url: String) {
        self.url = url;
    }
    fn body(self: Box<Self>) -> OwnedFuture<Vec<u8>, Error> {
        Box::pin(async move {
            let mut body = Vec::new();
            while let Some(chunk) = self
                .body
                .next_chunk()
                .await
                .map_err(|error| Error::FetchError(error.to_string()))?
            {
                body.extend(chunk);
            }
            Ok(body)
        })
    }
    fn text_encoding(&self) -> Option<&'static encoding_rs::Encoding> {
        self.encoding
    }
    fn status(&self) -> u16 {
        self.status
    }
    fn redirected(&self) -> bool {
        self.redirected
    }
    fn next_chunk(&mut self) -> OwnedFuture<Option<Vec<u8>>, Error> {
        let body = self.body.clone();
        Box::pin(async move {
            body.next_chunk()
                .await
                .map_err(|error| Error::FetchError(error.to_string()))
        })
    }
    fn expected_length(&self) -> Result<Option<u64>, Error> {
        Ok(self.length)
    }
}
