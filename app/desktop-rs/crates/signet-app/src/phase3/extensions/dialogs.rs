//! Native dialogs and notifications without extra dependencies: the platform's own tools (`osascript`, `zenity` or
//! `kdialog`, PowerShell). Every function blocks until the user answered: call them from a worker thread.

use std::path::PathBuf;
use std::process::Command;

/// AppleScript string literal.
pub fn applescript_quote(s: &str) -> String {
    let parts: Vec<String> = s.split('\n').map(|l| format!("\"{}\"", l.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    parts.join(" & return & ")
}

/// PowerShell single quoted literal.
#[allow(dead_code)]
pub fn powershell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Asks a yes/no question. `false` when cancelled or when no dialog tool exists.
pub fn confirm(title: &str, message: &str, ok: &str, cancel: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display dialog {} with title {} buttons {{{}, {}}} default button {} cancel button {}",
            applescript_quote(message),
            applescript_quote(title),
            applescript_quote(cancel),
            applescript_quote(ok),
            applescript_quote(ok),
            applescript_quote(cancel)
        );
        Command::new("osascript").args(["-e", &script]).output().map(|o| o.status.success()).unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (ok, cancel);
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; if ([System.Windows.Forms.MessageBox]::Show({}, {}, 'OKCancel') -eq 'OK') {{ exit 0 }} else {{ exit 1 }}",
            powershell_quote(message),
            powershell_quote(title)
        );
        Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().map(|o| o.status.success()).unwrap_or(false)
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let z = Command::new("zenity").args(["--question", "--no-markup", "--title", title, "--text", message, "--ok-label", ok, "--cancel-label", cancel]).status();
        match z {
            Ok(s) => s.success(),
            Err(_) => Command::new("kdialog").args(["--title", title, "--yesno", message]).status().map(|s| s.success()).unwrap_or(false),
        }
    }
}

/// Shows a message with an OK button.
pub fn alert(title: &str, message: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!("display dialog {} with title {} buttons {{\"OK\"}} default button \"OK\"", applescript_quote(message), applescript_quote(title));
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!("Add-Type -AssemblyName System.Windows.Forms; [void][System.Windows.Forms.MessageBox]::Show({}, {})", powershell_quote(message), powershell_quote(title));
        let _ = Command::new("powershell").args(["-NoProfile", "-Command", &script]).output();
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if Command::new("zenity").args(["--info", "--no-markup", "--title", title, "--text", message]).status().is_err() {
            let _ = Command::new("kdialog").args(["--title", title, "--msgbox", message]).status();
        }
    }
}

/// Lets the user choose a folder.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let out = Command::new("osascript").args(["-e", &format!("POSIX path of (choose folder with prompt {})", applescript_quote(title))]).output().ok()?;
    #[cfg(target_os = "windows")]
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = {}; if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.SelectedPath }}", powershell_quote(title))])
        .output()
        .ok()?;
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let out = match Command::new("zenity").args(["--file-selection", "--directory", "--title", title]).output() {
        Ok(o) => o,
        Err(_) => Command::new("kdialog").args(["--title", title, "--getexistingdirectory", "."]).output().ok()?,
    };
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// Shows a system notification (no click handling).
pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification {} with title {}", applescript_quote(body), applescript_quote(title));
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
    #[cfg(target_os = "windows")]
    {
        // No toast without a registered app id: a balloon from a hidden NotifyIcon.
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $n = New-Object System.Windows.Forms.NotifyIcon; $n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; $n.ShowBalloonTip(5000, {}, {}, 'Info'); Start-Sleep -Seconds 6; $n.Dispose()",
            powershell_quote(title),
            powershell_quote(body)
        );
        let _ = Command::new("powershell").args(["-NoProfile", "-Command", &script]).output();
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let _ = Command::new("notify-send").args([title, body]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_quotes_and_newlines() {
        assert_eq!(applescript_quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(applescript_quote("one\ntwo"), "\"one\" & return & \"two\"");
        assert_eq!(powershell_quote("it's"), "'it''s'");
    }
}
