//! Ports `app/desktop/src/shared/types/api.ts`: the contract between the interface and the app.
//!
//! In Electron the UI called `window.signet.*` over IPC. Here the UI (`signet-ui`) holds an `Rc<dyn SignetApi>` or
//! `Arc<dyn SignetApi>` and calls it directly; `signet-app` implements it. Every method is synchronous and returns the
//! value directly (methods that returned a `Promise<void>` return `()`). Everything that was pushed by the main process
//! (`onX(cb)` subscriptions) is a `UiEvent` delivered to the listener registered with `subscribe`.
//!
//! Not ported: the IPC validation layer (`ipc/validate.ts`, `contract.ts`); Rust types replace it.

use crate::core::store::SettingsPatch;
use crate::i18n::Locale;
use crate::titlebar::TitlebarAction;
use crate::types::*;
use serde::{Deserialize, Serialize};

// --------------------------------------------------------------------------- argument types

/// What the capture tool does with the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureAction {
    Copy,
    Save,
    Easel,
}

/// Result of `take_capture`: `ok` false if nothing was taken or saved; `png` (data URL) for `Copy`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CaptureResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkipDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordAnswer {
    Save,
    Never,
    Dismiss,
}

/// Options of `find`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindOptions {
    pub forward: bool,
    /// Go to the next occurrence of the same query.
    pub find_next: bool,
}

/// Result of `open_backup`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BackupOpen {
    Summary { summary: BackupSummary },
    Error { message: String },
    Cancelled,
}

/// Kind of grouped archive announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveKind {
    Archived,
    TabsClosed,
    SpaceDeleted,
}

/// Announcement of a grouped archive of today's tabs (undoable with `undo_archive`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchivedNotice {
    pub id: u64,
    pub kind: ArchiveKind,
    pub count: usize,
    pub name: String,
}

// --------------------------------------------------------------------------- pushed events

/// Everything the main process pushes to the interface (the `onX` subscriptions of `SignetApi`). The first words of
/// each variant doc give the TS subscription it replaces.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// `onState`: the full state.
    State(Box<Snapshot>),
    /// `onStatePatch`.
    StatePatch(StatePatch),
    /// `onSidebarSlide`: one step of the sidebar slide, 1 shown, 0 hidden.
    SidebarSlide(f64),
    /// `onPendingSpace`: a space is being chosen elsewhere (overlay popover).
    PendingSpace(String),
    /// `onEditAddressRequest`: Cmd+L, main asks for the address field's rectangle; answer with `open_palette(EditUrl, rect)`.
    EditAddressRequest,
    /// `onTabPreview`: a tab's hover preview (`#preview` view).
    TabPreview(TabPreviewPayload),
    /// `onTabPreviewHide`.
    TabPreviewHide,
    /// `onModsChanged`: mods changed elsewhere, the section re-reads.
    ModsChanged,
    /// `onEditSpace`: open the space form (`None`: new space).
    EditSpace { space_id: Option<String> },
    /// `onArchived`.
    Archived(ArchivedNotice),
    /// `onUndoExpired`: the undo window of announcement `id` has passed.
    UndoExpired(u64),
    /// `onOverlayShow`.
    OverlayShow(Box<OverlayPayload>),
    /// Engine suggestions for `query` arrived from the network (the command bar re-reads them).
    PaletteSuggestions { query: String, results: Vec<PaletteResult> },
    /// `onOverlayHide`: `instant` = immediate unmount (no exit animation).
    OverlayHide { instant: bool },
    /// `onFindState`.
    FindState(FindState),
    /// `onRenameFolder`: put the folder's name (or tab group's: `folder_id` = group id) into edit mode.
    RenameFolder { folder_id: String },
    /// `onRenameBookmark`.
    RenameBookmark { id: String },
    /// `onBoostState`.
    BoostState(Box<BoostEditorState>),
    /// `onSettingsInfo`.
    SettingsInfo(Box<SettingsInfo>),
    /// `onLibrarySection`.
    LibrarySection(LibrarySection),
    /// `onRenameTab`: put a tab's title into edit mode.
    RenameTab { tab_id: String },
    /// `onPickIcon`: open the emoji picker of a tab, folder or group.
    PickIcon(IconTarget),
    /// `onExtensionActionCounts`.
    ExtensionActionCounts(ExtensionActionCounts),
    /// `onExtensionActionStates`.
    ExtensionActionStates(ExtensionActionStates),
    /// `onRunExtensionAction`: an extension shortcut asks the toolbar to open an extension's button.
    RunExtensionAction { token: String, extension_id: String },
    /// `onLinkState`.
    LinkState(Box<LinkState>),
    /// `onLocale`.
    Locale(Locale),
    /// `onAppearance`.
    Appearance(Appearance),
    /// `onQuitState`.
    QuitState(QuitHudState),
    /// `onPipState`.
    PipState(PipState),
    /// `onLittleState`.
    LittleState(LittleState),
    /// `onPrivateState`.
    PrivateState(PrivateState),
    /// `onPrivateFocusAddress`.
    PrivateFocusAddress,
    /// `onWebAppsChanged`.
    WebAppsChanged,
    /// `onWebAppState`.
    WebAppState(WebAppState),
    /// `onPasskeyStatus`.
    PasskeyStatus(PasskeyStatus),
    /// `onDevToolsWindowState`.
    DevToolsWindowState(DevToolsWindowState),
}

