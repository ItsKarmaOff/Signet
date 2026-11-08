//! Native popup menus: page context menu (`tabs.ts`), tab, space, folder, group and bookmark menus
//! (`ipc/tabs.ts` handlers). The menu is shown by the window manager (`Cmd::Popup`); the choice comes back as an id.

use signet_core::model::locate;
use signet_core::types::{Item, PaletteMode, Zone};
use signet_web::{ContextMenuInfo, EditCommand};

use super::Core;
use crate::windows::{Cmd, Entry};

fn t(key: &str) -> String {
    signet_core::i18n::t(key)
}

fn item(label: impl Into<String>, id: &str) -> Entry {
    Entry::Item { label: label.into(), id: id.to_string(), enabled: true, checked: false }
}

fn disabled(label: impl Into<String>, id: &str) -> Entry {
    Entry::Item { label: label.into(), id: id.to_string(), enabled: false, checked: false }
}

impl Core {
    /// Shows a popup at (`x`, `y`) in window coordinates; `cb` receives the chosen id.
    pub fn popup(&mut self, entries: Vec<Entry>, at: (f64, f64), cb: impl FnOnce(&mut Core, &str) + 'static) {
        self.send(Cmd::Popup { entries, at, cb: Box::new(cb) });
    }

    /// Window position of a point given in a pane's page coordinates.
    fn page_point(&self, tab: &str, x: i32, y: i32) -> (f64, f64) {
        let origin = self.panes.iter().find(|p| p.tab_id == tab).map(|p| (p.rect.x, p.rect.y)).unwrap_or((0.0, 0.0));
        (origin.0 + f64::from(x), origin.1 + f64::from(y))
    }

