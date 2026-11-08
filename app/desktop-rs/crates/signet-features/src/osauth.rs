//! OS authentication before revealing passwords (port of `main/app/osauth.ts`).
//!
//! macOS: LocalAuthentication through `osascript` (JXA), which covers Touch ID and the account password, exactly like
//! the Electron fallback. Windows: Windows Hello through PowerShell. Linux: no universal prompt, `Unavailable`.
//! The app can replace this with its own implementation of `Authenticator` (for example native Touch ID).
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthResult {
    Ok,
    Denied,
    Unavailable,
}

pub trait Authenticator: Send + Sync {
    fn verify(&self, reason: &str) -> AuthResult;
}

const TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(target_os = "macos")]
const JXA: &str = r#"
ObjC.import('LocalAuthentication');
ObjC.import('Foundation');
function run(argv) {
  try {
    var ctx = $.LAContext.alloc.init;
    var err = Ref();
    if (!ctx.canEvaluatePolicyError(2, err)) return 'unavailable';
    var state = 'pending';
    ctx.evaluatePolicyLocalizedReasonReply(2, argv[0], function (ok, e) { state = ok ? 'ok' : 'denied'; });
    var until = Date.now() + 55000;
    while (state === 'pending' && Date.now() < until) {
      $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.2));
    }
    return state === 'pending' ? 'denied' : state;
  } catch (e) {
    return 'denied';
  }
}
"#;

#[cfg(target_os = "windows")]
const POWERSHELL: &str = r#"
$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  $uc = [Windows.Security.Credentials.UI.UserConsentVerifier, Windows.Security.Credentials.UI, ContentType = WindowsRuntime]
  $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
  })[0]
  function Await($op, $type) {
    $task = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
    $task.Wait(-1) | Out-Null
    $task.Result
  }
  $avail = Await ($uc::CheckAvailabilityAsync()) ([Windows.Security.Credentials.UI.UserConsentVerifierAvailability])
  if ($avail.ToString() -ne 'Available') { 'unavailable'; exit 0 }
  $res = Await ($uc::RequestVerificationAsync($env:SIGNET_AUTH_REASON)) ([Windows.Security.Credentials.UI.UserConsentVerificationResult])
  if ($res.ToString() -eq 'Verified') { 'ok' } else { 'denied' }
} catch {
  'denied'
}
"#;

/// Parses the last line of a script's output; any failure means a refusal.
pub fn parse_output(out: &str) -> AuthResult {
    match out.trim().lines().last().map(str::trim) {
        Some("ok") => AuthResult::Ok,
        Some("unavailable") => AuthResult::Unavailable,
        _ => AuthResult::Denied,
    }
}

#[allow(dead_code)]
fn run(file: &str, args: &[String], env: &[(&str, &str)]) -> AuthResult {
    let mut cmd = Command::new(file);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let Ok(mut child) = cmd.spawn() else { return AuthResult::Denied };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return AuthResult::Denied;
                }
                let mut s = String::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut s);
                }
                return parse_output(&s);
            }
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return AuthResult::Denied;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return AuthResult::Denied,
        }
    }
}

/// Real implementation.
pub struct SystemAuth;

impl Authenticator for SystemAuth {
    #[allow(unused_variables)]
    fn verify(&self, reason: &str) -> AuthResult {
        #[cfg(target_os = "macos")]
        return run("/usr/bin/osascript", &["-l".into(), "JavaScript".into(), "-e".into(), JXA.into(), reason.into()], &[]);
        #[cfg(target_os = "windows")]
        {
            use base64::Engine;
            let utf16: Vec<u8> = POWERSHELL.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            let ps = format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            return run(&ps, &["-NoProfile".into(), "-NonInteractive".into(), "-EncodedCommand".into(), encoded], &[("SIGNET_AUTH_REASON", reason)]);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        AuthResult::Unavailable
    }
}

/// After a successful authentication, later reveals do not ask again for one minute.
pub const AUTH_GRACE: Duration = Duration::from_secs(60);

/// Authentication with the grace period. `confirm_fallback` is the weaker confirmation dialog used when the
/// system cannot authenticate (Linux, no Windows Hello); the app shows it and returns the user's answer.
pub struct AuthGate {
    auth: Box<dyn Authenticator>,
    until: Mutex<Option<Instant>>,
}

impl AuthGate {
    pub fn new(auth: Box<dyn Authenticator>) -> Self {
        AuthGate { auth, until: Mutex::new(None) }
    }

    pub fn authenticate(&self, reason: &str, confirm_fallback: &dyn Fn(&str) -> bool) -> bool {
        if self.until.lock().ok().and_then(|u| *u).is_some_and(|t| Instant::now() < t) {
            return true;
        }
        match self.auth.verify(reason) {
            AuthResult::Denied => return false,
            AuthResult::Unavailable => {
                if !confirm_fallback(reason) {
                    return false;
                }
            }
            AuthResult::Ok => {}
        }
        if let Ok(mut u) = self.until.lock() {
            *u = Some(Instant::now() + AUTH_GRACE);
        }
        true
    }

    /// Forgets the grace period (lock).
    pub fn lock(&self) {
        if let Ok(mut u) = self.until.lock() {
            *u = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Fake(AuthResult, Arc<AtomicUsize>);
    impl Authenticator for Fake {
        fn verify(&self, _: &str) -> AuthResult {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0
        }
    }

    #[test]
    fn output_parsing() {
        assert_eq!(parse_output("ok\n"), AuthResult::Ok);
        assert_eq!(parse_output("noise\nunavailable\n"), AuthResult::Unavailable);
        assert_eq!(parse_output("whatever"), AuthResult::Denied);
        assert_eq!(parse_output(""), AuthResult::Denied);
    }

    #[test]
    fn grace_period_and_fallback() {
        let n = Arc::new(AtomicUsize::new(0));
        let g = AuthGate::new(Box::new(Fake(AuthResult::Ok, n.clone())));
        assert!(g.authenticate("r", &|_| false));
        assert!(g.authenticate("r", &|_| false));
        assert_eq!(n.load(Ordering::SeqCst), 1);
        g.lock();
        assert!(g.authenticate("r", &|_| false));
        assert_eq!(n.load(Ordering::SeqCst), 2);

        let denied = AuthGate::new(Box::new(Fake(AuthResult::Denied, Arc::new(AtomicUsize::new(0)))));
        assert!(!denied.authenticate("r", &|_| true));
        let unavailable = AuthGate::new(Box::new(Fake(AuthResult::Unavailable, Arc::new(AtomicUsize::new(0)))));
        assert!(!unavailable.authenticate("r", &|_| false));
        assert!(unavailable.authenticate("r", &|_| true));
    }
}
