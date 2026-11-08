//! The `SignetApi` implementation (port of `main/ipc/*.ts`): every method locks the core and delegates to the part
//! that owns the behavior. Features of phase 3 are in `crate::phase3`.

use signet_core::api::*;
use signet_core::core::store::SettingsPatch;
use signet_core::i18n::Locale;
use signet_core::types::*;

use super::Browser;

impl SignetApi for Browser {
    fn subscribe(&self, listener: Box<dyn Fn(UiEvent) + Send + Sync>) -> Subscription {
        self.bus.subscribe(listener)
    }

    fn get_state(&self) -> Snapshot {
        self.core.borrow().snapshot()
    }

    // Tabs
    fn activate_tab(&self, tab_id: &str) {
        self.core.borrow_mut().activate_tab(tab_id);
    }
    fn close_tab(&self, tab_id: &str) {
        self.core.borrow_mut().close_tab(tab_id);
    }
    fn navigate(&self, tab_id: &str, input: &str) {
        self.core.borrow_mut().navigate(tab_id, input);
    }
    fn go_back(&self, tab_id: Option<&str>) {
        self.core.borrow_mut().go_back(tab_id);
    }
    fn go_forward(&self, tab_id: Option<&str>) {
        self.core.borrow_mut().go_forward(tab_id);
    }
    fn reload(&self, tab_id: Option<&str>) {
        self.core.borrow_mut().reload_or_stop(tab_id);
    }
    fn toggle_mute(&self, tab_id: &str) {
        self.core.borrow_mut().toggle_mute(tab_id);
    }
    fn reset_pinned(&self, tab_id: &str) {
        self.core.borrow_mut().reset_pinned(tab_id);
    }
    fn toggle_reader(&self, tab_id: &str) {
        self.core.borrow_mut().toggle_reader(tab_id);
    }
    fn zoom_page(&self, tab_id: &str, delta: i32) {
        self.core.borrow_mut().zoom_page(tab_id, delta);
    }
    fn open_zoom(&self, tab_id: &str) {
        self.core.borrow_mut().open_zoom_bubble(tab_id);
    }

    // Live folders
    fn save_live_folder(&self, folder_id: Option<&str>, source: LiveSource) -> Option<String> {
        self.core.borrow_mut().save_live_folder(folder_id, source)
    }
    fn refresh_live_folder(&self, folder_id: &str) {
        self.core.borrow_mut().refresh_live_folder(folder_id);
    }
    fn has_github_token(&self) -> bool {
        self.core.borrow_mut().has_github_token()
    }
    fn set_github_token(&self, token: Option<&str>) -> Option<String> {
        self.core.borrow_mut().set_github_token(token)
    }
    fn show_add_folder_menu(&self) {
        self.core.borrow_mut().show_add_folder_menu();
    }
    fn toggle_translation(&self, tab_id: &str) {
        self.core.borrow_mut().toggle_translation(tab_id);
    }
    fn take_capture(&self, tab_id: &str, target: CaptureTarget, action: CaptureAction) -> CaptureResult {
        self.core.borrow_mut().take_capture(tab_id, target, action)
    }
    fn show_tab_preview(&self, tab_id: &str, rect: Rect, below: bool) {
        self.core.borrow_mut().show_tab_preview(tab_id, rect, below);
    }
    fn hide_tab_preview(&self) {
        self.core.borrow_mut().hide_tab_preview();
    }

