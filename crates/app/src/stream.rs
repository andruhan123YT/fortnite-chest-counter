//! Stream overlay: the chest count as a small web page for OBS (Sources > Browser). The
//! app serves it on 127.0.0.1 only, so nothing outside this PC can open it, and nothing
//! goes online.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::ChestApp;

pub const DEFAULT_PORT: u16 = 47321;
pub const DEFAULT_BACKGROUND: u8 = 86;
const PAGE: &str = include_str!("../assets/stream.html");
/// Changes whenever the page does. OBS keeps a browser source loaded for good, so the
/// page reloads itself when `/data` reports a newer one after an app update.
const PAGE_TAG: u64 = fnv1a(PAGE.as_bytes());

const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    hash
}
/// How often the numbers for the page are worked out again.
const REFRESH: Duration = Duration::from_secs(1);

/// What the page shows besides the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Show {
    pub last_match: bool,
    pub today: bool,
    pub squad: bool,
    pub treasures: bool,
    /// The "IN MATCH" badge while a match is being played.
    pub in_match: bool,
}

impl Default for Show {
    fn default() -> Self {
        Self { last_match: true, today: true, squad: true, treasures: true, in_match: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamSettings {
    pub enabled: bool,
    pub port: u16,
    pub show: Show,
    /// How solid the card behind the numbers is, in percent. 0 leaves the numbers alone.
    pub background: u8,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, show: Show::default(), background: DEFAULT_BACKGROUND }
    }
}

pub fn link(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// The numbers the page reads from `/data`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Snapshot {
    pub total: u64,
    pub last_match: u64,
    pub today: u64,
    pub squad: u64,
    pub treasures: u64,
    pub in_match: bool,
    pub show: Show,
    pub background: u8,
}

struct Running {
    port: u16,
    stop: Arc<AtomicBool>,
}

/// The server's state on the UI thread; requests are answered on a thread of its own.
#[derive(Default)]
pub struct Stream {
    snapshot: Arc<Mutex<Snapshot>>,
    running: Option<Running>,
    /// Why the server is not running although it is on, e.g. a port in use.
    pub error: Option<String>,
    /// The port that last failed, so a busy port is not retried every frame.
    failed_port: Option<u16>,
    refreshed: Option<Instant>,
}

impl Stream {
    /// Starts, stops or moves the server to match the settings.
    fn sync(&mut self, enabled: bool, port: u16) {
        let wanted = enabled.then_some(port);
        if self.running.as_ref().map(|r| r.port) == wanted {
            return;
        }
        if let Some(r) = self.running.take() {
            r.stop.store(true, Ordering::Relaxed);
        }
        let Some(port) = wanted else {
            self.error = None;
            self.failed_port = None;
            return;
        };
        if self.failed_port == Some(port) {
            return;
        }
        match serve(port, self.snapshot.clone()) {
            Ok((_, stop)) => {
                self.running = Some(Running { port, stop });
                self.error = None;
                self.failed_port = None;
            }
            Err(e) => {
                self.error = Some(format!("Port {port} is not available ({e}). Pick another one."));
                self.failed_port = Some(port);
            }
        }
    }

    /// Lets a port that failed be tried again, e.g. after the user changed it back.
    pub fn retry(&mut self) {
        self.failed_port = None;
    }

    pub fn running_port(&self) -> Option<u16> {
        self.running.as_ref().map(|r| r.port)
    }
}

/// Called every frame: keeps the server in line with the settings and the page's
/// numbers up to date.
pub fn update(app: &mut ChestApp) {
    let settings = app.settings.stream.clone();
    let on = settings.enabled && app.settings.setup_done;
    app.stream.sync(on, settings.port);
    if !on || app.stream.refreshed.is_some_and(|at| at.elapsed() < REFRESH) {
        return;
    }
    app.stream.refreshed = Some(Instant::now());
    let n = crate::overlay::numbers(app);
    let snapshot = Snapshot {
        total: n.total,
        last_match: n.last_match,
        today: n.today,
        squad: n.squad,
        treasures: n.treasures,
        in_match: n.in_match,
        show: settings.show,
        background: settings.background.min(100),
    };
    *app.stream.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = snapshot;
}

/// Binds `port` on 127.0.0.1 and answers requests until `stop` is set. Returns the port
/// bound (the one asked for, or a free one for 0).
fn serve(port: u16, snapshot: Arc<Mutex<Snapshot>>) -> std::io::Result<(u16, Arc<AtomicBool>)> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new().name("stream".into()).spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let data = snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let _ = answer(stream, port, &data);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    })?;
    Ok((port, stop))
}

