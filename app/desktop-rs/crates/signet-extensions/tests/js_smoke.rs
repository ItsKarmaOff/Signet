//! Runs the page-side scripts in Node against a fake bridge (skipped when `node` is not installed).

use serde_json::json;
use signet_extensions::host::{ExtensionHost, HostConfig};
use std::fs;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn the_context_script_builds_a_working_chrome_object() {
    if !node_available() {
        eprintln!("node not found: skipping");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let ext = tempfile::tempdir().unwrap();
    fs::create_dir_all(ext.path().join("_locales/en")).unwrap();
    fs::write(ext.path().join("_locales/en/messages.json"), r#"{ "Hello": { "message": "Hello $WHO$ $1", "placeholders": { "who": { "content": "world" } } } }"#).unwrap();
    fs::write(
        ext.path().join("manifest.json"),
        json!({ "manifest_version": 3, "name": "Smoke", "version": "1.0", "default_locale": "en", "permissions": ["storage", "alarms", "contextMenus", "tabs"], "action": {}, "background": { "service_worker": "sw.js" } }).to_string(),
    )
    .unwrap();
    let mut host = ExtensionHost::new(HostConfig::new(data.path()));
    host.add_unpacked(ext.path()).unwrap();
    let id = host.registry.list()[0].id.clone();
    let script = host.page_script(&id, "page", "page-1", None, 0).expect("script");
    assert!(host.implemented_methods().iter().any(|m| m == "storage.local.get"));

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("context.js"), &script).unwrap();
    fs::write(
        dir.path().join("driver.js"),
        format!(
            r#"
const posted = [];
globalThis.location = {{ href: 'chrome-extension://{id}/popup.html', origin: 'chrome-extension://{id}' }};
globalThis.__signetPost = (channel, json) => {{
  const msg = JSON.parse(json);
  posted.push(msg.method);
  let result = {{ ok: true }};
  if (msg.method === 'storage.local.get') result = {{ ok: true, value: {{ a: 1 }} }};
  if (msg.method === 'alarms.create') result = {{ ok: false, error: 'boom' }};
  setTimeout(() => globalThis.__signetCrxDispatch('signet-crx-reply', JSON.stringify({{ id: msg.id, result }})), 0);
}};
require('./context.js');
const assert = require('assert');
(async () => {{
  assert.strictEqual(chrome, browser, 'browser is chrome');
  assert.strictEqual(chrome.runtime.id, '{id}');
  assert.strictEqual(chrome.runtime.getURL('/a.html'), 'chrome-extension://{id}/a.html');
  assert.strictEqual(chrome.runtime.getManifest().name, 'Smoke');
  assert.deepStrictEqual(await chrome.storage.local.get('a'), {{ a: 1 }});
  // callback style + lastError
  await new Promise((resolve) => chrome.alarms.create('x', {{ delayInMinutes: 1 }}, () => {{
    assert.strictEqual(chrome.runtime.lastError.message, 'boom');
    resolve();
  }}));
  assert.strictEqual(chrome.runtime.lastError, undefined);
  assert.strictEqual(chrome.i18n.getMessage('hello', 'there'), 'Hello world there');
  // safety net: not implemented members reject, events exist
  await assert.rejects(chrome.tabs.group({{}}), /chrome\.tabs\.group is not supported in Signet/);
  assert.strictEqual(typeof chrome.tabs.onUpdated.addListener, 'function');
  assert.strictEqual(chrome.proxy, undefined, 'no proxy permission, no proxy namespace');
  assert.ok(Object.isFrozen(chrome));
  // events pushed by the host reach listeners
  let got;
  chrome.storage.onChanged.addListener((c, area) => (got = [c, area]));
  globalThis.__signetCrxDispatch('signet-crx-event', JSON.stringify({{ name: 'storage.onChanged', args: [{{ k: {{ newValue: 1 }} }}, 'local'] }}));
  assert.deepStrictEqual(got, [{{ k: {{ newValue: 1 }} }}, 'local']);
  // runtime messaging: a listener answers through sendResponse
  chrome.runtime.onMessage.addListener((m, s, send) => {{ send({{ echo: m }}); return true; }});
  globalThis.__signetCrxDispatch('signet-crx-ctx', JSON.stringify({{ name: 'runtime.onMessage', args: ['hi', {{}}, 7] }}));
  await new Promise((r) => setTimeout(r, 5));
  assert.ok(posted.includes('runtime.__reply'));
  assert.ok(posted.includes('__installed'), 'reports its installation');
  assert.ok(posted.includes('runtime.__openContext'));
  console.log('JS OK');
}})().catch((e) => {{ console.error(e); process.exit(1); }});
"#
        ),
    )
    .unwrap();
    let out = Command::new("node").arg("driver.js").current_dir(dir.path()).output().expect("run node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("JS OK"));
}

/// Chromium installs its own `chrome.webstorePrivate` after the page script ran: Signet's must keep winning, and every
/// method must work both with a callback and as a Promise (the store pages use both).
#[test]
fn the_store_script_wins_over_the_native_web_store_api() {
    if !node_available() {
        eprintln!("node not found: skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("store.js"), signet_extensions::assets::webstore_script()).unwrap();
    fs::write(
        dir.path().join("driver.js"),
        r#"
const posted = [];
globalThis.document = { addEventListener() {} };
globalThis.addEventListener = () => {};
globalThis.__signetCrx = {
  call: (method) => { posted.push(method); return Promise.resolve({ ok: true, value: method.endsWith('getExtensionStatus') ? 'installable' : '' }); }
};
globalThis.chrome = {};
require('./store.js');
const assert = require('assert');
(async () => {
  // Chromium's bindings arrive later and assign or define the native object.
  chrome.webstorePrivate = { beginInstallWithManifest3() { throw new Error('native'); } };
  try { Object.defineProperty(chrome, 'webstorePrivate', { value: { native: true } }); } catch (e) {}
  await new Promise((r) => setTimeout(r, 60));
  assert.ok(!chrome.webstorePrivate.native, 'ours wins');
  assert.strictEqual(await chrome.webstorePrivate.getExtensionStatus('abc'), 'installable');
  await new Promise((r) => chrome.webstorePrivate.getExtensionStatus('abc', '{}', (s) => { assert.strictEqual(s, 'installable'); r(); }));
  assert.strictEqual(await chrome.webstorePrivate.getBrowserLogin(), '');
  assert.ok(posted.includes('webstorePrivate.getExtensionStatus'));
  console.log('JS OK');
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
"#,
    )
    .unwrap();
    let out = Command::new("node").arg("driver.js").current_dir(dir.path()).output().expect("run node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("JS OK"));
}
