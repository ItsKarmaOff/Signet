//! Settings > Data > Import (port of `Import` and `ImportPicker` of `Settings.tsx`).

use gpui::AppContext as _;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    Styled, Window, div,
};
use signet_core::types::{ImportKind, ImportSourceInfo};

use super::kit::{self, Btn};
use super::window::{InfoStore, ModalEvent, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::theme::{ThemeExt, radius, space, weight};
use signet_core::i18n::IS_MAC;

pub fn kind_key(kind: ImportKind) -> &'static str {
    match kind {
        ImportKind::Spaces => "spaces",
        ImportKind::Bookmarks => "bookmarks",
        ImportKind::Passwords => "passwords",
        ImportKind::Extensions => "extensions",
        ImportKind::History => "history",
    }
}

pub struct Import {
    store: Entity<InfoStore>,
    busy: Option<String>,
    choosing: Option<String>,
    failed: bool,
}

impl EventEmitter<ModalEvent> for Import {}

impl Import {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store, busy: None, choosing: None, failed: false }
    }

    fn run(&mut self, id: &str, kinds: Vec<ImportKind>, cx: &mut Context<Self>) {
        self.choosing = None;
        cx.emit(ModalEvent::Hide);
        self.busy = Some(id.to_string());
        self.failed = false;
        // The app shows the summary in a dialog; the call returns when the import is done.
        api(cx).run_import(id, &kinds);
        self.busy = None;
        cx.notify();
    }

    fn choose(&mut self, source: ImportSourceInfo, window: &mut Window, cx: &mut Context<Self>) {
        self.choosing = Some(source.id.clone());
        let this = cx.entity();
        let picker = cx.new(|cx| ImportPicker::new(source.clone(), window, cx));
        let id = source.id.clone();
        cx.subscribe(&picker, move |this, _, ev: &PickerDone, cx| match &ev.0 {
            Some(kinds) => this.run(&id, kinds.clone(), cx),
            None => {
                this.choosing = None;
                cx.emit(ModalEvent::Hide);
                cx.notify();
            }
        })
        .detach();
        let _ = this;
        cx.emit(ModalEvent::Show(picker.into()));
        cx.notify();
    }
}

impl Render for Import {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let rows: Vec<gpui::AnyElement> = info
            .import_sources
            .iter()
            .enumerate()
            .map(|(i, src)| {
                let src = src.clone();
                let importing = self.busy.as_deref() == Some(src.id.as_str());
                let control = if src.available {
                    let for_click = src.clone();
                    Btn::new(("import", i), if importing { tr("settings.import.importing") } else { tr("settings.import.run") })
                        .disabled(self.busy.is_some())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // Only one data item (Safari): no choice to make.
                            if for_click.kinds.len() > 1 {
                                this.choose(for_click.clone(), window, cx)
                            } else {
                                this.run(&for_click.id, for_click.kinds.clone(), cx)
                            }
                        }))
                        .into_any_element()
                } else {
                    kit::hint_text(cx, tr("settings.import.unavailable")).into_any_element()
                };
                kit::row(cx).child(div().flex_1().child(src.title.clone())).child(control).into_any_element()
            })
            .collect();
        kit::body()
            .child(kit::group(cx, rows))
            .when_some(self.failed.then(|| tr("settings.import.failed")), |d, m| d.child(kit::note_err(cx, m)))
            .when(IS_MAC, |d| d.child(kit::note(cx, tr("settings.import.note"))))
    }
}

use gpui::prelude::FluentBuilder;

// --------------------------------------------------------------------------- picker

/// Result of the picker: the chosen kinds, or None if cancelled.
pub struct PickerDone(pub Option<Vec<ImportKind>>);

/// Window for choosing the data to import from a source: everything checked at the start.
pub struct ImportPicker {
    source: ImportSourceInfo,
    picked: Vec<ImportKind>,
    focus: FocusHandle,
}

impl EventEmitter<PickerDone> for ImportPicker {}

impl Focusable for ImportPicker {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ImportPicker {
    fn new(source: ImportSourceInfo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { picked: source.kinds.clone(), source, focus }
    }

    fn toggle(&mut self, kind: ImportKind, cx: &mut Context<Self>) {
        if let Some(i) = self.picked.iter().position(|k| *k == kind) {
            self.picked.remove(i);
        } else {
            self.picked.push(kind);
        }
        cx.notify();
    }
}

impl Render for ImportPicker {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let rows: Vec<gpui::AnyElement> = self
            .source
            .kinds
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let kind = *k;
                let hint = match kind {
                    ImportKind::Passwords => Some(tr("settings.import.passwordsHint")),
                    ImportKind::Extensions => Some(tr("settings.import.extensionsHint")),
                    _ => None,
                };
                let this = cx.entity();
                kit::check_row(
                    cx,
                    ("import-kind", i),
                    self.picked.contains(&kind),
                    false,
                    tr(&format!("settings.import.kind.{}", kind_key(kind))),
                    hint.map(Into::into),
                    move |_, cx| this.update(cx, |p, cx| p.toggle(kind, cx)),
                )
            })
            .collect();
        let start: Vec<ImportKind> = self.source.kinds.iter().copied().filter(|k| self.picked.contains(k)).collect();
        div()
            .id("import-picker")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p(space::S4)
            .bg(gpui::black().opacity(0.28))
            .on_key_down(cx.listener(|_, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    cx.emit(PickerDone(None));
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.emit(PickerDone(None))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S3)
                    .w(gpui::px(420.0))
                    .max_w_full()
                    .p(space::S5)
                    .rounded(radius::POP)
                    .bg(t.win_bg)
                    .shadow(t.shadow_modal.clone())
                    .text_size(t.type_scale.base)
                    .text_color(t.text)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(t.type_scale.lg)
                            .font_weight(weight::STRONG)
                            .child(tr_args("settings.import.choose", &[("name", &self.source.title)])),
                    )
                    .child(kit::note(cx, tr("settings.import.chooseHint")))
                    .child(kit::group(cx, rows))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(space::S2)
                            .mt(space::S1)
                            .child(Btn::new("import-cancel", tr("settings.import.cancel")).on_click(cx.listener(|_, _, _, cx| cx.emit(PickerDone(None)))))
                            .child(
                                Btn::new("import-start", tr("settings.import.start"))
                                    .solid()
                                    .disabled(self.picked.is_empty())
                                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(PickerDone(Some(start.clone()))))),
                            ),
                    ),
            )
    }
}