fn answer(mut stream: TcpStream, port: u16, data: &Snapshot) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::new();
    let mut buf = [0u8; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
    }
    stream.write_all(&respond(&String::from_utf8_lossy(&request), port, data))
}

/// The whole HTTP response to a request.
fn respond(request: &str, port: u16, data: &Snapshot) -> Vec<u8> {
    let mut lines = request.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (method, path) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    let host = lines
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("host")).map(|(_, v)| v.trim()));
    // A web page elsewhere could point its own name at 127.0.0.1 to read this; only
    // requests addressed to this PC by name get an answer.
    let local = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| local.iter().any(|l| l.eq_ignore_ascii_case(h))) {
        return reply("403 Forbidden", "text/plain", b"Forbidden");
    }
    if method != "GET" {
        return reply("405 Method Not Allowed", "text/plain", b"Method not allowed");
    }
    match path.split('?').next().unwrap_or_default() {
        "/" | "/overlay" => {
            let page = PAGE.replace("__PAGE_TAG__", &format!("{PAGE_TAG:016x}"));
            reply("200 OK", "text/html; charset=utf-8", page.as_bytes())
        }
        "/data" => {
            let mut json = serde_json::to_value(data).unwrap_or_default();
            json["page"] = format!("{PAGE_TAG:016x}").into();
            let body = serde_json::to_vec(&json).unwrap_or_default();
            reply("200 OK", "application/json", &body)
        }
        _ => reply("404 Not Found", "text/plain", b"Not found"),
    }
}

fn reply(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Snapshot {
        Snapshot {
            total: 3253,
            last_match: 42,
            today: 120,
            squad: 2352,
            treasures: 31,
            in_match: true,
            show: Show::default(),
            background: DEFAULT_BACKGROUND,
        }
    }

    fn body(response: &[u8]) -> String {
        let text = String::from_utf8_lossy(response);
        text.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()).unwrap_or_default()
    }

    #[test]
    fn serves_the_page_and_the_numbers() {
        let page = respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:47321\r\n\r\n", 47321, &data());
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(body(&page).contains("CHESTS"));
        let json = respond("GET /data HTTP/1.1\r\nHost: localhost:47321\r\n\r\n", 47321, &data());
        let value: serde_json::Value = serde_json::from_str(&body(&json)).unwrap();
        assert_eq!(value["total"], 3253);
        assert_eq!(value["show"]["squad"], true);
        // The page knows its own tag, so it can tell when the app serves a newer one.
        let tag = value["page"].as_str().unwrap();
        assert_eq!(tag.len(), 16);
        assert!(body(&page).contains(&format!("const PAGE_TAG = \"{tag}\"")));
        assert!(!body(&page).contains("__PAGE_TAG__"));
    }

    #[test]
    fn answers_only_requests_addressed_to_this_pc() {
        let rebound = respond("GET /data HTTP/1.1\r\nHost: evil.example:47321\r\n\r\n", 47321, &data());
        assert!(rebound.starts_with(b"HTTP/1.1 403"));
        let no_host = respond("GET /data HTTP/1.1\r\n\r\n", 47321, &data());
        assert!(no_host.starts_with(b"HTTP/1.1 403"));
        let other = respond("GET /secret HTTP/1.1\r\nHost: 127.0.0.1:47321\r\n\r\n", 47321, &data());
        assert!(other.starts_with(b"HTTP/1.1 404"));
        let post = respond("POST /data HTTP/1.1\r\nHost: 127.0.0.1:47321\r\n\r\n", 47321, &data());
        assert!(post.starts_with(b"HTTP/1.1 405"));
    }

    #[test]
    fn a_running_server_answers_over_tcp() {
        let shared = Arc::new(Mutex::new(data()));
        let (port, stop) = serve(0, shared).unwrap();
        let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(conn, "GET /data HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
        let mut response = Vec::new();
        conn.read_to_end(&mut response).unwrap();
        assert!(body(&response).contains("\"last_match\":42"));
        stop.store(true, Ordering::Relaxed);
    }
}