    // Mods
    fn list_mods(&self) -> ModsInfo {
        self.core.borrow_mut().list_mods()
    }
    fn save_mod(&self, id: Option<&str>, name: &str, css: &str) -> String {
        self.core.borrow_mut().save_mod(id, name, css)
    }
    fn delete_mod(&self, id: &str) {
        self.core.borrow_mut().delete_mod(id);
    }
    fn set_mod_enabled(&self, id: &str, enabled: bool) {
        self.core.borrow_mut().set_mod_enabled(id, enabled);
    }
    fn move_mod(&self, id: &str, delta: i32) {
        self.core.borrow_mut().move_mod(id, delta);
    }
    fn set_mods_disabled(&self, disabled: bool) {
        self.core.borrow_mut().set_mods_disabled(disabled);
    }
    fn import_mod(&self) -> Option<String> {
        self.core.borrow_mut().import_mod()
    }
    fn export_mod(&self, id: &str) {
        self.core.borrow_mut().export_mod(id);
    }
    fn show_tab_menu(&self, tab_id: &str) {
        self.core.borrow_mut().show_tab_menu(tab_id);
    }
    fn move_tab_step(&self, tab_id: &str, dir: i32) {
        self.core.borrow_mut().move_tab_step(tab_id, dir);
    }
    fn show_new_menu(&self) {
        self.core.borrow_mut().show_new_menu();
    }
    fn move_item(&self, source: DragSource, target: DropTarget) {
        self.core.borrow_mut().move_item(&source, &target);
    }

    // Spaces
    fn switch_space(&self, space_id: &str) {
        self.core.borrow_mut().switch_space(space_id);
    }
    fn create_space(&self, draft: SpaceDraft) {
        self.core.borrow_mut().create_space(&draft);
    }
    fn update_space(&self, space_id: &str, draft: SpaceDraft) {
        self.core.borrow_mut().update_space(space_id, &draft);
    }
    fn show_space_menu(&self, space_id: &str) {
        self.core.borrow_mut().show_space_menu(space_id);
    }

    // Sidebar
    fn toggle_sidebar(&self) {
        self.core.borrow_mut().toggle_sidebar();
    }
    fn set_sidebar_width(&self, width: f64, live: bool) {
        self.core.borrow_mut().set_sidebar_width(width, live);
    }
    fn clear_today(&self) {
        self.core.borrow_mut().clear_today();
    }
    fn undo_archive(&self, id: u64) {
        self.core.borrow_mut().undo(id);
    }

    // Split view
    fn focus_pane(&self, tab_id: &str) {
        self.core.borrow_mut().activate_tab(tab_id);
    }
    fn unsplit(&self, split_id: &str) {
        self.core.borrow_mut().unsplit(split_id);
    }

    // Command bar
    fn open_palette(&self, mode: PaletteMode, anchor: Option<Rect>) {
        self.core.borrow_mut().open_palette(mode, anchor);
    }
    fn find_ready(&self) {
        self.core.borrow_mut().find_ready();
    }

    fn overlay_ready(&self) {
        self.core.borrow_mut().overlay_ready();
    }
    fn close_overlay(&self, instant: bool) {
        self.core.borrow_mut().close_overlay(instant);
    }
    fn toast_done(&self) {
        self.core.borrow_mut().toast_done();
    }
    fn palette_query(&self, query: &str, mode: PaletteMode) -> Vec<PaletteResult> {
        self.core.borrow_mut().palette_query(query, mode)
    }
    fn palette_suggest(&self, query: &str) -> Vec<PaletteResult> {
        self.core.borrow_mut().palette_suggest(query)
    }
    fn palette_execute(&self, result: PaletteResult, mode: PaletteMode) {
        self.core.borrow_mut().palette_execute(result, mode);
    }

    fn expand_peek(&self) {
        self.core.borrow_mut().expand_peek();
    }
    fn open_popover(&self, popover: PopoverRequest, anchor: Rect) {
        self.core.borrow_mut().open_popover(popover, anchor);
    }
    fn set_sidebar_hover(&self, hover: bool) {
        self.core.borrow_mut().set_sidebar_hover(hover);
    }

    // Find
    fn find(&self, query: &str, opts: FindOptions) {
        self.core.borrow_mut().find(query, opts);
    }
    fn close_find(&self) {
        self.core.borrow_mut().close_find(true);
    }

