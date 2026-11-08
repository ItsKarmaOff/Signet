//! Registry of the extension contexts that live in CEF frames (background page, extension pages, content scripts).
//!
//! A context is created when the page script of a frame is generated (on a helper thread of the browser process, before
//! the page's own scripts run) and looked up on the main thread when a `signet-crx` message arrives. The key of a frame
//! is `(view, frame id)`: both come from the browser side for messages, never from the payload.

use std::collections::HashMap;

use signet_web::ViewId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The hidden background page (service worker emulation).
    Worker,
    /// An extension page: popup, options, a tab, an iframe.
    Page,
    /// A content script group in a web page.
    Content,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Worker => "worker",
            Kind::Page => "page",
            Kind::Content => "content",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ctx {
    /// `<extension id>:<kind>-<n>`: the host derives the extension from the part before the first colon.
    pub id: String,
    pub ext: String,
    pub kind: Kind,
    pub view: ViewId,
    pub frame: String,
    #[allow(dead_code)]
    pub url: String,
}

#[derive(Default)]
pub struct Contexts {
    by_id: HashMap<String, Ctx>,
    counter: u64,
    /// Ids of contexts replaced by a new document in the same frame, to tell the host.
    replaced: Vec<String>,
    /// Contexts that became live and that the host must hear about (`worker_running`).
    opened: Vec<Ctx>,
}

impl Contexts {
    /// Registers a context for a new document. A previous context of the same frame (and extension, for content scripts)
    /// is replaced.
    pub fn open(&mut self, ext: &str, kind: Kind, view: ViewId, frame: &str, url: &str) -> Ctx {
        self.counter += 1;
        let ctx = Ctx { id: format!("{ext}:{}-{}", kind.name(), self.counter), ext: ext.into(), kind, view, frame: frame.into(), url: url.into() };
        let old: Vec<String> = self
            .by_id
            .values()
            .filter(|c| c.view == view && c.frame == frame && ((kind == Kind::Content) == (c.kind == Kind::Content)) && (kind != Kind::Content || c.ext == ext))
            .map(|c| c.id.clone())
            .collect();
        for id in old {
            self.by_id.remove(&id);
            self.replaced.push(id);
        }
        self.by_id.insert(ctx.id.clone(), ctx.clone());
        self.opened.push(ctx.clone());
        ctx
    }

    pub fn get(&self, id: &str) -> Option<&Ctx> {
        self.by_id.get(id)
    }

    /// The extension page or background context of a frame.
    pub fn page_of(&self, view: ViewId, frame: &str) -> Option<&Ctx> {
        self.by_id.values().find(|c| c.view == view && c.frame == frame && c.kind != Kind::Content)
    }

    /// Removes every context of a view (closed) and returns their ids.
    pub fn remove_view(&mut self, view: ViewId) -> Vec<String> {
        let ids: Vec<String> = self.by_id.values().filter(|c| c.view == view).map(|c| c.id.clone()).collect();
        for id in &ids {
            self.by_id.remove(id);
        }
        ids
    }

    /// Removes every context of an extension (unloaded) and returns their ids.
    pub fn remove_ext(&mut self, ext: &str) -> Vec<String> {
        let ids: Vec<String> = self.by_id.values().filter(|c| c.ext == ext).map(|c| c.id.clone()).collect();
        for id in &ids {
            self.by_id.remove(id);
        }
        ids
    }

    /// Live contexts of an extension, background and pages (not content scripts).
    pub fn running(&self, ext: &str) -> Vec<Ctx> {
        self.by_id.values().filter(|c| c.ext == ext && c.kind != Kind::Content).cloned().collect()
    }

    pub fn worker_of(&self, ext: &str) -> Option<Ctx> {
        self.by_id.values().find(|c| c.ext == ext && c.kind == Kind::Worker).cloned()
    }

    pub fn take_replaced(&mut self) -> Vec<String> {
        std::mem::take(&mut self.replaced)
    }

    pub fn take_opened(&mut self) -> Vec<Ctx> {
        std::mem::take(&mut self.opened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn a_new_document_replaces_the_context_of_its_frame() {
        let mut c = Contexts::default();
        let first = c.open(A, Kind::Page, ViewId(1), "f1", "chrome-extension://a/x.html");
        let second = c.open(A, Kind::Page, ViewId(1), "f1", "chrome-extension://a/y.html");
        assert_ne!(first.id, second.id);
        assert!(c.get(&first.id).is_none());
        assert_eq!(c.page_of(ViewId(1), "f1").map(|x| x.id.clone()), Some(second.id));
        assert_eq!(c.take_replaced(), vec![first.id]);
    }

    #[test]
    fn content_scripts_of_two_extensions_share_a_frame() {
        let mut c = Contexts::default();
        let a = c.open(A, Kind::Content, ViewId(2), "f", "https://x.test/");
        let b = c.open(B, Kind::Content, ViewId(2), "f", "https://x.test/");
        assert!(c.get(&a.id).is_some() && c.get(&b.id).is_some());
        assert!(c.page_of(ViewId(2), "f").is_none());
        assert!(c.take_replaced().is_empty());
        // The same extension navigating the frame replaces only its own script.
        let a2 = c.open(A, Kind::Content, ViewId(2), "f", "https://x.test/2");
        assert_eq!(c.take_replaced(), vec![a.id]);
        assert!(c.get(&a2.id).is_some());
    }

    #[test]
    fn ids_start_with_the_extension_id() {
        let mut c = Contexts::default();
        let x = c.open(A, Kind::Worker, ViewId(3), "m", "u");
        assert_eq!(x.id.split(':').next(), Some(A));
        assert_eq!(c.worker_of(A).map(|w| w.id), Some(x.id));
    }

    #[test]
    fn removing_a_view_or_an_extension_forgets_their_contexts() {
        let mut c = Contexts::default();
        c.open(A, Kind::Page, ViewId(1), "f", "u");
        c.open(B, Kind::Page, ViewId(2), "f", "u");
        assert_eq!(c.remove_view(ViewId(1)).len(), 1);
        assert_eq!(c.remove_ext(B).len(), 1);
        assert!(c.running(A).is_empty() && c.running(B).is_empty());
    }
}
