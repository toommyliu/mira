use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

#[derive(Clone)]
pub(crate) struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    pub(crate) fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub(crate) fn json(body: impl Into<String>) -> Self {
        Self {
            body: body.into().into_bytes(),
            ..Self::status(200)
        }
        .header("content-type", "application/json")
    }

    pub(crate) fn bytes(body: Vec<u8>) -> Self {
        Self {
            body,
            ..Self::status(200)
        }
    }

    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Default)]
struct Shared {
    routes: HashMap<String, Reply>,
    requests: Vec<String>,
}

pub(crate) struct Fake {
    address: SocketAddr,
    shared: Arc<Mutex<Shared>>,
}

impl Fake {
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let serving = shared.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = serving.clone();
                thread::spawn(move || answer(stream, &shared));
            }
        });
        Self { address, shared }
    }

    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(crate) fn route(&self, target: &str, reply: Reply) {
        self.shared
            .lock()
            .unwrap()
            .routes
            .insert(target.to_owned(), reply);
    }

    pub(crate) fn requests(&self) -> Vec<String> {
        self.shared.lock().unwrap().requests.clone()
    }
}

fn answer(mut stream: TcpStream, shared: &Mutex<Shared>) {
    let mut received = Vec::new();
    let mut buffer = [0; 4096];
    while !received.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => received.extend_from_slice(&buffer[..read]),
        }
    }
    let head = String::from_utf8_lossy(&received).into_owned();
    let target = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_owned();
    let reply = {
        let mut shared = shared.lock().unwrap();
        shared.requests.push(head);
        shared
            .routes
            .get(&target)
            .cloned()
            .unwrap_or_else(|| Reply::status(404))
    };
    let mut response = format!("HTTP/1.1 {} Fake\r\n", reply.status);
    for (name, value) in &reply.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        reply.body.len()
    ));
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&reply.body);
}