    // Downloads
    fn open_download(&self, id: &str) {
        self.core.borrow_mut().open_download(id);
    }
    fn reveal_download(&self, id: &str) {
        self.core.borrow_mut().reveal_download(id);
    }
    fn cancel_download(&self, id: &str) {
        self.core.borrow_mut().cancel_download(id);
    }
    fn retry_download(&self, id: &str) {
        self.core.borrow_mut().retry_download(id);
    }
    fn clear_downloads(&self) {
        self.core.borrow_mut().clear_downloads();
    }

    // Folders and groups
    fn create_folder(&self) {
        self.core.borrow_mut().create_folder();
    }
    fn rename_folder(&self, folder_id: &str, name: &str) {
        self.core.borrow_mut().rename_folder(folder_id, name);
    }
    fn toggle_folder(&self, folder_id: &str, open: Option<bool>) {
        self.core.borrow_mut().toggle_folder(folder_id, open);
    }
    fn show_folder_menu(&self, folder_id: &str) {
        self.core.borrow_mut().show_folder_menu(folder_id);
    }
    fn rename_group(&self, group_id: &str, name: &str) {
        self.core.borrow_mut().rename_group(group_id, name);
    }
    fn toggle_group(&self, group_id: &str, open: Option<bool>) {
        self.core.borrow_mut().toggle_group(group_id, open);
    }
    fn show_group_menu(&self, group_id: &str) {
        self.core.borrow_mut().show_group_menu(group_id);
    }

    // Bookmarks
    fn toggle_bookmark(&self, tab_id: Option<&str>) {
        self.core.borrow_mut().toggle_bookmark(tab_id);
    }
    fn add_bookmark(&self, tab_id: &str, target: BookmarkTarget) {
        self.core.borrow_mut().add_bookmark(tab_id, &target);
    }
    fn move_bookmark(&self, id: &str, target: BookmarkTarget) {
        self.core.borrow_mut().move_bookmark(id, &target);
    }
    fn open_bookmark(&self, id: &str, tab_id: Option<&str>, new_tab: bool) {
        self.core.borrow_mut().open_bookmark(id, tab_id, new_tab);
    }
    fn rename_bookmark(&self, id: &str, name: &str) {
        self.core.borrow_mut().rename_bookmark(id, name);
    }
    fn show_bookmark_menu(&self, id: &str, tab_id: Option<&str>) {
        self.core.borrow_mut().show_bookmark_menu(id, tab_id);
    }
    fn show_bookmarks_bar_menu(&self) {
        self.core.borrow_mut().show_bookmarks_bar_menu();
    }

    // Boosts
    fn save_boost(&self, site: &str, css: Option<&str>, enabled: Option<bool>) {
        self.core.borrow_mut().save_boost(site, css, enabled);
    }
    fn start_zap(&self, site: &str) {
        self.core.borrow_mut().start_zap(site);
    }
    fn stop_zap(&self) {
        self.core.borrow_mut().stop_zap();
    }
    fn unzap(&self, site: &str, selector: &str) {
        self.core.borrow_mut().unzap(site, selector);
    }
    fn delete_boost(&self, site: &str) {
        self.core.borrow_mut().delete_boost(site);
    }

