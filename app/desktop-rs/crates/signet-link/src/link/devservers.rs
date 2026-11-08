//! Development servers on the computer: listening TCP ports kept if they answer over HTTP (page title read along the way),
//! plus tabs open on a local address. The list goes to connected phones (`servers` message).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;

use super::loopback::{is_loopback_url, loopback_port, to_link_localhost};
use super::protocol::DevServer;

/// Ports that are never a development server.
const IGNORED_PORTS: &[u16] = &[22, 25, 53, 88, 111, 135, 137, 138, 139, 389, 445, 465, 548, 587, 631, 993, 995, 1433, 3306, 3389, 5353, 5432, 5900, 6379, 11211, 27017];
const EPHEMERAL_START: u16 = 49152;
const MAX_SERVERS: usize = 64;
const REFRESH_HTTP_MS: u64 = 30_000;
const REFRESH_OTHER_MS: u64 = 60_000;
const PROBE_MS: u64 = 1000;
const PROBE_BYTES: usize = 16 * 1024;
const PROBE_PARALLEL: usize = 8;
const LIST_TIMEOUT_MS: u64 = 4000;

fn port_of(addr: &str) -> Option<u16> {
    let (_, p) = addr.rsplit_once(':')?;
    if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    p.parse::<u32>().ok().filter(|n| *n > 0 && *n < 65536).map(|n| n as u16)
}

fn unique(ports: impl IntoIterator<Item = Option<u16>>) -> Vec<u16> {
    let mut v: Vec<u16> = ports.into_iter().flatten().collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// macOS: `lsof -nP -iTCP -sTCP:LISTEN`.
pub fn parse_lsof(text: &str) -> Vec<u16> {
    unique(text.lines().map(|l| {
        let i = l.find("TCP")?;
        // `\sTCP\s+(\S+)\s+\(LISTEN\)`
        if i == 0 || !l[..i].ends_with(char::is_whitespace) {
            return None;
        }
        let rest = l[i + 3..].trim_start();
        let (addr, tail) = rest.split_once(char::is_whitespace)?;
        tail.trim_start().starts_with("(LISTEN)").then(|| port_of(addr)).flatten()
    }))
}

/// Linux: `ss -ltnH` (local address in the 4th column).
pub fn parse_ss(text: &str) -> Vec<u16> {
    unique(text.lines().map(|l| port_of(l.split_whitespace().nth(3).unwrap_or(""))))
}

/// Windows: `netstat -ano -p TCP`. "LISTENING" is translated: the remote address `...:0` is recognized instead.
pub fn parse_netstat(text: &str) -> Vec<u16> {
    unique(text.lines().map(|l| {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.first()?.eq_ignore_ascii_case("TCP") && c.get(2)?.ends_with(":0") { port_of(c.get(1)?) } else { None }
    }))
}

/// Linux without `ss`: `/proc/net/tcp` and `tcp6` (state `0A` = listening, port in hexadecimal).
pub fn parse_proc_net(text: &str) -> Vec<u16> {
    unique(text.lines().map(|l| {
        let c: Vec<&str> = l.split_whitespace().collect();
        if *c.get(3)? != "0A" {
            return None;
        }
        let local = c.get(1)?;
        let hex = local.rsplit_once(':')?.1;
        (hex.len() == 4).then(|| u16::from_str_radix(hex, 16).ok()).flatten()
    }))
}

async fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args).kill_on_drop(true);
    #[cfg(windows)]
    {
        c.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(Duration::from_millis(LIST_TIMEOUT_MS), c.output()).await.ok()?.ok()?;
    // `lsof` returns 1 when nothing is listening: the output counts, not the code.
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// TCP ports listening on this machine. Empty if the system command is missing.
pub async fn list_listening_ports() -> Vec<u16> {
    if cfg!(target_os = "macos") {
        return run("/usr/sbin/lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"]).await.map(|t| parse_lsof(&t)).unwrap_or_default();
    }
    if cfg!(windows) {
        return run("netstat", &["-ano", "-p", "TCP"]).await.map(|t| parse_netstat(&t)).unwrap_or_default();
    }
    if let Some(t) = run("ss", &["-ltnH"]).await {
        return parse_ss(&t);
    }
    let mut all = vec![];
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let t = tokio::fs::read_to_string(f).await.unwrap_or_default();
        all.extend(parse_proc_net(&t).into_iter().map(Some));
    }
    unique(all)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// The port answers with an HTML page (a JSON API or an app service does not count).
    pub http: bool,
    pub title: Option<String>,
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'")
}

