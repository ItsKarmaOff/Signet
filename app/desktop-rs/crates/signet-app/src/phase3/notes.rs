//! PHASE3: notes and Easels (`signet_features::notes`, `signet_pages`): storage, `notes:*` page channels, create_note.
//!
//! Port of the page channels of `main/features/notes/notes.ts`. The pages `signet://note/<id>` and
//! `signet://easel/<id>` are served by `signet-pages`; their script only calls `notes:*`, and the document is the one
//! named by the sender frame's address (checked by `signet_scripts::validate`, never by the payload).
//!
//! Saves and removals go through one writer thread (a 30 MiB Easel must not block the interface); a `load` waits for
//! the queue so it never reads a document older than the last save.

use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};
use signet_features::notes::{self as store, NoteKind, Notes, NotesDoc};
use signet_scripts::notes as wire;
use signet_scripts::validate::NotesOp;
use signet_scripts::Reply;

use super::pages::PageCtx;
use crate::browser::Core;

/// Work for the writer thread.
enum Job {
    Save { kind: NoteKind, id: String, patch: store::NotesPatch },
    Remove(String),
    /// Answered once every job queued before it is done.
    Flush(Sender<()>),
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    writer: Option<(Sender<Job>, JoinHandle<()>)>,
}

impl State {
    fn queue(&mut self, data_dir: &Path, job: Job) -> bool {
        if self.writer.is_none() {
            let notes = Notes::new(data_dir);
            let (tx, rx) = channel::<Job>();
            let handle = std::thread::Builder::new().name("signet-notes".into()).spawn(move || {
                while let Ok(job) = rx.recv() {
                    run_job(&notes, job);
                }
            });
            match handle {
                Ok(h) => self.writer = Some((tx, h)),
                Err(_) => return false,
            }
        }
        self.writer.as_ref().is_some_and(|(tx, _)| tx.send(job).is_ok())
    }