/// Handle of a registered listener: dropping it unsubscribes.
pub struct Subscription(Option<Box<dyn FnOnce() + Send>>);

impl Subscription {
    pub fn new(cancel: impl FnOnce() + Send + 'static) -> Self {
        Subscription(Some(Box::new(cancel)))
    }
    /// A subscription that does nothing when dropped.
    pub fn none() -> Self {
        Subscription(None)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            c();
        }
    }
}

// --------------------------------------------------------------------------- the API

/// API the interface calls. Implemented by `signet-app`.
#[allow(clippy::too_many_arguments)]
pub trait SignetApi {
    // Window

    /// What a double-click on a window's title area does (system setting). Zoom when the platform has none.
    fn titlebar_double_click_action(&self) -> TitlebarAction {
        TitlebarAction::Zoom
    }

    // Events

    /// Registers a listener for pushed events. Events are delivered on the thread that produced them: listeners must
    /// hand them over to the UI thread themselves.
    fn subscribe(&self, listener: Box<dyn Fn(UiEvent) + Send + Sync>) -> Subscription;

    // State
    fn get_state(&self) -> Snapshot;

    // Tabs
    fn activate_tab(&self, tab_id: &str);
    fn close_tab(&self, tab_id: &str);
    fn navigate(&self, tab_id: &str, input: &str);
    fn go_back(&self, tab_id: Option<&str>);
    fn go_forward(&self, tab_id: Option<&str>);
    fn reload(&self, tab_id: Option<&str>);
    fn toggle_mute(&self, tab_id: &str);
    /// Puts a pinned tab back on its "home" URL.
    fn reset_pinned(&self, tab_id: &str);
    /// Enters or leaves the tab's reader mode.
    fn toggle_reader(&self, tab_id: &str);
    /// Zooms the tab's page by `delta` levels (shortcut code path); 0 goes back to the default. Shows the zoom bubble.
    fn zoom_page(&self, tab_id: &str, delta: i32);
    /// Opens the zoom bubble (click on the toolbar's zoom icon).
    fn open_zoom(&self, tab_id: &str);