    // Settings
    fn open_settings(&self, section: Option<&str>) {
        self.core.borrow_mut().open_settings_window(section);
    }
    fn report_ui_error(&self, route: &str, message: &str, stack: &str) {
        self.core.borrow_mut().report_ui_error(route, message, stack);
    }
    fn get_settings_info(&self) -> SettingsInfo {
        self.core.borrow_mut().settings_info()
    }
    fn update_settings(&self, patch: SettingsPatch) {
        self.core.borrow_mut().update_settings(patch);
    }
    fn open_welcome(&self) {
        self.core.borrow_mut().open_welcome_window();
    }
    fn check_for_updates(&self) {
        self.core.borrow_mut().check_for_updates();
    }
    fn welcome_finish(&self) {
        self.core.borrow_mut().welcome_finish();
    }
    fn get_backup_info(&self) -> BackupInfo {
        self.core.borrow_mut().get_backup_info()
    }
    fn export_backup(&self, sections: &[BackupSection], password: Option<&str>) -> Option<String> {
        self.core.borrow_mut().export_backup(sections, password)
    }
    fn open_backup(&self) -> BackupOpen {
        self.core.borrow_mut().open_backup()
    }
    fn open_safety_backup(&self, name: &str) -> Result<BackupSummary, String> {
        self.core.borrow_mut().open_safety_backup(name)
    }
    fn apply_backup(&self, token: &str, sections: &[BackupSection], mode: BackupMode, password: Option<&str>) -> Option<String> {
        self.core.borrow_mut().apply_backup(token, sections, mode, password)
    }
    fn reveal_safety_backups(&self) {
        self.core.borrow_mut().reveal_safety_backups();
    }
    fn set_shortcut_recording(&self, on: bool) {
        self.core.borrow_mut().set_shortcut_recording(on);
    }
    fn set_default_browser(&self) {
        self.core.borrow_mut().set_default_browser();
    }
    fn choose_download_dir(&self) {
        self.core.borrow_mut().choose_download_dir();
    }
    fn relaunch_app(&self) {
        self.core.borrow_mut().relaunch_app();
    }
    fn open_proxy_settings(&self) {
        self.core.borrow_mut().open_proxy_settings();
    }
    fn run_import(&self, source_id: &str, kinds: &[ImportKind]) {
        self.core.borrow_mut().run_import(source_id, kinds);
    }
    fn open_boost(&self, site: &str) {
        self.core.borrow_mut().open_boost(site);
    }
    fn clear_browsing_data(&self, kinds: &[BrowsingDataKind]) {
        let mut c = self.core.borrow_mut();
        c.clear_browsing_data(kinds);
        c.clear_browsing_data_engine(kinds.contains(&BrowsingDataKind::Cache), kinds.contains(&BrowsingDataKind::Cookies));
    }
    fn list_site_permissions(&self) -> Vec<SitePermissionEntry> {
        self.core.borrow_mut().list_site_permissions()
    }
    fn reset_site_permissions(&self, origin: Option<&str>, permission: Option<&str>) {
        self.core.borrow_mut().reset_site_permissions(origin, permission);
    }
    fn open_extension_store(&self) {
        self.core.borrow_mut().open_extension_store();
    }
    fn remove_extension(&self, id: &str) {
        self.core.borrow_mut().remove_extension(id);
    }
    fn load_unpacked_extension(&self) {
        self.core.borrow_mut().load_unpacked_extension();
    }
    fn reload_unpacked_extension(&self, path: &str) {
        self.core.borrow_mut().reload_unpacked_extension(path);
    }
    fn remove_unpacked_extension(&self, path: &str) {
        self.core.borrow_mut().remove_unpacked_extension(path);
    }
    fn show_extension_menu(&self, id: &str) {
        self.core.borrow_mut().show_extension_menu(id);
    }
    fn show_app_menu(&self) {
        // Linux and Windows: the hidden menu bar under the pointer. PHASE3 (GPUI popup menu window).
        self.core.borrow().log("PHASE3: show_app_menu");
    }
    fn show_extensions_menu(&self) -> Option<String> {
        self.core.borrow_mut().show_extensions_menu()
    }
    fn get_extension_action_counts(&self) -> ExtensionActionCounts {
        self.core.borrow_mut().get_extension_action_counts()
    }
    fn get_extension_action_states(&self) -> ExtensionActionStates {
        self.core.borrow_mut().get_extension_action_states()
    }
    fn ack_extension_action(&self, token: &str) -> bool {
        self.core.borrow_mut().ack_extension_action(token)
    }