    pub fn show_page_context_menu(&mut self, tab: &str, info: ContextMenuInfo) {
        let mut e: Vec<Entry> = Vec::new();
        let link = info.link_url.clone();
        let image = info.image_url.clone();
        if let Some(l) = &link {
            e.push(item(t("menu.page.openInNewTab"), "link:new"));
            if self.model(|m| m.can_split()) {
                e.push(item(t("menu.page.openInSplit"), "link:split"));
            }
            e.push(item(t("menu.page.openInPrivate"), "link:private"));
            e.push(item(t("menu.page.copyLink"), "link:copy"));
            let _ = l;
            e.push(Entry::Sep);
        }
        if image.is_some() {
            e.push(item(t("menu.page.openImageInNewTab"), "img:new"));
            e.push(item(t("menu.page.copyImage"), "img:copy"));
            e.push(item(t("menu.page.saveImage"), "img:save"));
            e.push(Entry::Sep);
        }
        if let Some(word) = &info.misspelled_word {
            for s in info.suggestions.iter().take(5) {
                e.push(item(s.clone(), &format!("spell:{s}")));
            }
            if info.suggestions.is_empty() {
                e.push(disabled(t("menu.page.noSuggestions"), "none"));
            }
            let _ = word;
            e.push(Entry::Sep);
        }
        if info.editable {
            e.push(Entry::Item { label: t("menu.edit.undo"), id: "edit:undo".into(), enabled: info.can_undo, checked: false });
            e.push(Entry::Item { label: t("menu.edit.redo"), id: "edit:redo".into(), enabled: info.can_redo, checked: false });
            e.push(Entry::Sep);
            e.push(Entry::Item { label: t("menu.edit.cut"), id: "edit:cut".into(), enabled: info.can_cut, checked: false });
            e.push(Entry::Item { label: t("menu.edit.copy"), id: "edit:copy".into(), enabled: info.can_copy, checked: false });
            e.push(Entry::Item { label: t("menu.edit.paste"), id: "edit:paste".into(), enabled: info.can_paste, checked: false });
            e.push(Entry::Item { label: t("menu.edit.selectAll"), id: "edit:all".into(), enabled: info.can_select_all, checked: false });
        } else if info.has_selection && !info.selection_text.trim().is_empty() {
            e.push(item(t("menu.edit.copy"), "edit:copy"));
            let text: String = info.selection_text.trim().chars().take(30).collect();
            e.push(item(signet_core::i18n::t_args("menu.page.search", &[("text", &text)]), "sel:search"));
        } else if link.is_none() && image.is_none() {
            let (back, fwd) = self.views.view_of(tab).map(|v| (v.can_go_back(), v.can_go_forward())).unwrap_or((false, false));
            e.push(Entry::Item { label: t("menu.page.back"), id: "nav:back".into(), enabled: back, checked: false });
            e.push(Entry::Item { label: t("menu.page.forward"), id: "nav:forward".into(), enabled: fwd, checked: false });
            e.push(item(t("menu.page.reload"), "nav:reload"));
            e.push(Entry::Sep);
            e.push(item(t("menu.page.print"), "page:print"));
            e.push(item(t("menu.page.viewSource"), "page:source"));
        }
        e.push(Entry::Sep);
        e.push(item(t("menu.page.inspect"), "inspect"));
        let ext_entries = self.extension_context_entries(&info);
        if !ext_entries.is_empty() {
            e.push(Entry::Sep);
            e.extend(ext_entries);
        }
        while matches!(e.first(), Some(Entry::Sep)) {
            e.remove(0);
        }
        let at = self.page_point(tab, info.x, info.y);
        let tab = tab.to_string();
        self.popup(e, at, move |c, id| {
            if c.extension_context_run(id, &tab, &info) {
                return;
            }
            let view = c.views.view_of(&tab);
            match id {
                "link:new" => {
                    if let Some(l) = &link {
                        c.open_tab(l, signet_core::model::OpenOptions { activate: true, opener_id: Some(tab.clone()), ..Default::default() });
                    }
                }
                "link:split" => {
                    if let Some(l) = &link {
                        c.open_in_split(l);
                    }
                }
                "link:private" => {
                    if let Some(l) = &link {
                        c.open_private_url(l);
                    }
                }
                "link:copy" => {
                    if let Some(l) = &link {
                        crate::app::platform::copy_text(l);
                    }
                }
                "img:new" => {
                    if let Some(l) = &image {
                        c.open_tab(l, signet_core::model::OpenOptions { activate: true, opener_id: Some(tab.clone()), ..Default::default() });
                    }
                }
                "img:copy" => {
                    if let Some(l) = &image {
                        crate::app::platform::copy_text(l);
                    }
                }
                "img:save" => {
                    if let (Some(l), Some(v)) = (&image, view) {
                        v.start_download(l);
                    }
                }
                "edit:undo" | "edit:redo" | "edit:cut" | "edit:copy" | "edit:paste" | "edit:all" => {
                    if let Some(v) = view {
                        v.edit(match id {
                            "edit:undo" => EditCommand::Undo,
                            "edit:redo" => EditCommand::Redo,
                            "edit:cut" => EditCommand::Cut,
                            "edit:copy" => EditCommand::Copy,
                            "edit:paste" => EditCommand::Paste,
                            _ => EditCommand::SelectAll,
                        });
                    }
                }
                "sel:search" => {
                    let url = c.search.engine_search_url(info.selection_text.trim());
                    c.open_tab(&url, signet_core::model::OpenOptions { activate: true, opener_id: Some(tab.clone()), ..Default::default() });
                }
                "nav:back" => c.go_back(Some(&tab)),
                "nav:forward" => c.go_forward(Some(&tab)),
                "nav:reload" => c.reload_tab(Some(&tab), false),
                "page:print" => {
                    if let Some(v) = view {
                        v.print();
                    }
                }
                "page:source" => {
                    if let Some(v) = view {
                        v.view_source();
                    }
                }
                "inspect" => c.inspect_at(&tab, info.x, info.y),
                other => {
                    if let Some(w) = other.strip_prefix("spell:")
                        && let Some(v) = view
                    {
                        v.replace_misspelling(w);
                    }
                }
            }
        });
    }

    /// Tab, space, folder, group and bookmark menus open at the pointer (Electron's `Menu.popup()` default).
    fn menu_origin(&self) -> (f64, f64) {
        (f64::NAN, f64::NAN)
    }