    // Live folders
    /// Creates a live folder (`folder_id` None) or changes its source; returns a translated error message, or None.
    fn save_live_folder(&self, folder_id: Option<&str>, source: LiveSource) -> Option<String>;
    fn refresh_live_folder(&self, folder_id: &str);
    /// True if a GitHub token is stored (the token itself never leaves main).
    fn has_github_token(&self) -> bool;
    /// Stores (encrypted) or removes (`None`) the GitHub token; returns a translated error message, or None.
    fn set_github_token(&self, token: Option<&str>) -> Option<String>;
    /// "New folder" button menu: plain folder or live folder.
    fn show_add_folder_menu(&self);
    /// Translates the tab's page into the preferred language, or goes back to the original.
    fn toggle_translation(&self, tab_id: &str);
    /// Takes a capture of the tab (`capture` overlay). `Copy`: returns the image (PNG as a data URL) that the UI puts on
    /// the clipboard; `Save`: closes the tool and asks where to save it. `ok` false if nothing was taken or saved.
    fn take_capture(&self, tab_id: &str, target: CaptureTarget, action: CaptureAction) -> CaptureResult;
    /// Hover over a tab: preview next to `rect` (coordinates of the calling view); `below` = under the element.
    fn show_tab_preview(&self, tab_id: &str, rect: Rect, below: bool);
    fn hide_tab_preview(&self);

    // UI mods (Settings > Mods)
    fn list_mods(&self) -> ModsInfo;
    /// Adds (None id) or edits a mod; returns its id.
    fn save_mod(&self, id: Option<&str>, name: &str, css: &str) -> String;
    fn delete_mod(&self, id: &str);
    fn set_mod_enabled(&self, id: &str, enabled: bool);
    /// Moves a mod one step up (-1) or down (1): the order is the order of application.
    fn move_mod(&self, id: &str, delta: i32);
    fn set_mods_disabled(&self, disabled: bool);
    /// Imports a .css file (dialog); returns the id of the created mod, or None if cancelled.
    fn import_mod(&self) -> Option<String>;
    fn export_mod(&self, id: &str);
    fn show_tab_menu(&self, tab_id: &str);
    /// Moves a tab up (-1) or down (1) one rank in its list.
    fn move_tab_step(&self, tab_id: &str, dir: i32);
    /// "New" menu (tab, note, Easel), on right click on "New tab".
    fn show_new_menu(&self);
    fn move_item(&self, source: DragSource, target: DropTarget);

    // Spaces
    fn switch_space(&self, space_id: &str);
    fn create_space(&self, draft: SpaceDraft);
    fn update_space(&self, space_id: &str, draft: SpaceDraft);
    fn show_space_menu(&self, space_id: &str);

    // Sidebar
    fn toggle_sidebar(&self);
    /// `live`: gesture in progress, main tolerates a slight overshoot (rubber-band) without persisting it.
    fn set_sidebar_width(&self, width: f64, live: bool);
    fn clear_today(&self);
    fn undo_archive(&self, id: u64);

    // Split view
    fn focus_pane(&self, tab_id: &str);
    fn unsplit(&self, split_id: &str);

    // Command bar
    /// `anchor`: address field from which the command bar grows (`EditUrl` mode).
    fn open_palette(&self, mode: PaletteMode, anchor: Option<Rect>);
    /// The overlay is subscribed to `OverlayShow`: main can send it the pending payload.
    fn overlay_ready(&self);
    /// The find bar view is subscribed: main sends it the current state (its window opens after the first state).
    fn find_ready(&self) {}
    /// `instant`: click outside the panel, no exit animation (the view must stop swallowing clicks).
    fn close_overlay(&self, instant: bool);
    /// The archive announcement view finished its exit: main removes it.
    fn toast_done(&self);
    fn palette_query(&self, query: &str, mode: PaletteMode) -> Vec<PaletteResult>;
    fn palette_suggest(&self, query: &str) -> Vec<PaletteResult>;
    fn palette_execute(&self, result: PaletteResult, mode: PaletteMode);

    // Peek
    fn expand_peek(&self);

