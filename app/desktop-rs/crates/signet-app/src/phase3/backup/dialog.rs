//! Blocking native dialogs (file pickers and message boxes) for the main-process features. GPUI prompts need an
//! `App` context and are asynchronous, while `SignetApi` methods are synchronous like the Electron IPC handlers
//! that awaited `dialog.*`. Each platform therefore uses its own system dialog through a helper process:
//! macOS `osascript`, Linux `zenity` (or `kdialog`), Windows PowerShell with Windows Forms. Called on the main
//! thread: the UI is blocked while the dialog is open, like a modal sheet.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Icon of a message box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// Output of a helper process when it exits successfully.
#[allow(dead_code)]
fn capture(program: &str, args: &[String], envs: &[(&str, &str)]) -> Option<String> {
    let out = Command::new(program).args(args).envs(envs.iter().copied()).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Message and detail as one text block.
#[allow(dead_code)]
fn text_of(message: &str, detail: &str) -> String {
    if detail.is_empty() { message.to_string() } else { format!("{message}\n\n{detail}") }
}

/// Adds `.ext` if the user typed a name without it.
pub fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
        return path;
    }
    let mut s = path.into_os_string();
    s.push(format!(".{ext}"));
    PathBuf::from(s)
}

fn path_of(out: Option<String>) -> Option<PathBuf> {
    let s = out?;
    let s = s.trim_end_matches(['\r', '\n']);
    (!s.is_empty()).then(|| PathBuf::from(s))
}

#[cfg(target_os = "macos")]
fn osascript(lines: &[&str], args: &[String]) -> Option<String> {
    let mut a: Vec<String> = Vec::new();
    for l in lines {
        a.push("-e".into());
        a.push((*l).into());
    }
    a.extend(args.iter().cloned());
    capture("osascript", &a, &[])
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
fn have(program: &str) -> bool {
    Command::new(program).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

/// Escapes the markup characters zenity interprets in `--text`.
#[allow(dead_code)]
fn pango_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(target_os = "windows")]
fn powershell(script: &str, envs: &[(&str, &str)]) -> Option<String> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let ps = format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let full = format!("Add-Type -AssemblyName System.Windows.Forms; {script}");
    capture(&ps, &["-NoProfile".into(), "-STA".into(), "-Command".into(), full], envs)
}

