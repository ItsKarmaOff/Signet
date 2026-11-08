//! Port of `browser/undo.ts`: one undoable announcement at a time (archived tabs, closed tabs, deleted space).

use signet_core::api::{ArchiveKind, ArchivedNotice, UiEvent};

use super::{Core, UndoAction};
use crate::windows::{Cmd, Later};

/// How long an announcement can be undone (ms).
pub const UNDO_MS: u64 = 8000;

impl Core {
    pub fn announce(&mut self, kind: ArchiveKind, count: usize, name: String, action: UndoAction) {
        // The previous announcement is committed (its window is over).
        self.commit_notice();
        self.notices.next += 1;
        let id = self.notices.next;
        let notice = ArchivedNotice { id, kind, count, name };
        self.notices.current = Some((notice.clone(), action));
        self.send(Cmd::Later(UNDO_MS, Later::UndoExpire(id)));
        self.emit(UiEvent::Archived(notice));
        // Without the sidebar the notice shows in the toast window.
        self.changed_view();
    }

    /// Commits the pending announcement (nothing left to undo).
    fn commit_notice(&mut self) {
        if let Some((notice, UndoAction::Space(_))) = self.notices.current.take() {
            self.log(format!("space deletion {} committed", notice.name));
        }
    }

    pub fn undo(&mut self, id: u64) {
        let Some((n, action)) = self.notices.current.take() else { return };
        if n.id != id {
            self.notices.current = Some((n, action));
            return;
        }
        let ok = match action {
            UndoAction::Archive(u) => self.model(|m| m.restore_archive(&u)),
            UndoAction::Removed(u) => self.model(|m| m.restore_removed(&u)),
            UndoAction::Space(u) => self.model(|m| m.restore_space(&u)),
        };
        if ok {
            self.store.schedule_bookmarks();
            self.request_focus();
            self.changed();
        }
    }

    /// The undo window of announcement `id` has passed.
    pub fn undo_expired(&mut self, id: u64) {
        if self.notices.current.as_ref().is_some_and(|(n, _)| n.id == id) {
            self.commit_notice();
            self.emit(UiEvent::UndoExpired(id));
            self.changed_view();
        }
    }

    /// The toast window finished its exit animation.
    pub fn toast_done(&mut self) {
        self.changed_view();
    }
}
