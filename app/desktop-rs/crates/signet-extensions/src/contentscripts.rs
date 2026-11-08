//! Content scripts: which manifest scripts apply to a frame, and the JS to inject. New code.
//!
//! CEF (Alloy style) has no extension system, so the web layer injects content scripts itself: for each frame it asks
//! [`scripts_for`] at `document_start` (and again at `document_end` / `document_idle`), reads the files with
//! [`ScriptPlan::load`], and executes the string from [`ScriptPlan::js_source`] in the frame. CEF has no isolated worlds
//! for page scripts, so the `ISOLATED` world is emulated: every script group runs inside its own function scope with a
//! private `chrome` object (see assets/content-chrome.js). Page globals stay visible (a documented limitation).

use crate::manifest::{ContentScript, Manifest, RunAt, World};
use crate::urlmatch::MatchPattern;
use regex::Regex;
use std::fs;
use std::path::Path;

/// A glob of `include_globs` / `exclude_globs`: `*` is any run of characters, `?` one character.
pub fn glob_matches(glob: &str, url: &str) -> bool {
    let mut re = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            other => re.push_str(&regex::escape(&other.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).is_ok_and(|r| r.is_match(url))
}

/// The frame a script may be injected in.
#[derive(Debug, Clone)]
pub struct FrameContext<'a> {
    pub url: &'a str,
    pub is_main_frame: bool,
    /// URL of the frame's parent (or opener) for `about:blank`, `about:srcdoc` and `data:` frames with
    /// `match_origin_as_fallback`.
    pub fallback_url: Option<&'a str>,
}

fn is_blank(url: &str) -> bool {
    url == "about:blank" || url == "about:srcdoc"
}

/// Does the content script apply to this frame?
pub fn applies(cs: &ContentScript, frame: &FrameContext) -> bool {
    if !frame.is_main_frame && !cs.all_frames {
        return false;
    }
    let url = if is_blank(frame.url) || frame.url.starts_with("data:") {
        if !(cs.match_about_blank || cs.match_origin_as_fallback) {
            return false;
        }
        match frame.fallback_url {
            Some(u) => u,
            None => return false,
        }
    } else {
        frame.url
    };
    let matches = cs.matches.iter().filter_map(|p| MatchPattern::parse(p)).any(|p| p.matches(url));
    if !matches {
        return false;
    }
    if cs.exclude_matches.iter().filter_map(|p| MatchPattern::parse(p)).any(|p| p.matches(url)) {
        return false;
    }
    if !cs.include_globs.is_empty() && !cs.include_globs.iter().any(|g| glob_matches(g, url)) {
        return false;
    }
    !cs.exclude_globs.iter().any(|g| glob_matches(g, url))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptPlan {
    pub extension_id: String,
    /// Index of the entry in the manifest's `content_scripts`.
    pub index: usize,
    pub run_at: RunAt,
    pub world: World,
    pub js: Vec<String>,
    pub css: Vec<String>,
}

/// The scripts of an extension that apply to a frame, optionally only those of one `run_at` phase.
pub fn scripts_for(extension_id: &str, manifest: &Manifest, frame: &FrameContext, phase: Option<RunAt>) -> Vec<ScriptPlan> {
    manifest
        .content_scripts
        .iter()
        .enumerate()
        .filter(|(_, cs)| phase.is_none_or(|p| p == cs.run_at) && applies(cs, frame))
        .map(|(index, cs)| ScriptPlan { extension_id: extension_id.into(), index, run_at: cs.run_at, world: cs.world, js: cs.js.clone(), css: cs.css.clone() })
        .collect()
}

/// A script file read from the extension folder.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScript {
    pub name: String,
    pub source: String,
}

impl ScriptPlan {
    /// Reads the files of the plan from the extension folder (files that cannot be read are skipped, like Chrome).
    pub fn load(&self, dir: &Path) -> (Vec<LoadedScript>, Vec<LoadedScript>) {
        let read = |names: &[String]| -> Vec<LoadedScript> {
            names
                .iter()
                .filter_map(|n| {
                    let path = crate::urlmatch::extension_file_path(dir, &self.extension_id, n)?;
                    Some(LoadedScript { name: n.clone(), source: fs::read_to_string(path).ok()? })
                })
                .collect()
        };
        (read(&self.js), read(&self.css))
    }

    /// The JS text to execute in the frame for the script files: one function scope per group, `chrome` and `browser`
    /// bound to the extension's content-script object (installed on the frame by the content-chrome asset).
    pub fn js_source(&self, scripts: &[LoadedScript]) -> String {
        if scripts.is_empty() {
            return String::new();
        }
        let ext = &self.extension_id;
        let body: String = scripts.iter().map(|s| format!("{}\n//# sourceURL=chrome-extension://{ext}/{}\n", s.source, s.name)).collect::<Vec<_>>().join(";\n");
        if self.world == World::Main {
            // MAIN world: the script shares the page's globals and gets no `chrome` object.
            return format!("(function(){{\n{body}\n}})();");
        }
        format!("(function(chrome, browser){{\n{body}\n}})(window.__signetContent && window.__signetContent[{ext:?}], window.__signetContent && window.__signetContent[{ext:?}]);")
    }