/// Save dialog. `default_path` is the proposed file; the result always ends with `.ext`. `None` = cancelled.
pub fn save_file(title: &str, default_path: &Path, filter_name: &str, ext: &str) -> Option<PathBuf> {
    let dir = default_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let name = default_path.file_name().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (&dir, &name);
    #[cfg(target_os = "macos")]
    let out = {
        let _ = (filter_name, &dir);
        osascript(
            &[
                "on run argv",
                "set d to POSIX file (item 3 of argv)",
                "set f to choose file name with prompt (item 1 of argv) default name (item 2 of argv) default location d",
                "return POSIX path of f",
                "end run",
            ],
            &[title.into(), name, dir],
        )
    };
    #[cfg(target_os = "windows")]
    let out = powershell(
        "$d = New-Object System.Windows.Forms.SaveFileDialog; $d.Title = $env:SG_TITLE; $d.FileName = $env:SG_NAME; \
         $d.InitialDirectory = $env:SG_DIR; $d.OverwritePrompt = $true; \
         $d.Filter = \"$($env:SG_FILTER) (*.$($env:SG_EXT))|*.$($env:SG_EXT)\"; \
         if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.FileName) }",
        &[("SG_TITLE", title), ("SG_NAME", &name), ("SG_DIR", &dir), ("SG_FILTER", filter_name), ("SG_EXT", ext)],
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let out = {
        let full = default_path.to_string_lossy().into_owned();
        if have("zenity") {
            capture(
                "zenity",
                &["--file-selection".into(), "--save".into(), "--confirm-overwrite".into(), "--title".into(), title.into(), "--filename".into(), full, "--file-filter".into(), format!("{filter_name} | *.{ext}")],
                &[],
            )
        } else {
            capture("kdialog", &["--title".into(), title.into(), "--getsavefilename".into(), full, format!("*.{ext}|{filter_name}")], &[])
        }
    };
    path_of(out).map(|p| with_extension(p, ext))
}

/// Open dialog for one file of the given extension. `None` = cancelled.
pub fn open_file(title: &str, filter_name: &str, ext: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let out = {
        let _ = filter_name;
        osascript(
            &["on run argv", "set f to choose file with prompt (item 1 of argv) of type {item 2 of argv}", "return POSIX path of f", "end run"],
            &[title.into(), ext.into()],
        )
    };
    #[cfg(target_os = "windows")]
    let out = powershell(
        "$d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = $env:SG_TITLE; \
         $d.Filter = \"$($env:SG_FILTER) (*.$($env:SG_EXT))|*.$($env:SG_EXT)\"; \
         if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.FileName) }",
        &[("SG_TITLE", title), ("SG_FILTER", filter_name), ("SG_EXT", ext)],
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let out = if have("zenity") {
        capture("zenity", &["--file-selection".into(), "--title".into(), title.into(), "--file-filter".into(), format!("{filter_name} | *.{ext}")], &[])
    } else {
        capture("kdialog", &["--title".into(), title.into(), "--getopenfilename".into(), ".".into(), format!("*.{ext}|{filter_name}")], &[])
    };
    path_of(out)
}

/// Two-button question. Returns true if the user picked `ok`. `default_cancel`: Cancel is the default button
/// (destructive actions).
pub fn confirm(level: Level, message: &str, detail: &str, ok: &str, cancel: &str, default_cancel: bool) -> bool {
    let text = text_of(message, detail);
    #[cfg(target_os = "macos")]
    {
        let icon = if level == Level::Info { "note" } else { "caution" };
        let default = if default_cancel { "item 3 of argv" } else { "item 2 of argv" };
        let script = format!(
            "set r to display dialog (item 1 of argv) with title \"Signet\" buttons {{item 3 of argv, item 2 of argv}} default button ({default}) cancel button (item 3 of argv) with icon {icon}"
        );
        let out = osascript(&["on run argv", &script, "return button returned of r", "end run"], &[text, ok.into(), cancel.into()]);
        return out.is_some_and(|o| o.trim() == ok);
    }
    #[cfg(target_os = "windows")]
    {
        let icon = match level {
            Level::Info => "Question",
            Level::Warning => "Warning",
            Level::Error => "Error",
        };
        let default = if default_cancel { "Button2" } else { "Button1" };
        let script = format!(
            "$r = [System.Windows.Forms.MessageBox]::Show($env:SG_TEXT + [Environment]::NewLine + [Environment]::NewLine + \"[OK] \" + $env:SG_OK + \"   [Cancel] \" + $env:SG_CANCEL, 'Signet', 'OKCancel', '{icon}', '{default}'); [Console]::Out.Write($r)"
        );
        let out = powershell(&script, &[("SG_TEXT", &text), ("SG_OK", ok), ("SG_CANCEL", cancel)]);
        return out.is_some_and(|o| o.trim() == "OK");
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (level, default_cancel);
        if have("zenity") {
            capture("zenity", &["--question".into(), "--title".into(), "Signet".into(), "--no-markup".into(), "--text".into(), text, "--ok-label".into(), ok.into(), "--cancel-label".into(), cancel.into()], &[]).is_some()
        } else {
            capture("kdialog", &["--title".into(), "Signet".into(), "--yes-label".into(), ok.into(), "--no-label".into(), cancel.into(), "--yesno".into(), text], &[]).is_some()
        }
    }
}

/// One-button message.
pub fn notify(level: Level, message: &str, detail: &str, ok: &str) {
    let text = text_of(message, detail);
    #[cfg(target_os = "macos")]
    {
        let icon = match level {
            Level::Info => "note",
            Level::Warning => "caution",
            Level::Error => "stop",
        };
        let script = format!("display dialog (item 1 of argv) with title \"Signet\" buttons {{item 2 of argv}} default button 1 with icon {icon}");
        let _ = osascript(&["on run argv", &script, "end run"], &[text, ok.into()]);
    }
    #[cfg(target_os = "windows")]
    {
        let icon = match level {
            Level::Info => "Information",
            Level::Warning => "Warning",
            Level::Error => "Error",
        };
        let script = format!("[void][System.Windows.Forms.MessageBox]::Show($env:SG_TEXT, 'Signet', 'OK', '{icon}')");
        let _ = powershell(&script, &[("SG_TEXT", &text)]);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = ok;
        if have("zenity") {
            let kind = match level {
                Level::Info => "--info",
                Level::Warning => "--warning",
                Level::Error => "--error",
            };
            let _ = capture("zenity", &[kind.into(), "--title".into(), "Signet".into(), "--no-markup".into(), "--text".into(), text], &[]);
        } else {
            let kind = match level {
                Level::Info => "--msgbox",
                Level::Warning => "--sorry",
                Level::Error => "--error",
            };
            let _ = capture("kdialog", &["--title".into(), "Signet".into(), kind.into(), text], &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_added_once() {
        assert_eq!(with_extension(PathBuf::from("/a/b"), "signetbackup"), PathBuf::from("/a/b.signetbackup"));
        assert_eq!(with_extension(PathBuf::from("/a/b.SignetBackup"), "signetbackup"), PathBuf::from("/a/b.SignetBackup"));
        assert_eq!(with_extension(PathBuf::from("/a/b.txt"), "signetbackup"), PathBuf::from("/a/b.txt.signetbackup"));
    }

    #[test]
    fn helper_output_is_trimmed() {
        assert_eq!(path_of(Some("/x/y\n".into())), Some(PathBuf::from("/x/y")));
        assert_eq!(path_of(Some("\n".into())), None);
        assert_eq!(path_of(None), None);
    }

    #[test]
    fn texts() {
        assert_eq!(text_of("a", ""), "a");
        assert_eq!(text_of("a", "b"), "a\n\nb");
        assert_eq!(pango_escape("a<b>&"), "a&lt;b&gt;&amp;");
    }
}