/// Content of the `<title>` tag on one line, or None.
pub fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = start + lower[start..].find('>')? + 1;
    let close = open_end + lower[open_end..].find("</title>")?;
    // ASCII lowercase keeps byte offsets identical.
    let title = decode_entities(&html[open_end..close]).split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then(|| title.chars().take(200).collect())
}

async fn probe_host(host: &str, port: u16) -> Option<ProbeResult> {
    let work = async {
        let mut s = TcpStream::connect((host, port)).await.ok()?;
        let req = format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\nAccept: text/html,*/*;q=0.5\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.ok()?;
        let mut buf: Vec<u8> = vec![];
        let mut chunk = [0u8; 4096];
        let mut head_end = None;
        loop {
            let n = s.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if head_end.is_none() {
                head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
            }
            if let Some(h) = head_end {
                let body = String::from_utf8_lossy(&buf[h..]).to_lowercase();
                if buf.len() - h >= PROBE_BYTES || body.contains("</title>") {
                    break;
                }
            } else if buf.len() > PROBE_BYTES {
                return None;
            }
        }
        let h = head_end?;
        let head = String::from_utf8_lossy(&buf[..h]).to_string();
        if !head.starts_with("HTTP/") {
            return None;
        }
        let header = |name: &str| -> String {
            head.lines()
                .skip(1)
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                })
                .unwrap_or_default()
        };
        // The macOS AirPlay service answers over HTTP but is not a page.
        if header("server").to_ascii_lowercase().starts_with("airtunes") {
            return None;
        }
        if !header("content-type").to_ascii_lowercase().contains("html") {
            return Some(ProbeResult { http: false, title: None });
        }
        Some(ProbeResult { http: true, title: extract_title(&String::from_utf8_lossy(&buf[h..])) })
    };
    tokio::time::timeout(Duration::from_millis(PROBE_MS), work).await.ok().flatten()
}

/// Requests `/` over HTTP on 127.0.0.1 then ::1 (Vite often listens on ::1 only).
pub async fn probe_port(port: u16) -> ProbeResult {
    for host in ["127.0.0.1", "::1"] {
        if let Some(r) = probe_host(host, port).await {
            return r;
        }
    }
    ProbeResult { http: false, title: None }
}

#[derive(Debug, Clone)]
pub struct TabLike {
    pub url: String,
    pub title: String,
}

type ProbeFn = Box<dyn Fn(u16) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProbeResult> + Send>> + Send + Sync>;
type ListFn = Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<u16>> + Send>> + Send + Sync>;

/// Detects dev servers. `scan()` returns the full list; probe results are kept for a while.
pub struct DevServerScanner {
    cache: HashMap<u16, (u64, ProbeResult)>,
    /// All ports listening at the last sweep, plus those of local tabs: the phone relays them.
    pub ports: Vec<u16>,
    list: ListFn,
    probe: ProbeFn,
}

impl DevServerScanner {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            ports: vec![],
            list: Box::new(|| Box::pin(list_listening_ports())),
            probe: Box::new(|p| Box::pin(probe_port(p))),
        }
    }

    pub fn with_sources(list: ListFn, probe: ProbeFn) -> Self {
        Self { cache: HashMap::new(), ports: vec![], list, probe }
    }

    pub async fn scan(&mut self, own_port: Option<u16>, tabs: &[TabLike], now_ms: u64) -> Vec<DevServer> {
        let ports: Vec<u16> =
            (self.list)().await.into_iter().filter(|p| Some(*p) != own_port && *p < EPHEMERAL_START && !IGNORED_PORTS.contains(p)).collect();
        let stale: Vec<u16> = ports
            .iter()
            .copied()
            .filter(|p| match self.cache.get(p) {
                None => true,
                Some((at, r)) => now_ms.saturating_sub(*at) >= if r.http { REFRESH_HTTP_MS } else { REFRESH_OTHER_MS },
            })
            .collect();
        for group in stale.chunks(PROBE_PARALLEL) {
            let results = futures_util::future::join_all(group.iter().map(|p| (self.probe)(*p))).await;
            for (p, r) in group.iter().zip(results) {
                self.cache.insert(*p, (now_ms, r));
            }
        }
        let set: HashSet<u16> = ports.iter().copied().collect();
        self.cache.retain(|p, _| set.contains(p));
        let tab_ports = tabs.iter().filter(|t| is_loopback_url(&t.url)).filter_map(|t| loopback_port(&t.url));
        let mut all: Vec<u16> = ports.iter().copied().chain(tab_ports).filter(|p| Some(*p) != own_port).collect();
        all.sort_unstable();
        all.dedup();
        self.ports = all;
        let listening: Vec<(u16, Option<String>)> =
            ports.iter().filter_map(|p| self.cache.get(p).filter(|(_, r)| r.http).map(|(_, r)| (*p, r.title.clone()))).collect();
        combine(own_port, tabs, &listening)
    }
}

impl Default for DevServerScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Local tabs first (exact address, tab title), then ports that listen without a tab.
fn combine(own: Option<u16>, tabs: &[TabLike], listening: &[(u16, Option<String>)]) -> Vec<DevServer> {
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut tab_ports = HashSet::new();
    for tab in tabs {
        let Some(port) = loopback_port(&tab.url) else { continue };
        if Some(port) == own || !is_loopback_url(&tab.url) {
            continue;
        }
        let url = to_link_localhost(&tab.url);
        if !seen.insert(url.clone()) {
            continue;
        }
        tab_ports.insert(port);
        let title: String = tab.title.trim().chars().take(200).collect();
        out.push(DevServer { url, port, title: (!title.is_empty()).then_some(title), from_tab: Some(true) });
    }
    for (port, title) in listening {
        if tab_ports.contains(port) {
            continue;
        }
        out.push(DevServer { url: format!("http://localhost:{port}/"), port: *port, title: title.clone(), from_tab: None });
    }
    out.truncate(MAX_SERVERS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    #[test]
    fn lsof_macos() {
        let out = "COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
node    41234 chris   23u  IPv6 0x1234      0t0  TCP [::1]:5173 (LISTEN)
node    41235 chris   24u  IPv4 0x1235      0t0  TCP 127.0.0.1:3000 (LISTEN)
rapportd  612 chris    8u  IPv4 0x1236      0t0  TCP *:49152 (LISTEN)
rapportd  612 chris    9u  IPv6 0x1237      0t0  TCP *:49152 (LISTEN)
Safari    700 chris   10u  IPv4 0x1238      0t0  TCP 192.168.1.2:50000->1.2.3.4:443 (ESTABLISHED)";
        assert_eq!(parse_lsof(out), vec![3000, 5173, 49152]);
    }

    #[test]
    fn ss_linux() {
        let out = "LISTEN 0 511 127.0.0.1:3000 0.0.0.0:*\nLISTEN 0 4096 [::1]:5173 [::]:*\nLISTEN 0 128 0.0.0.0:22 0.0.0.0:*\nLISTEN 0 128 *:8080 *:*";
        assert_eq!(parse_ss(out), vec![22, 3000, 5173, 8080]);
    }

    #[test]
    fn netstat_windows_whatever_the_language() {
        let out = "
Connexions actives

  Proto  Adresse locale         Adresse distante       État
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1032
  TCP    127.0.0.1:5173         0.0.0.0:0              ABHÖREN         4412
  TCP    [::]:3000              [::]:0                 LISTENING       4412
  TCP    192.168.1.5:50123      52.1.2.3:443           ESTABLISHED     8000";
        assert_eq!(parse_netstat(out), vec![135, 3000, 5173]);
    }

    #[test]
    fn proc_net_tcp() {
        let out = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1
   1: 0100007F:C350 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 12346 1";
        assert_eq!(parse_proc_net(out), vec![8080]);
    }

    #[test]
    fn title_extraction() {
        assert_eq!(extract_title("<html><head><TITLE>\n  Mon  app &amp; co \n</TITLE>").as_deref(), Some("Mon app & co"));
        assert_eq!(extract_title("<html><body>rien</body>"), None);
        assert_eq!(extract_title("<title> </title>"), None);
    }

    async fn serve(response: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 1024];
                    let _ = s.read(&mut b).await;
                    let _ = s.write_all(response.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn probe_recognizes_http_and_reads_the_title() {
        let p = serve("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><title>Vite App</title></html>").await;
        assert_eq!(probe_port(p).await, ProbeResult { http: true, title: Some("Vite App".into()) });
    }

    #[tokio::test]
    async fn probe_json_api_does_not_count() {
        let p = serve("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}").await;
        assert_eq!(probe_port(p).await, ProbeResult { http: false, title: None });
    }

    #[tokio::test]
    async fn probe_ignores_airplay() {
        let p = serve("HTTP/1.1 403 Forbidden\r\nServer: AirTunes/860.7.1\r\n\r\n").await;
        assert!(!probe_port(p).await.http);
    }

    #[tokio::test]
    async fn probe_discards_non_http_ports() {
        let p = serve("SSH-2.0-pas-du-http\r\n").await;
        assert!(!probe_port(p).await.http);
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let silent = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut keep = vec![];
            while let Ok(c) = l.accept().await {
                keep.push(c);
            }
        });
        assert!(!probe_port(silent).await.http);
    }

    fn scanner(ports: Vec<u16>, calls: Arc<AtomicUsize>) -> DevServerScanner {
        DevServerScanner::with_sources(
            Box::new(move || {
                let p = ports.clone();
                Box::pin(async move { p })
            }),
            Box::new(move |port| {
                calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    match port {
                        5173 => ProbeResult { http: true, title: Some("Vite".into()) },
                        3000 => ProbeResult { http: true, title: None },
                        _ => ProbeResult { http: false, title: None },
                    }
                })
            }),
        )
    }

    #[tokio::test]
    async fn keeps_http_servers_without_link_system_or_ephemeral_ports() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut s = scanner(vec![22, 3000, 5173, 9999, 47615, 49182], calls.clone());
        let got = s.scan(Some(47615), &[], 0).await;
        assert_eq!(
            got,
            vec![
                DevServer { url: "http://localhost:3000/".into(), port: 3000, title: None, from_tab: None },
                DevServer { url: "http://localhost:5173/".into(), port: 5173, title: Some("Vite".into()), from_tab: None },
            ]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn local_tabs_come_first_mapped_to_localhost() {
        let mut s = scanner(vec![5173, 3000], Arc::new(AtomicUsize::new(0)));
        let tab = |u: &str, t: &str| TabLike { url: u.into(), title: t.into() };
        let tabs = [
            tab("http://127.0.0.1:5173/app?x=1", "Mon app"),
            tab("http://[::1]:5173/app?x=1", "Doublon"),
            tab("https://exemple.com/", "Dehors"),
            tab("http://localhost:47615/", "Le lien lui-meme"),
        ];
        let got = s.scan(Some(47615), &tabs, 0).await;
        assert_eq!(
            got,
            vec![
                DevServer { url: "http://localhost:5173/app?x=1".into(), port: 5173, title: Some("Mon app".into()), from_tab: Some(true) },
                DevServer { url: "http://localhost:3000/".into(), port: 3000, title: None, from_tab: None },
            ]
        );
    }

    #[tokio::test]
    async fn does_not_probe_a_recent_port_twice() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut s = scanner(vec![5173], calls.clone());
        s.scan(None, &[], 0).await;
        s.scan(None, &[], 5000).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        s.scan(None, &[], 35_000).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
