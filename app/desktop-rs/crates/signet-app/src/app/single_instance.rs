//! Single instance (`app.requestSingleInstanceLock` of `index.ts`): the first launch listens on a loopback port
//! written to `<data>/instance.port`; a later launch connects, sends its URL arguments and exits.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub enum Role {
    /// This process is the first one; messages from later launches arrive on the receiver.
    First(futures::channel::mpsc::UnboundedReceiver<Vec<String>>),
    /// Another instance took the arguments.
    Forwarded,
}

fn port_file(dir: &Path) -> PathBuf {
    dir.join("instance.port")
}

/// URL-like arguments of the command line (everything that is not a flag), and the web app launch flag
/// `--signet-webapp=<id>` (sites installed as apps start the browser with it).
pub fn url_args() -> Vec<String> {
    std::env::args().skip(1).filter(|a| !a.starts_with('-') || a.starts_with("--signet-webapp=")).collect()
}

pub fn acquire(dir: &Path, args: &[String]) -> Role {
    if let Some(port) = std::fs::read_to_string(port_file(dir)).ok().and_then(|s| s.trim().parse::<u16>().ok())
        && let Ok(mut s) = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(400))
    {
        let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
        let line = serde_json::to_string(args).unwrap_or_else(|_| "[]".into());
        if writeln!(s, "{line}").is_ok() {
            return Role::Forwarded;
        }
    }
    let (tx, rx) = futures::channel::mpsc::unbounded();
    match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => {
            if let Ok(addr) = listener.local_addr() {
                let _ = std::fs::write(port_file(dir), addr.port().to_string());
            }
            std::thread::spawn(move || {
                for conn in listener.incoming().flatten() {
                    let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut line = String::new();
                    if BufReader::new(conn).read_line(&mut line).is_ok()
                        && let Ok(args) = serde_json::from_str::<Vec<String>>(line.trim())
                        && tx.unbounded_send(args).is_err()
                    {
                        break;
                    }
                }
            });
        }
        Err(e) => eprintln!("[instance] cannot listen: {e}"),
    }
    Role::First(rx)
}

pub fn release(dir: &Path) {
    let _ = std::fs::remove_file(port_file(dir));
}