    // Passwords
    fn list_passwords(&self) -> PasswordsInfo {
        self.core.borrow_mut().list_passwords()
    }
    fn reveal_password(&self, id: &str) -> Option<String> {
        self.core.borrow_mut().reveal_password(id)
    }
    fn copy_password(&self, id: &str) -> bool {
        self.core.borrow_mut().copy_password(id)
    }
    fn save_password(&self, id: Option<&str>, origin: &str, username: &str, password: Option<&str>) -> Option<String> {
        self.core.borrow_mut().save_password(id, origin, username, password)
    }
    fn delete_password(&self, id: &str) {
        self.core.borrow_mut().delete_password(id);
    }
    fn import_passwords(&self) -> Option<PasswordImportResult> {
        self.core.borrow_mut().import_passwords()
    }
    fn export_passwords(&self) -> bool {
        self.core.borrow_mut().export_passwords()
    }
    fn install_password_extension(&self, manager: PasswordManager) {
        self.core.borrow_mut().install_password_extension(manager);
    }
    fn answer_password_prompt(&self, token: &str, answer: PasswordAnswer) {
        self.core.borrow_mut().answer_password_prompt(token, answer);
    }
    fn open_password_key(&self, tab_id: &str, anchor: Rect) {
        self.core.borrow_mut().open_password_key(tab_id, anchor);
    }

    // Autofill
    fn list_autofill(&self) -> AutofillInfo {
        self.core.borrow_mut().list_autofill()
    }
    fn save_address(&self, id: Option<&str>, address: AddressInput) -> Option<String> {
        self.core.borrow_mut().save_address(id, address)
    }
    fn delete_address(&self, id: &str) {
        self.core.borrow_mut().delete_address(id);
    }
    fn save_card(&self, id: Option<&str>, card: CardInput) -> Option<String> {
        self.core.borrow_mut().save_card(id, card)
    }
    fn delete_card(&self, id: &str) {
        self.core.borrow_mut().delete_card(id);
    }
    fn reveal_card(&self, id: &str) -> Option<String> {
        self.core.borrow_mut().reveal_card(id)
    }

    // Compact mode
    fn toggle_compact(&self) {
        self.core.borrow_mut().toggle_compact();
    }
    fn set_toolbar_hover(&self, hover: bool) {
        self.core.borrow_mut().set_toolbar_hover(hover);
    }

    // Media
    fn media_toggle(&self, tab_id: &str) {
        self.core.borrow_mut().media_toggle(tab_id);
    }
    fn media_skip(&self, tab_id: &str, direction: SkipDirection) {
        self.core.borrow_mut().media_skip(tab_id, direction);
    }

    // Advanced split view
    fn begin_tab_drag(&self, tab_id: &str) {
        self.core.borrow_mut().begin_drag(tab_id);
    }
    fn end_tab_drag(&self) {
        self.core.borrow_mut().end_drag();
    }
    fn drop_on_content(&self, tab_id: &str, zone: DropZone) {
        self.core.borrow_mut().drop_on_content(tab_id, zone);
    }
    fn set_split_layout(&self, split_id: &str, layout: SplitLayout) {
        self.core.borrow_mut().set_split_layout(split_id, layout);
    }
    fn set_split_sizes(&self, split_id: &str, sizes: &[f64], live: bool) {
        self.core.borrow_mut().set_split_sizes(split_id, sizes, live);
    }
    fn set_dev_tools_dock(&self, tab_id: &str, dock: DevToolsDock) {
        self.core.borrow_mut().set_dev_tools_dock(tab_id, dock);
    }
    fn close_dev_tools(&self, tab_id: &str) {
        self.core.borrow_mut().close_dev_tools(tab_id);
    }
    fn set_dev_tools_size(&self, tab_id: &str, size: f64, done: bool) {
        self.core.borrow_mut().set_dev_tools_size(tab_id, size, done);
    }
    fn show_dev_tools_menu(&self, tab_id: &str) {
        self.core.borrow_mut().show_dev_tools_menu(tab_id);
    }
    fn show_develop_menu(&self, tab_id: &str) {
        self.core.borrow_mut().show_develop_menu(tab_id);
    }
    fn set_responsive(&self, tab_id: &str, responsive: Option<DevResponsive>) {
        self.core.borrow_mut().set_responsive(tab_id, responsive);
    }
    fn set_responsive_dragging(&self, tab_id: &str, dragging: bool) {
        self.core.borrow_mut().set_responsive_dragging(tab_id, dragging);
    }
    fn show_console(&self, tab_id: &str) {
        self.core.borrow_mut().show_console(tab_id);
    }
    fn dev_tools_window_ready(&self) -> Option<DevToolsWindowState> {
        self.core.borrow_mut().dev_tools_window_ready()
    }