    pub fn show_tab_menu(&mut self, tab_id: &str) {
        let Some(loc) = locate(self.state(), tab_id) else { return };
        let Some(tab) = self.state().tabs.get(tab_id).cloned() else { return };
        let mut e = Vec::new();
        let favorite = loc.zone == Zone::Favorites;
        let pinned = loc.zone == Zone::Pinned;
        e.push(item(t(if pinned || favorite { "menu.tab.unpin" } else { "menu.tab.pin" }), "pin"));
        e.push(item(t(if favorite { "menu.tab.removeFavorite" } else { "menu.tab.addFavorite" }), "favorite"));
        if matches!(loc.item, Item::Split(_)) {
            e.push(item(t("menu.tab.removeFromSplit"), "unsplit-tab"));
        } else if self.active_tab_id().as_deref() != Some(tab_id) && !favorite {
            e.push(item(t("menu.tab.splitWithActive"), "split"));
        }
        if loc.item.group().is_some() {
            e.push(item(t("menu.tab.leaveGroup"), "leave-group"));
        }
        e.push(Entry::Sep);
        e.push(item(t("menu.tab.duplicate"), "duplicate"));
        e.push(item(t("menu.tab.copyUrl"), "copy"));
        e.extend(self.link_menu_entries(Some(tab_id)));
        e.push(item(t("menu.tab.moveUp"), "up"));
        e.push(item(t("menu.tab.moveDown"), "down"));
        let others: Vec<Entry> = self.state().spaces.iter().filter(|s| Some(&s.id) != loc.space.and_then(|i| self.state().spaces.get(i)).map(|s| &s.id)).map(|s| item(format!("{} {}", s.icon, s.name), &format!("space:{}", s.id))).collect();
        if !others.is_empty() {
            e.push(Entry::Sub { label: t("menu.tab.moveToSpace"), items: others });
        }
        e.push(Entry::Sep);
        e.push(item(t("menu.tab.rename"), "rename"));
        e.push(item(t("menu.tab.chooseIcon"), "icon"));
        if tab.custom_title.is_some() || tab.custom_icon.is_some() {
            e.push(item(t("menu.tab.resetNameAndIcon"), "reset"));
        }
        e.push(Entry::Sep);
        if tab.loaded && (pinned || favorite) {
            e.push(item(t("menu.tab.unload"), "unload"));
        }
        e.push(item(t("menu.tab.close"), "close"));
        let id = tab_id.to_string();
        self.popup(e, self.menu_origin(), move |c, choice| match choice {
            "pin" => c.pin_toggle(&id),
            "favorite" => c.favorite_toggle(&id),
            "unsplit-tab" => c.remove_from_split(&id),
            "split" => c.split_active_with(&id, None),
            "leave-group" => c.leave_group(&id),
            "duplicate" => c.duplicate_tab(&id),
            "copy" => c.copy_url(Some(&id)),
            "up" => c.move_tab_step(&id, -1),
            "down" => c.move_tab_step(&id, 1),
            "rename" => c.emit(signet_core::api::UiEvent::RenameTab { tab_id: id.clone() }),
            "icon" => c.emit(signet_core::api::UiEvent::PickIcon(signet_core::types::IconTarget::Tab { tab_id: id.clone() })),
            "reset" => {
                if let Some(t) = c.store.data.state.tabs.get_mut(&id) {
                    t.custom_title = None;
                    t.custom_icon = None;
                }
                c.changed();
            }
            "unload" => c.unload_tab(&id),
            "close" => c.close_tab(&id),
            other => {
                if c.link_menu_run(Some(&id), other) {
                    return;
                }
                if let Some(space) = other.strip_prefix("space:") {
                    if c.model(|m| m.move_tab_to_space(&id, space)) {
                        c.changed();
                    }
                }
            }
        });
    }

    pub fn show_space_menu(&mut self, space_id: &str) {
        let Some(space) = self.state().spaces.iter().find(|s| s.id == space_id).cloned() else { return };
        let idx = self.state().spaces.iter().position(|s| s.id == space_id).unwrap_or(0);
        let last = self.state().spaces.len() - 1;
        let mut e = vec![item(t("menu.space.edit"), "edit")];
        e.push(Entry::Item { label: t("menu.space.moveLeft"), id: "left".into(), enabled: idx > 0, checked: false });
        e.push(Entry::Item { label: t("menu.space.moveRight"), id: "right".into(), enabled: idx < last, checked: false });
        e.push(Entry::Item { label: t("menu.space.isolatedProfile"), id: "isolated".into(), enabled: true, checked: space.is_isolated() });
        e.push(Entry::Sep);
        e.push(Entry::Item { label: t("menu.space.delete"), id: "delete".into(), enabled: self.state().spaces.len() > 1, checked: false });
        let id = space_id.to_string();
        self.popup(e, self.menu_origin(), move |c, choice| match choice {
            "edit" => c.edit_space(Some(id.clone())),
            "left" => c.move_space(&id, -1),
            "right" => c.move_space(&id, 1),
            "isolated" => c.set_space_profile(&id, if space.is_isolated() { signet_core::types::SpaceProfile::Shared } else { signet_core::types::SpaceProfile::Isolated }),
            "delete" => c.delete_space(&id),
            _ => {}
        });
    }