    /// Waits (bounded) until the queue is empty.
    fn flush(&mut self, data_dir: &Path) {
        if self.writer.is_none() {
            return;
        }
        let (tx, rx) = channel();
        if self.queue(data_dir, Job::Flush(tx)) {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    }
}

fn run_job(notes: &Notes, job: Job) {
    match job {
        Job::Save { kind, id, patch } => {
            let _ = notes.save(kind, &id, &patch);
        }
        Job::Remove(id) => notes.remove(&id),
        Job::Flush(done) => {
            let _ = done.send(());
        }
    }
}

/// `signet-scripts` kind to `signet-features` kind.
fn kind_of(kind: wire::NoteKind) -> NoteKind {
    match kind {
        wire::NoteKind::Note => NoteKind::Note,
        wire::NoteKind::Easel => NoteKind::Easel,
    }
}

/// A save validated by `signet-scripts` to the shape `Notes::save` takes.
fn patch_of(patch: &wire::NotesPatch) -> store::NotesPatch {
    match patch {
        wire::NotesPatch::Note { title, html } => store::NotesPatch::Note { title: title.clone(), html: html.clone() },
        wire::NotesPatch::Easel { title, items, view } => store::NotesPatch::Easel {
            title: title.clone(),
            items: serde_json::to_value(items).unwrap_or(Value::Array(vec![])),
            view: serde_json::to_value(view).ok(),
        },
    }
}

/// Answer of `notes:load`: `{ kind, id, doc, hue }`; `doc` is null when missing or of the other kind.
fn env_of(kind: NoteKind, id: &str, doc: Option<NotesDoc>, hue: f64) -> Value {
    let doc = doc.filter(|d| d.kind() == kind);
    json!({ "kind": kind.as_str(), "id": id, "doc": doc, "hue": hue })
}

fn parse_kind(kind: &str) -> Option<NoteKind> {
    match kind {
        "note" => Some(NoteKind::Note),
        "easel" => Some(NoteKind::Easel),
        _ => None,
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(_core: &mut Core) {}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    let dir = core.data_dir.clone();
    core.phase3.notes.flush(&dir);
    if let Some((tx, handle)) = core.phase3.notes.writer.take() {
        drop(tx);
        let _ = handle.join();
    }
}

impl Core {
    /// Creates an empty note or Easel document on disk and returns its tab address (`signet://note/<id>` or
    /// `signet://easel/<id>`), or `None` for an unknown kind or a write error.
    ///
    /// `kind` is `"note"` or `"easel"`. The caller (`windows.rs`, `create_note`) opens the address in a new active tab
    /// of the active space, as `Notes.create` did in Electron:
    ///
    /// ```ignore
    /// if let Some(url) = self.create_note_doc(kind) {
    ///     self.open_tab(&url, OpenOptions { activate: true, ..Default::default() });
    /// }
    /// ```
    pub fn create_note_doc(&mut self, kind: &str) -> Option<String> {
        let kind = parse_kind(kind)?;
        let notes = Notes::new(&self.data_dir);
        match notes.create(kind) {
            Ok(id) => Some(store::note_address(kind, &id)),
            Err(e) => {
                self.log(format!("notes: cannot create a document: {e}"));
                None
            }
        }
    }

    /// New Easel holding an image (capture tool), scaled down to 900 px wide at most. Returns the tab address like
    /// [`Core::create_note_doc`].
    pub fn create_easel_with_image(&mut self, png_data_url: &str, width: f64, height: f64) -> Option<String> {
        let notes = Notes::new(&self.data_dir);
        match notes.create_easel_with_image(png_data_url, width, height) {
            Ok(id) => Some(store::note_address(NoteKind::Easel, &id)),
            Err(e) => {
                self.log(format!("notes: cannot create an Easel: {e}"));
                None
            }
        }
    }

    /// Page channels `notes:*` (accepted by `signet_scripts::validate`). Answer requests with `self.page_reply(ctx, ..)`.
    pub fn page_notes(&mut self, ctx: &PageCtx, accepted: signet_scripts::Accepted) {
        let signet_scripts::Accepted::Notes { doc, op } = accepted else { return };
        let Some(rid) = ctx.rid.clone() else { return };
        let kind = kind_of(doc.kind);
        let id = doc.id;
        match op {
            NotesOp::Locale => {
                let locale = self.locale().id().to_owned();
                self.page_reply(ctx, Reply::ok(&rid, &locale));
            }
            NotesOp::Appearance => {
                let look = self.appearance();
                self.page_reply(ctx, Reply::ok(&rid, &look));
            }
            NotesOp::Load => {
                let dir = self.data_dir.clone();
                self.phase3.notes.flush(&dir);
                let hue = self.model(|m| m.locate(&ctx.tab)).and_then(|l| l.space).and_then(|i| self.state().spaces.get(i).map(|s| s.hue)).unwrap_or_else(|| self.hue());
                let env = env_of(kind, &id, Notes::new(&dir).read(&id), hue);
                self.page_reply(ctx, Reply::ok(&rid, &env));
            }
            NotesOp::Save(patch) => {
                let dir = self.data_dir.clone();
                let ok = self.phase3.notes.queue(&dir, Job::Save { kind, id, patch: patch_of(&patch) });
                self.page_reply(ctx, Reply::ok(&rid, &ok));
            }
            NotesOp::Remove => {
                let dir = self.data_dir.clone();
                self.phase3.notes.queue(&dir, Job::Remove(id.clone()));
                self.page_reply(ctx, Reply::ok(&rid, &Value::Null));
                let tabs: Vec<String> = self
                    .state()
                    .tabs
                    .values()
                    .filter(|t| store::parse_note_address(&t.url).is_some_and(|(k, i)| k == kind && i == id))
                    .map(|t| t.id.clone())
                    .collect();
                for tab in tabs {
                    self.close_tab(&tab);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_scripts::notes::{EaselItem, EaselView};

    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn temp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("signet-notes-test-{}", signet_core::ids::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn kinds_and_patches_convert() {
        assert_eq!(kind_of(wire::NoteKind::Note), NoteKind::Note);
        assert_eq!(kind_of(wire::NoteKind::Easel), NoteKind::Easel);
        let note = patch_of(&wire::NotesPatch::Note { title: "t".into(), html: "<p>x</p>".into() });
        assert!(matches!(note, store::NotesPatch::Note { ref title, ref html } if title == "t" && html == "<p>x</p>"));
        let item = EaselItem { id: "a".into(), kind: "text".into(), x: 1.0, y: 2.0, w: 3.0, h: 4.0, color: None, html: Some("hi".into()), src: None, points: None, vw: None, vh: None, size: None };
        let easel = patch_of(&wire::NotesPatch::Easel { title: "e".into(), items: vec![item], view: EaselView { x: 5.0, y: 6.0, zoom: 2.0 } });
        let store::NotesPatch::Easel { items, view, .. } = easel else { panic!("easel expected") };
        assert_eq!(items[0]["type"], "text");
        assert_eq!(view.unwrap()["zoom"], 2.0);
    }

    #[test]
    fn load_answer_hides_a_document_of_the_other_kind() {
        let dir = temp();
        let notes = Notes::new(&dir);
        let id = notes.create(NoteKind::Note).unwrap();
        let doc = notes.read(&id);
        assert!(doc.is_some());
        let env = env_of(NoteKind::Note, &id, doc.clone(), 200.0);
        assert_eq!(env["kind"], "note");
        assert_eq!(env["doc"]["kind"], "note");
        assert_eq!(env["hue"], 200.0);
        assert_eq!(env_of(NoteKind::Easel, &id, doc, 1.0)["doc"], Value::Null);
        assert_eq!(env_of(NoteKind::Note, ID, None, 1.0)["doc"], Value::Null);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn writer_saves_then_removes_in_order() {
        let dir = temp();
        let mut st = State::default();
        let id = Notes::new(&dir).create(NoteKind::Note).unwrap();
        let patch = store::NotesPatch::Note { title: "  Title ".into(), html: "<p>a</p>".into() };
        assert!(st.queue(&dir, Job::Save { kind: NoteKind::Note, id: id.clone(), patch }));
        st.flush(&dir);
        match Notes::new(&dir).read(&id) {
            Some(NotesDoc::Note { title, html, .. }) => {
                assert_eq!(title, "Title");
                assert_eq!(html, "<p>a</p>");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(st.queue(&dir, Job::Remove(id.clone())));
        st.flush(&dir);
        assert!(Notes::new(&dir).read(&id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn kind_names() {
        assert_eq!(parse_kind("note"), Some(NoteKind::Note));
        assert_eq!(parse_kind("easel"), Some(NoteKind::Easel));
        assert_eq!(parse_kind("x"), None);
    }
}