    // Tab row at the top (`horizontal` layout): popover above the pages, anchored under `anchor` (window coordinates)
    fn open_popover(&self, popover: PopoverRequest, anchor: Rect);

    // Floating sidebar (when hidden)
    /// The mouse touches the window's left edge (true) or has left the floating sidebar (false).
    fn set_sidebar_hover(&self, hover: bool);

    // Find in page
    /// Searches `query` in the active tab.
    fn find(&self, query: &str, opts: FindOptions);
    fn close_find(&self);

    // Downloads
    fn open_download(&self, id: &str);
    fn reveal_download(&self, id: &str);
    fn cancel_download(&self, id: &str);
    /// Resumes an interrupted download, or restarts its address if it cannot resume.
    fn retry_download(&self, id: &str);
    fn clear_downloads(&self);

    // Pinned folders
    /// Creates "New folder" at the head of the active space's pinned items and asks for its rename (`RenameFolder`).
    fn create_folder(&self);
    fn rename_folder(&self, folder_id: &str, name: &str);
    /// Toggles the folder, or sets it to the `open` state if given.
    fn toggle_folder(&self, folder_id: &str, open: Option<bool>);
    fn show_folder_menu(&self, folder_id: &str);

    // Today tab groups
    fn rename_group(&self, group_id: &str, name: &str);
    /// Collapses or expands the group, or sets it to the `open` state if given.
    fn toggle_group(&self, group_id: &str, open: Option<bool>);
    fn show_group_menu(&self, group_id: &str);

    // Bookmarks (per space)
    /// Adds the tab's page to the space's bookmarks, or removes it if already there (Cmd+D, star).
    fn toggle_bookmark(&self, tab_id: Option<&str>);
    /// Adds the tab to the active space's bookmarks at this spot (tab dropped on the bar).
    fn add_bookmark(&self, tab_id: &str, target: BookmarkTarget);
    fn move_bookmark(&self, id: &str, target: BookmarkTarget);
    /// Opens the bookmark in tab `tab_id` (the pane's), or in a new background tab (`new_tab`).
    fn open_bookmark(&self, id: &str, tab_id: Option<&str>, new_tab: bool);
    fn rename_bookmark(&self, id: &str, name: &str);
    fn show_bookmark_menu(&self, id: &str, tab_id: Option<&str>);
    /// Right click on the bar outside a bookmark: new folder, hide the bar.
    fn show_bookmarks_bar_menu(&self);

    // Boosts (`#boost` window)
    /// Saves and applies right away to the site's open tabs.
    fn save_boost(&self, site: &str, css: Option<&str>, enabled: Option<bool>);
    /// Starts the "Hide" tool in the site's active tab; each click adds a selector, Escape stops.
    fn start_zap(&self, site: &str);
    fn stop_zap(&self);
    fn unzap(&self, site: &str, selector: &str);
    fn delete_boost(&self, site: &str);

