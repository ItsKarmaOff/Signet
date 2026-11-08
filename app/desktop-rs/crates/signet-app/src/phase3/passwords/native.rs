//! Blocking native dialogs and clipboard reading, through the system tools (no extra dependency): `osascript` on
//! macOS, PowerShell on Windows, `zenity` on Linux. Text always travels through arguments or environment variables,
//! never spliced into a script. Call them off the UI thread when the caller can wait.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Runs a command and returns its trimmed stdout when it exits successfully with something to say.
fn run(cmd: &mut Command) -> Option<String> {
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(target_os = "windows")]
fn powershell(script: &str, env: &[(&str, &str)]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script]).creation_flags(0x0800_0000);
    for (k, v) in env {
        c.env(k, v);
    }
    run(&mut c)
}

/// Asks for an existing CSV file.
pub fn choose_csv(prompt: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let out = run(Command::new("osascript").args([
        "-e",
        "on run argv",
        "-e",
        "POSIX path of (choose file with prompt (item 1 of argv) of type {\"csv\", \"public.comma-separated-values-text\"})",
        "-e",
        "end run",
        prompt,
    ]));
    #[cfg(target_os = "windows")]
    let out = powershell(
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = $env:SIGNET_TITLE; $d.Filter = 'CSV (*.csv)|*.csv|All files (*.*)|*.*'; if ($d.ShowDialog() -eq 'OK') { $d.FileName }",
        &[("SIGNET_TITLE", prompt)],
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let out = run(Command::new("zenity").args(["--file-selection", "--title", prompt, "--file-filter=CSV | *.csv", "--file-filter=All files | *"]));
    out.map(PathBuf::from)
}

/// Asks where to save a CSV file, starting from `default` (a full path).
pub fn save_csv(prompt: &str, default: &std::path::Path) -> Option<PathBuf> {
    let name = default.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = default.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    #[cfg(target_os = "macos")]
    let out = {
        let _ = &dir;
        run(Command::new("osascript").args([
            "-e",
            "on run argv",
            "-e",
            "POSIX path of (choose file name with prompt (item 1 of argv) default name (item 2 of argv))",
            "-e",
            "end run",
            prompt,
            &name,
        ]))
    };
    #[cfg(target_os = "windows")]
    let out = powershell(
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.SaveFileDialog; $d.Title = $env:SIGNET_TITLE; $d.FileName = $env:SIGNET_NAME; if ($env:SIGNET_DIR) { $d.InitialDirectory = $env:SIGNET_DIR }; $d.Filter = 'CSV (*.csv)|*.csv'; $d.OverwritePrompt = $true; if ($d.ShowDialog() -eq 'OK') { $d.FileName }",
        &[("SIGNET_TITLE", prompt), ("SIGNET_NAME", &name), ("SIGNET_DIR", &dir)],
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let out = {
        let start = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
        run(Command::new("zenity").args(["--file-selection", "--save", "--confirm-overwrite", "--title", prompt, "--filename", &start]))
    };
    out.map(PathBuf::from)
}

/// Weaker confirmation used when the system cannot authenticate (Linux, no Windows Hello): true if allowed.
pub fn confirm(title: &str, detail: &str, allow: &str, cancel: &str) -> bool {
    #[cfg(target_os = "macos")]
    let ok = run(Command::new("osascript").args([
        "-e",
        "on run argv",
        "-e",
        "display dialog (item 2 of argv) with title (item 1 of argv) buttons {(item 4 of argv), (item 3 of argv)} default button (item 4 of argv) cancel button (item 4 of argv) with icon caution",
        "-e",
        "end run",
        title,
        detail,
        allow,
        cancel,
    ]))
    .is_some();
    #[cfg(target_os = "windows")]
    let ok = {
        let _ = (allow, cancel);
        powershell(
            "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show($env:SIGNET_DETAIL, $env:SIGNET_TITLE, 'YesNo', 'Warning', 'Button2')",
            &[("SIGNET_TITLE", title), ("SIGNET_DETAIL", detail)],
        )
        .as_deref()
            == Some("Yes")
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let ok = Command::new("zenity")
        .args(["--question", "--title", title, "--text", detail, "--ok-label", allow, "--cancel-label", cancel])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ok
}

/// Text currently on the system clipboard.
pub fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "macos")]
    let out = Command::new("pbpaste").stdin(Stdio::null()).stderr(Stdio::null()).output().ok();
    #[cfg(target_os = "windows")]
    let out = {
        use std::os::windows::process::CommandExt;
        Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard -Raw"]).creation_flags(0x0800_0000).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let out = ["wl-paste", "xclip"]
        .iter()
        .find_map(|p| {
            let args: &[&str] = if *p == "xclip" { &["-selection", "clipboard", "-o"] } else { &["-n"] };
            Command::new(p).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success())
        });
    let out = out?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