    /// The JS that injects the stylesheets of the plan as `<style>` elements (document_start safe: waits for a root).
    pub fn css_source(&self, styles: &[LoadedScript]) -> String {
        if styles.is_empty() {
            return String::new();
        }
        let css: String = styles.iter().map(|s| s.source.as_str()).collect::<Vec<_>>().join("\n");
        let literal = serde_json::to_string(&css).unwrap_or_else(|_| "\"\"".into());
        format!(
            "(function(){{var css={literal};var add=function(){{var s=document.createElement('style');s.setAttribute('data-signet-extension','{}');s.textContent=css;(document.head||document.documentElement).appendChild(s);}};if(document.documentElement)add();else document.addEventListener('readystatechange',add,{{once:true}});}})();",
            self.extension_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn manifest(scripts: serde_json::Value) -> Manifest {
        Manifest::from_value(json!({ "manifest_version": 3, "name": "T", "version": "1", "content_scripts": scripts })).unwrap()
    }
    fn main(url: &str) -> FrameContext<'_> {
        FrameContext { url, is_main_frame: true, fallback_url: None }
    }

    #[test]
    fn matches_and_excludes() {
        let m = manifest(json!([{ "matches": ["https://*.example.com/*"], "exclude_matches": ["https://private.example.com/*"], "js": ["a.js"] }]));
        assert_eq!(scripts_for(ID, &m, &main("https://www.example.com/x"), None).len(), 1);
        assert!(scripts_for(ID, &m, &main("https://private.example.com/x"), None).is_empty());
        assert!(scripts_for(ID, &m, &main("https://other.org/"), None).is_empty());
    }

    #[test]
    fn subframes_need_all_frames() {
        let m = manifest(json!([{ "matches": ["<all_urls>"], "js": ["a.js"] }, { "matches": ["<all_urls>"], "js": ["b.js"], "all_frames": true }]));
        let sub = FrameContext { url: "https://x.test/", is_main_frame: false, fallback_url: None };
        let plans = scripts_for(ID, &m, &sub, None);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].js, ["b.js"]);
        assert_eq!(scripts_for(ID, &m, &main("https://x.test/"), None).len(), 2);
    }

    #[test]
    fn about_blank_needs_the_flag_and_uses_the_fallback_url() {
        let m = manifest(json!([{ "matches": ["https://x.test/*"], "js": ["a.js"], "all_frames": true, "match_about_blank": true }, { "matches": ["https://x.test/*"], "js": ["b.js"], "all_frames": true }]));
        let blank = FrameContext { url: "about:blank", is_main_frame: false, fallback_url: Some("https://x.test/page") };
        let plans = scripts_for(ID, &m, &blank, None);
        assert_eq!(plans.iter().map(|p| p.js[0].as_str()).collect::<Vec<_>>(), ["a.js"]);
        let orphan = FrameContext { url: "about:blank", is_main_frame: false, fallback_url: None };
        assert!(scripts_for(ID, &m, &orphan, None).is_empty());
    }

    #[test]
    fn globs_refine_the_match() {
        assert!(glob_matches("https://x.test/*/edit", "https://x.test/a/b/edit"));
        assert!(glob_matches("https://x.test/?", "https://x.test/a"));
        assert!(!glob_matches("https://x.test/?", "https://x.test/ab"));
        let m = manifest(json!([{ "matches": ["https://x.test/*"], "include_globs": ["*/docs/*"], "exclude_globs": ["*draft*"], "js": ["a.js"] }]));
        assert_eq!(scripts_for(ID, &m, &main("https://x.test/docs/a"), None).len(), 1);
        assert!(scripts_for(ID, &m, &main("https://x.test/other"), None).is_empty());
        assert!(scripts_for(ID, &m, &main("https://x.test/docs/draft"), None).is_empty());
    }

    #[test]
    fn phases_are_filtered() {
        let m = manifest(json!([{ "matches": ["<all_urls>"], "js": ["s.js"], "run_at": "document_start" }, { "matches": ["<all_urls>"], "js": ["i.js"] }]));
        assert_eq!(scripts_for(ID, &m, &main("https://x.test/"), Some(RunAt::DocumentStart))[0].js, ["s.js"]);
        assert_eq!(scripts_for(ID, &m, &main("https://x.test/"), Some(RunAt::DocumentIdle))[0].js, ["i.js"]);
        assert!(scripts_for(ID, &m, &main("https://x.test/"), Some(RunAt::DocumentEnd)).is_empty());
    }

    #[test]
    fn plans_load_files_and_build_js() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.js"), "var x = 1;").unwrap();
        fs::write(d.path().join("a.css"), "body{color:red}").unwrap();
        let m = manifest(json!([{ "matches": ["<all_urls>"], "js": ["a.js", "missing.js"], "css": ["a.css"] }, { "matches": ["<all_urls>"], "js": ["a.js"], "world": "MAIN" }]));
        let plans = scripts_for(ID, &m, &main("https://x.test/"), None);
        let (js, css) = plans[0].load(d.path());
        assert_eq!(js.len(), 1, "missing files are skipped");
        let src = plans[0].js_source(&js);
        assert!(src.starts_with("(function(chrome, browser){") && src.contains("var x = 1;") && src.contains(&format!("sourceURL=chrome-extension://{ID}/a.js")));
        assert!(src.contains(&format!("window.__signetContent[\"{ID}\"]")));
        let main_src = plans[1].js_source(&plans[1].load(d.path()).0);
        assert!(main_src.starts_with("(function(){") && !main_src.contains("__signetContent"));
        let style = plans[0].css_source(&css);
        assert!(style.contains("body{color:red}") && style.contains("createElement('style')"));
        assert!(plans[0].js_source(&[]).is_empty() && plans[0].css_source(&[]).is_empty());
    }
}