    // Settings (`#settings` window)
    /// Opens Settings, on a section if given (`passwords`...).
    fn open_settings(&self, section: Option<&str>);
    /// UI error, noted in the `logs/interface.log` log of the data folder.
    fn report_ui_error(&self, route: &str, message: &str, stack: &str);
    fn get_settings_info(&self) -> SettingsInfo;
    /// Applies a partial settings update (JSON object of the changed fields, camelCase keys, validated by `clean_settings`).
    fn update_settings(&self, patch: SettingsPatch);
    /// First-launch introduction (`#welcome` window): reopens it (Settings > About).
    fn open_welcome(&self);
    /// Help > Check for Updates (also in Settings > About): looks for a new version and shows the outcome.
    fn check_for_updates(&self);
    /// Finishes or skips the introduction: it will not come back on its own, the window closes.
    fn welcome_finish(&self);
    // Backup (Settings > Backup)
    fn get_backup_info(&self) -> BackupInfo;
    /// Exports the checked parts to a chosen file; `password` required with passwords. Error message, or None.
    fn export_backup(&self, sections: &[BackupSection], password: Option<&str>) -> Option<String>;
    /// Chooses a backup file: its summary, an error message, or cancelled.
    fn open_backup(&self) -> BackupOpen;
    /// Opens a safety copy (by its name) like a chosen file.
    fn open_safety_backup(&self, name: &str) -> Result<BackupSummary, String>;
    /// Imports the checked parts then restarts; returns an error message, or None (cancelled).
    fn apply_backup(&self, token: &str, sections: &[BackupSection], mode: BackupMode, password: Option<&str>) -> Option<String>;
    fn reveal_safety_backups(&self);
    /// Shortcut capture in progress (true) or done: menu shortcuts and Cmd+Q are suspended during capture.
    fn set_shortcut_recording(&self, on: bool);
    fn set_default_browser(&self);
    /// Chooses the downloads folder in a system dialog (cancelled = unchanged).
    fn choose_download_dir(&self);
    /// Relaunches the app (graphics acceleration change). Packaged app only.
    fn relaunch_app(&self);
    /// Opens the system's proxy settings.
    fn open_proxy_settings(&self);
    /// Starts the import of a source (`import_sources[].id`), for the checked data; the summary is shown in a dialog.
    fn run_import(&self, source_id: &str, kinds: &[ImportKind]);
    fn open_boost(&self, site: &str);
    fn clear_browsing_data(&self, kinds: &[BrowsingDataKind]);
    /// Permission decisions remembered per site.
    fn list_site_permissions(&self) -> Vec<SitePermissionEntry>;
    /// Forgets one decision, those of a site, or all (no argument).
    fn reset_site_permissions(&self, origin: Option<&str>, permission: Option<&str>);
    /// Opens the extension store in a new tab.
    fn open_extension_store(&self);
    fn remove_extension(&self, id: &str);
    /// Unpacked extensions: choose a folder to load, reload, remove (the folder stays).
    fn load_unpacked_extension(&self);
    fn reload_unpacked_extension(&self, path: &str);
    fn remove_unpacked_extension(&self, path: &str);
    /// Native context menu of an extension button (options, manage, remove...).
    fn show_extension_menu(&self, id: &str);
    /// Linux and Windows: opens the app menu (the hidden menu bar's) under the pointer.
    fn show_app_menu(&self);
    /// Extensions button menu: all extensions with a button, and their pinning. Returns the one to open, or None.
    fn show_extensions_menu(&self) -> Option<String>;
    /// Counts of requests blocked per tab and extension (`declarativeNetRequest` action count badge).
    fn get_extension_action_counts(&self) -> ExtensionActionCounts;
    /// Enabled state and badge text color set by extensions on their buttons.
    fn get_extension_action_states(&self) -> ExtensionActionStates;
    /// The toolbar takes the request; false if it was already taken or main gave up waiting.
    fn ack_extension_action(&self, token: &str) -> bool;

    // Passwords (Settings > Passwords; never a password without `reveal_password`)
    fn list_passwords(&self) -> PasswordsInfo;
    /// Plaintext password, after Touch ID if available; None if refused.
    fn reveal_password(&self, id: &str) -> Option<String>;
    /// Copies the password to the clipboard (Touch ID if available); false if refused.
    fn copy_password(&self, id: &str) -> bool;
    /// Adds (None id) or edits a login; None `password` = unchanged. Returns an error message, or None.
    fn save_password(&self, id: Option<&str>, origin: &str, username: &str, password: Option<&str>) -> Option<String>;
    fn delete_password(&self, id: &str);
    /// Imports a CSV file chosen in a dialog; returns the report, or None if cancelled.
    fn import_passwords(&self) -> Option<PasswordImportResult>;
    /// Exports as CSV (Touch ID then save dialog); false if cancelled.
    fn export_passwords(&self) -> bool;
    /// Installs a password manager's extension from the store.
    fn install_password_extension(&self, manager: PasswordManager);
    /// Answer to the save prompt (overlay, `password` mode).
    fn answer_password_prompt(&self, token: &str, answer: PasswordAnswer);
    /// Key button of the address bar: reopens the pending prompt, otherwise lists the site's accounts, under `anchor`.
    fn open_password_key(&self, tab_id: &str, anchor: Rect);