    pub fn show_folder_menu(&mut self, folder_id: &str) {
        let e = vec![item(t("menu.folder.rename"), "rename"), item(t("menu.folder.newInside"), "inside"), Entry::Sep, item(t("menu.folder.unpin"), "unpin"), item(t("menu.folder.delete"), "delete"), item(t("menu.folder.deleteWithTabs"), "delete-tabs")];
        let id = folder_id.to_string();
        self.popup(e, self.menu_origin(), move |c, choice| match choice {
            "rename" => c.request_rename(&id),
            "inside" => {
                let f = c.model(|m| m.create_folder(Some(&id)));
                c.changed();
                c.request_rename(&f.id);
            }
            "unpin" => {
                if c.model(|m| m.unpin_folder(&id, None)) {
                    c.changed();
                }
            }
            "delete" => c.delete_folder(&id, false),
            "delete-tabs" => c.delete_folder(&id, true),
            _ => {}
        });
    }

    pub fn show_group_menu(&mut self, group_id: &str) {
        let open = self.active_space().groups.as_ref().and_then(|g| g.get(group_id)).is_none_or(|g| g.open);
        let e = vec![
            item(t("menu.group.rename"), "rename"),
            item(t(if open { "menu.group.collapse" } else { "menu.group.expand" }), "toggle"),
            item(t("menu.group.pin"), "pin"),
            item(t("menu.group.ungroup"), "ungroup"),
            Entry::Sep,
            item(t("menu.group.close"), "close"),
        ];
        let id = group_id.to_string();
        self.popup(e, self.menu_origin(), move |c, choice| match choice {
            "rename" => c.request_rename(&id),
            "toggle" => c.toggle_group(&id, None),
            "pin" => {
                if c.model(|m| m.pin_group(&id, None)) {
                    c.changed();
                }
            }
            "ungroup" => c.ungroup(&id),
            "close" => c.close_group(&id),
            _ => {}
        });
    }

    pub fn show_bookmark_menu(&mut self, id: &str, tab_id: Option<&str>) {
        let e = vec![item(t("menu.bookmark.open"), "open"), item(t("menu.bookmark.openInNewTab"), "new"), item(t("menu.bookmark.rename"), "rename"), Entry::Sep, item(t("menu.bookmark.delete"), "delete")];
        let (bid, tab) = (id.to_string(), tab_id.map(str::to_string));
        self.popup(e, self.menu_origin(), move |c, choice| match choice {
            "open" => c.open_bookmark(&bid, tab.as_deref(), false),
            "new" => c.open_bookmark(&bid, None, true),
            "rename" => c.emit(signet_core::api::UiEvent::RenameBookmark { id: bid.clone() }),
            "delete" => c.remove_bookmark(&bid),
            _ => {}
        });
    }

    pub fn show_bookmarks_bar_menu(&mut self) {
        let hidden = !self.bookmarks_visible();
        let e = vec![item(t("menu.bookmark.newFolder"), "folder"), item(t(if hidden { "menu.view.showBookmarksBar" } else { "menu.view.hideBookmarksBar" }), "bar")];
        self.popup(e, self.menu_origin(), |c, choice| match choice {
            "folder" => c.new_bookmark_folder(),
            "bar" => c.toggle_bookmarks_bar(),
            _ => {}
        });
    }

    /// "New" menu (right click on "New tab"): tab, note, Easel.
    pub fn show_new_menu(&mut self) {
        let e = vec![item(t("menu.file.newTab"), "tab"), item(t("menu.file.newNote"), "note"), item(t("menu.file.newEasel"), "easel")];
        self.popup(e, self.menu_origin(), |c, choice| match choice {
            "tab" => c.open_palette(PaletteMode::NewTab, None),
            "note" => c.create_note("note"),
            "easel" => c.create_note("easel"),
            _ => {}
        });
    }

    /// "New folder" button: plain folder or live folder.
    pub fn show_add_folder_menu(&mut self) {
        let e = vec![item(t("menu.folder.newFolder"), "folder"), item(t("menu.folder.newLiveFolder"), "live")];
        self.popup(e, self.menu_origin(), |c, choice| match choice {
            "folder" => c.create_folder(),
            "live" => c.open_popover_kind(signet_core::types::PopoverRequest::LiveFolder { folder_id: None }),
            _ => {}
        });
    }
}