    // Advanced spaces
    fn set_space_profile(&self, space_id: &str, profile: SpaceProfile) {
        self.core.borrow_mut().set_space_profile(space_id, profile);
    }
    fn add_route(&self, pattern: &str, space_id: &str) {
        self.core.borrow_mut().add_route(pattern, space_id);
    }
    fn remove_route(&self, route_id: &str) {
        self.core.borrow_mut().remove_route(route_id);
    }

    // Library
    fn open_library(&self, section: Option<LibrarySection>) {
        let name = section.and_then(|s| serde_json::to_value(s).ok()).and_then(|v| v.as_str().map(str::to_string));
        self.core.borrow_mut().open_library_window(name.as_deref());
    }
    fn open_library_url(&self, url: &str) {
        self.core.borrow_mut().open_tab(url, signet_core::model::OpenOptions { activate: true, ..Default::default() });
    }
    fn query_history(&self, query: &str, offset: usize, limit: usize) -> Vec<HistoryEntry> {
        self.core.borrow_mut().history.list(query, offset, limit.min(HISTORY_QUERY_MAX))
    }
    fn delete_history(&self, url: &str) {
        self.core.borrow_mut().history.remove(url);
    }
    fn list_archive(&self, query: &str) -> Vec<ArchivedEntry> {
        let c = self.core.borrow();
        let q = signet_core::urls::fold(query.trim());
        c.state()
            .archive
            .iter()
            .filter(|a| q.is_empty() || signet_core::urls::fold(&a.title).contains(&q) || signet_core::urls::fold(&a.url).contains(&q))
            .map(|a| ArchivedEntry { id: a.id.clone().unwrap_or_else(|| format!("{}:{}", a.archived_at, a.url)), url: a.url.clone(), title: a.title.clone(), favicon: a.favicon.clone(), archived_at: a.archived_at })
            .collect()
    }
    fn restore_archived(&self, id: &str) {
        let mut c = self.core.borrow_mut();
        let pos = c.state().archive.iter().position(|a| a.id.as_deref() == Some(id) || format!("{}:{}", a.archived_at, a.url) == id);
        if let Some(i) = pos {
            let a = c.store.data.state.archive.remove(i);
            c.open_tab(&a.url, signet_core::model::OpenOptions { activate: true, ..Default::default() });
        }
    }
    fn delete_archived(&self, id: &str) {
        let mut c = self.core.borrow_mut();
        c.store.data.state.archive.retain(|a| a.id.as_deref() != Some(id) && format!("{}:{}", a.archived_at, a.url) != id);
        c.changed();
    }
    fn rename_tab(&self, tab_id: &str, title: Option<&str>) {
        self.core.borrow_mut().rename_tab(tab_id, title);
    }
    fn set_icon(&self, target: IconTarget, emoji: Option<&str>) {
        self.core.borrow_mut().set_icon(&target, emoji);
    }
    fn open_site_panel(&self, anchor: Rect) {
        self.core.borrow_mut().open_site_panel(anchor);
    }
    fn set_site_permission(&self, origin: &str, permission: &str, state: PermissionState) {
        self.core.borrow_mut().set_site_permission(origin, permission, state);
    }
    fn clear_site_data(&self, origin: &str) {
        self.core.borrow_mut().clear_site_data(origin);
    }

