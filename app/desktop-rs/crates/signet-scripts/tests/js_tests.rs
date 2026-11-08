//! Runs the JS tests (`tests/js`, Node's built-in test runner) when `node` is available, on the bundles that
//! `signet_scripts` generates. Skips quietly without node.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use signet_scripts::{FrameContext, Registry, RunAt, ScriptSettings};

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn ctx(url: &str, main: bool, gpc: bool, webauthn: bool) -> FrameContext {
    FrameContext {
        url: url.into(),
        is_main_frame: main,
        is_private: false,
        settings: ScriptSettings { global_privacy_control: gpc, webauthn_intercept: webauthn },
    }
}

fn dump_bundles(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signet-scripts-bundles-{}-{tag}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let r = Registry::default();
    let cases = [
        ("web-main.js", ctx("https://example.com/", true, false, false)),
        ("web-sub.js", ctx("https://example.com/", false, false, false)),
        ("notes.js", ctx("signet://note/0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11", true, false, false)),
        ("gpc.js", ctx("https://example.com/", true, true, false)),
        ("everything.js", ctx("https://example.com/", true, true, true)),
    ];
    for (name, c) in cases {
        let js = r.bundle_for(&c, RunAt::DocumentStart).expect("a bundle");
        fs::write(dir.join(name), js).expect("write bundle");
    }
    dir
}

#[test]
fn bundles_are_valid_javascript() {
    if !node_available() {
        eprintln!("node not found: skipped");
        return;
    }
    let dir = dump_bundles("check");
    for entry in fs::read_dir(&dir).expect("read dir").flatten() {
        let out = Command::new("node").arg("--check").arg(entry.path()).output().expect("run node");
        assert!(out.status.success(), "{:?}: {}", entry.path(), String::from_utf8_lossy(&out.stderr));
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn node_tests_pass() {
    if !node_available() {
        eprintln!("node not found: skipped");
        return;
    }
    let dir = dump_bundles("node");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("node")
        .current_dir(&root)
        .env("SIGNET_BUNDLE_DIR", &dir)
        .args(["--test", "tests/js"])
        .output()
        .expect("run node");
    let _ = fs::remove_dir_all(dir);
    assert!(
        out.status.success(),
        "node tests failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