    // Autofill (Settings > Autofill; a card number only leaves main through `reveal_card`, after authentication)
    fn list_autofill(&self) -> AutofillInfo;
    /// Adds (None id) or edits an address. Returns an error message, or None.
    fn save_address(&self, id: Option<&str>, address: AddressInput) -> Option<String>;
    fn delete_address(&self, id: &str);
    /// Adds (None id) or edits a card; a new number for an existing card asks for authentication. Error message, or None.
    fn save_card(&self, id: Option<&str>, card: CardInput) -> Option<String>;
    fn delete_card(&self, id: &str);
    /// Full card number, after the system's authentication; None if refused.
    fn reveal_card(&self, id: &str) -> Option<String>;

    // Compact mode
    fn toggle_compact(&self);
    /// The mouse touches the web card's top edge (true) or has left the floating toolbar (false).
    fn set_toolbar_hover(&self, hover: bool);

    // Media
    fn media_toggle(&self, tab_id: &str);
    fn media_skip(&self, tab_id: &str, direction: SkipDirection);

    // Advanced split view
    /// The sidebar starts dragging a tab: main shows the drop zones on the page.
    fn begin_tab_drag(&self, tab_id: &str);
    fn end_tab_drag(&self);
    /// Drop on a page zone: `Center` replaces the displayed tab, the edges create or enlarge the split view.
    fn drop_on_content(&self, tab_id: &str, zone: DropZone);
    fn set_split_layout(&self, split_id: &str, layout: SplitLayout);
    fn set_split_sizes(&self, split_id: &str, sizes: &[f64], live: bool);
    /// Developer tools of a tab: placement, close, size (0 = default; `done` at the end of the drag), button menu.
    fn set_dev_tools_dock(&self, tab_id: &str, dock: DevToolsDock);
    fn close_dev_tools(&self, tab_id: &str);
    fn set_dev_tools_size(&self, tab_id: &str, size: f64, done: bool);
    fn show_dev_tools_menu(&self, tab_id: &str);
    /// Menu of a tab's active development settings (address bar button).
    fn show_develop_menu(&self, tab_id: &str);
    /// Responsive mode of a tab: device and size, or None to leave it.
    fn set_responsive(&self, tab_id: &str, responsive: Option<DevResponsive>);
    /// Dragging responsive mode handles: the fit scale is frozen while true, then recomputed.
    fn set_responsive_dragging(&self, tab_id: &str, dragging: bool);
    /// Opens the tab's developer tools on the console.
    fn show_console(&self, tab_id: &str);
    /// Detached tools window: state of its header.
    fn dev_tools_window_ready(&self) -> Option<DevToolsWindowState>;

    // Advanced spaces
    fn set_space_profile(&self, space_id: &str, profile: SpaceProfile);
    fn add_route(&self, pattern: &str, space_id: &str);
    fn remove_route(&self, route_id: &str);

    // Library and tabs
    fn open_library(&self, section: Option<LibrarySection>);
    /// Opens a history entry in a new tab of the main window.
    fn open_library_url(&self, url: &str);
    fn query_history(&self, query: &str, offset: usize, limit: usize) -> Vec<HistoryEntry>;
    fn delete_history(&self, url: &str);
    fn list_archive(&self, query: &str) -> Vec<ArchivedEntry>;
    fn restore_archived(&self, id: &str);
    fn delete_archived(&self, id: &str);
    fn rename_tab(&self, tab_id: &str, title: Option<&str>);
    /// Emoji of a tab, folder or group; None = default icon.
    fn set_icon(&self, target: IconTarget, emoji: Option<&str>);
    /// Opens the active tab's site panel, anchored under `anchor` (window coordinates).
    fn open_site_panel(&self, anchor: Rect);
    fn set_site_permission(&self, origin: &str, permission: &str, state: PermissionState);
    fn clear_site_data(&self, origin: &str);