    // Devices
    fn get_link_state(&self) -> LinkState {
        self.core.borrow_mut().get_link_state()
    }
    fn set_link_general(&self, on: bool) {
        self.core.borrow_mut().set_link_general(on);
    }
    fn set_link_device(&self, link_id: &str, on: bool) {
        self.core.borrow_mut().set_link_device(link_id, on);
    }
    fn forget_link(&self, link_id: &str) {
        self.core.borrow_mut().forget_link(link_id);
    }
    fn start_link_pairing(&self) -> bool {
        self.core.borrow_mut().start_link_pairing()
    }
    fn cancel_link_pairing(&self) {
        self.core.borrow_mut().cancel_link_pairing();
    }

    // Language and appearance
    fn get_locale(&self) -> Locale {
        self.core.borrow().locale()
    }
    fn get_appearance(&self) -> Appearance {
        self.core.borrow().appearance()
    }
    fn fallback_favicon(&self, url: &str) -> Option<String> {
        self.core.borrow_mut().fallback_favicon(url)
    }

    // Mini player
    fn pip_command(&self, command: PipCommand, value: Option<f64>) {
        self.core.borrow_mut().pip_command(command, value);
    }
    fn open_pip(&self, tab_id: Option<&str>) {
        self.core.borrow_mut().open_pip(tab_id);
    }

    // Little Signet
    fn little_ready(&self) {
        self.core.borrow_mut().little_ready();
    }
    fn little_open_in_main(&self) {
        self.core.borrow_mut().little_open_in_main();
    }
    fn little_copy_url(&self) {
        self.core.borrow_mut().little_copy_url();
    }

    // Private windows
    fn get_private_state(&self) -> Option<PrivateState> {
        self.core.borrow_mut().get_private_state()
    }
    fn private_new_tab(&self) {
        self.core.borrow_mut().private_new_tab();
    }
    fn private_activate(&self, tab_id: &str) {
        self.core.borrow_mut().private_activate(tab_id);
    }
    fn private_close_tab(&self, tab_id: &str) {
        self.core.borrow_mut().private_close_tab(tab_id);
    }
    fn private_navigate(&self, input: &str) {
        self.core.borrow_mut().private_navigate(input);
    }
    fn open_private_window(&self) {
        self.core.borrow_mut().open_private_window();
    }

    // Passkey
    fn get_passkey_prompt(&self) -> Option<PasskeyPromptState> {
        self.core.borrow_mut().get_passkey_prompt()
    }
    fn cancel_passkey_prompt(&self) {
        self.core.borrow_mut().cancel_passkey_prompt();
    }

    // Apps
    fn install_web_app(&self, token: &str, name: &str, icon: &str) {
        self.core.borrow_mut().install_web_app(token, name, icon);
    }
    fn list_web_apps(&self) -> WebAppsInfo {
        self.core.borrow_mut().list_web_apps()
    }
    fn open_web_app(&self, id: &str) {
        self.core.borrow_mut().open_web_app(id);
    }
    fn rename_web_app(&self, id: &str, name: &str) {
        self.core.borrow_mut().rename_web_app(id, name);
    }
    fn reveal_web_app(&self, id: &str) {
        self.core.borrow_mut().reveal_web_app(id);
    }
    fn remove_web_app(&self, id: &str) {
        self.core.borrow_mut().remove_web_app(id);
    }
    fn web_app_ready(&self) {
        self.core.borrow_mut().web_app_ready();
    }
    fn web_app_home(&self) {
        self.core.borrow_mut().web_app_home();
    }
    fn web_app_open_in_main(&self) {
        self.core.borrow_mut().web_app_open_in_main();
    }
    fn web_app_copy_url(&self) {
        self.core.borrow_mut().web_app_copy_url();
    }
}