    // Devices (link with the phone: Settings > Devices, toolbar indicator)
    fn get_link_state(&self) -> LinkState;
    /// Master switch ("Sync").
    fn set_link_general(&self, on: bool);
    /// Switch for one phone, on this computer.
    fn set_link_device(&self, link_id: &str, on: bool);
    /// Removes the link (keys included) and notifies the phone if connected.
    fn forget_link(&self, link_id: &str);
    /// Creates a pairing QR code (replaces the previous one); the pushed state contains it. False if the computer cannot listen.
    fn start_link_pairing(&self) -> bool;
    fn cancel_link_pairing(&self);

    // UI language and appearance (all views)
    fn get_locale(&self) -> Locale;
    fn get_appearance(&self) -> Appearance;
    /// Favicon the UI could not load, fetched by main (data URL), or None.
    fn fallback_favicon(&self, url: &str) -> Option<String>;

    // Floating mini player
    /// `Seek`: `value` = position in seconds; `Back` / `Forward`: 15 s.
    fn pip_command(&self, command: PipCommand, value: Option<f64>);
    /// Opens the mini-player for the active tab's video (context menu, command bar).
    fn open_pip(&self, tab_id: Option<&str>);

    // Little Signet (called from a Little Signet window's bar)
    /// The bar is subscribed to `LittleState`: main sends the state back.
    fn little_ready(&self);
    fn little_open_in_main(&self);
    fn little_copy_url(&self);

    // Private windows (called from a private window's bar, which targets its own window)
    fn get_private_state(&self) -> Option<PrivateState>;
    fn private_new_tab(&self);
    fn private_activate(&self, tab_id: &str);
    fn private_close_tab(&self, tab_id: &str);
    fn private_navigate(&self, input: &str);
    fn open_private_window(&self);

    // Passkey from a phone (`#passkey` window)
    /// The request waiting for a phone (QR code to show), or None if there is none anymore.
    fn get_passkey_prompt(&self) -> Option<PasskeyPromptState>;
    /// Cancels the request (button, Escape): the site gets a refusal and offers its other methods.
    fn cancel_passkey_prompt(&self);

    // Apps (installed sites: `webapp` overlay, Settings > Apps)
    /// Installs the site offered by the overlay (`token`) under this name, with this icon (1024 px PNG as a data URL).
    fn install_web_app(&self, token: &str, name: &str, icon: &str);
    fn list_web_apps(&self) -> WebAppsInfo;
    fn open_web_app(&self, id: &str);
    fn rename_web_app(&self, id: &str, name: &str);
    /// Shows the launcher in the file manager (recreated if it disappeared).
    fn reveal_web_app(&self, id: &str);
    fn remove_web_app(&self, id: &str);

    // Bar of an app window (`#webapp` route, which targets its own window)
    /// The bar is subscribed to `WebAppState`: main sends the state back.
    fn web_app_ready(&self);
    /// Goes back to the app's start address.
    fn web_app_home(&self);
    /// Opens the page in a Signet tab and closes the app window.
    fn web_app_open_in_main(&self);
    fn web_app_copy_url(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait must stay object safe: the UI holds a `dyn SignetApi`.
    #[test]
    fn api_is_object_safe() {
        fn _takes(_: &dyn SignetApi) {}
    }

    #[test]
    fn subscription_cancels_on_drop() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        drop(Subscription::new(move || f.store(true, Ordering::SeqCst)));
        assert!(flag.load(Ordering::SeqCst));
        drop(Subscription::none());
    }
}
