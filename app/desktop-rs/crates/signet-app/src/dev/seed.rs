//! Port of `dev/seed.ts`: forced theme and sample state (`SIGNET_THEME`, `SIGNET_SEED=1` and the options that shape it).

use signet_core::core::history::{History, ImportedEntry};
use signet_core::core::store::{ArchivedTab, Store};
use signet_core::factory::{new_space, new_tab, CleanDraft};
use signet_core::ids::now_ms;
use signet_core::types::{DownloadInfo, DownloadStatus, FolderItem, Item, SpaceProfile, SpaceRoute, SplitItem, SplitLayout, TabLayout, Theme};

use super::{enabled, flag, var};

/// `encodeURIComponent`.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Sample favicon: colored square with a letter (no network).
fn letter_icon(letter: &str, hue: u32) -> String {
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"8\" fill=\"hsl({hue} 60% 50%)\"/><text x=\"16\" y=\"22\" font-size=\"18\" font-family=\"sans-serif\" font-weight=\"700\" text-anchor=\"middle\" fill=\"white\">{letter}</text></svg>"
    );
    format!("data:image/svg+xml;utf8,{}", encode(&svg))
}

fn first_upper(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

/// Before the window opens: forced theme (`SIGNET_THEME`), onboarding off, then the sample state (`SIGNET_SEED=1`).
pub fn setup(store: &mut Store) {
    if !enabled() {
        return;
    }
    let state = &mut store.data.state;
    match var("SIGNET_THEME").as_deref() {
        Some("light") => state.settings.theme = Some(Theme::Light),
        Some("dark") => state.settings.theme = Some(Theme::Dark),
        _ => {}
    }
    // The welcome window only opens when it is the target (it would sit over every other shot).
    let welcome = var("SIGNET_CAPTURE").is_some_and(|c| c.starts_with("welcome:"));
    if var("SIGNET_CAPTURE").is_some() {
        state.settings.onboarded = Some(!welcome);
    }
    if !flag("SIGNET_SEED") {
        return;
    }

    let make = |state: &mut signet_core::core::store::State, title: &str, url: &str, hue: u32| -> String {
        let mut t = new_tab(url, title);
        t.favicon = Some(letter_icon(&first_upper(title), hue));
        let id = t.id.clone();
        state.tabs.insert(id.clone(), t);
        id
    };
    state.tabs.clear();
    let favorites = vec![
        make(state, "Wikipédia", "https://www.wikipedia.org/", 210),
        make(state, "GitHub", "https://github.com/", 260),
        make(state, "Hacker News", "https://news.ycombinator.com/", 25),
        make(state, "Le Monde", "https://www.lemonde.fr/", 0),
    ];
    let mut perso = new_space(CleanDraft { name: "Personnel".into(), icon: "🏠".into(), hue: 250.0, theme: None });
    let folder = |id: &str, name: &str, hue: f64, items: Vec<Item>| {
        Item::Folder(FolderItem { id: id.into(), name: name.into(), icon: None, open: true, items, hue: Some(hue), live: None })
    };
    let lecture = vec![
        Item::tab(make(state, "Wikipédia · Le libre", "https://fr.wikipedia.org/", 210)),
        Item::tab(make(state, "W3C", "https://www.w3.org/", 200)),
        Item::tab(make(state, "Example Domain", "https://example.com/", 150)),
    ];
    let actu = vec![Item::tab(make(state, "Le Monde · À la une", "https://www.lemonde.fr/", 0))];
    let notes = Item::tab(make(state, "Notes de version", "https://github.com/electron/electron/releases", 270));
    perso.pinned = vec![folder("dossier-bleu", "Lecture", 215.0, lecture), folder("dossier-rouge", "Actualités", 0.0, actu), notes];
    let wiki = make(state, "Wikipédia, l’encyclopédie libre", "https://www.wikipedia.org/", 210);
    perso.today = vec![
        Item::tab(wiki.clone()),
        Item::tab(make(state, "Hacker News", "https://news.ycombinator.com/", 25)),
        Item::tab(make(state, "Example Domain", "https://example.com/", 150)),
        Item::tab(make(state, "W3C - Standards du Web", "https://www.w3.org/", 200)),
        Item::tab(make(state, "GitHub - Where software is built", "https://github.com/", 260)),
    ];
    perso.active_tab_id = Some(wiki);
    // SIGNET_ACTIVE=<URL fragment>: active tab at startup.
    if let Some(wanted) = var("SIGNET_ACTIVE")
        && let Some(found) = state.tabs.values().find(|t| t.url.contains(&wanted))
    {
        perso.active_tab_id = Some(found.id.clone());
    }
    // SIGNET_URL: replaces the active tab's address.
    if let Some(url) = var("SIGNET_URL")
        && let Some(t) = perso.active_tab_id.as_ref().and_then(|id| state.tabs.get_mut(id))
    {
        t.url = url;
    }
    // SIGNET_SPLIT=columns|rows|grid (and SIGNET_SPLIT_N=2..4): the first today tabs form a split view.
    let layout = match var("SIGNET_SPLIT").as_deref() {
        Some("columns") => Some(SplitLayout::Columns),
        Some("rows") => Some(SplitLayout::Rows),
        Some("grid") => Some(SplitLayout::Grid),
        _ => None,
    };
    if let Some(layout) = layout {
        let n = var("SIGNET_SPLIT_N").and_then(|v| v.parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(4).clamp(2, 4);
        let n = n.min(perso.today.len());
        let tab_ids: Vec<String> = perso.today.drain(..n).filter_map(|i| if let Item::Tab(t) = i { Some(t.tab_id) } else { None }).collect();
        perso.today.insert(0, Item::Split(SplitItem { id: "vue-partagee".into(), group: None, tab_ids, layout: Some(layout), sizes: None }));
    }
    // SIGNET_SIDEBAR=0: sidebar hidden.
    if var("SIGNET_SIDEBAR").as_deref() == Some("0") {
        state.sidebar_visible = false;
    }
    // SIGNET_LAYOUT=horizontal: tabs on top, with fake downloads.
    if var("SIGNET_LAYOUT").as_deref() == Some("horizontal") {
        state.settings.tab_layout = Some(TabLayout::Horizontal);
        let now = now_ms();
        let dl = |id: &str, name: &str, path: &str, url: &str, status, got: u64, total: u64, at: i64| DownloadInfo {
            id: id.into(),
            filename: name.into(),
            path: path.into(),
            url: url.into(),
            status,
            received_bytes: got,
            total_bytes: total,
            started_at: at,
            private: None,
        };
        state.downloads = vec![
            dl("dl-1", "rapport-annuel-2026.pdf", "/tmp/rapport.pdf", "https://example.com/r.pdf", DownloadStatus::Progressing, 3_200_000, 8_000_000, now),
            dl("dl-2", "photos-vacances.zip", "/tmp/photos.zip", "https://example.com/p.zip", DownloadStatus::Completed, 52_000_000, 52_000_000, now - 60_000),
        ];
    }
    // SIGNET_TABS=<n>: n today tabs in total.
    let want = var("SIGNET_TABS").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    const TITLES: [&str; 20] = [
        "Docs React", "MDN Web Docs", "Stack Overflow", "Wikipédia · Océan", "npm", "TypeScript", "Le Monde · Sport", "YouTube", "OpenStreetMap", "Electron - Releases", "CSS-Tricks",
        "Hacker News · Show", "Can I use", "W3C · CSS", "Lobsters", "GitHub · Issues", "Vite", "Rust Book", "Python docs", "Linux Kernel",
    ];
    for n in perso.today.len()..want {
        let title = format!("{} {}", TITLES[n % TITLES.len()], n / TITLES.len() + 1);
        perso.today.push(Item::tab(make(state, &title, "https://example.com/", ((n * 47) % 360) as u32)));
    }
    let mut travail = new_space(CleanDraft { name: "Travail".into(), icon: "💼".into(), hue: 28.0, theme: None });
    // Batch D: renamed tabs (title only, title + emoji), isolated space and sample archives.
    let today_tab = |state: &signet_core::core::store::State, perso: &signet_core::types::Space, title: &str| -> Option<String> {
        perso.today.iter().find_map(|i| match i {
            Item::Tab(t) if state.tabs.get(&t.tab_id).is_some_and(|x| x.title == title) => Some(t.tab_id.clone()),
            _ => None,
        })
    };
    if let Some(id) = today_tab(state, &perso, "Hacker News")
        && let Some(t) = state.tabs.get_mut(&id)
    {
        t.custom_title = Some("Veille techno".into());
        t.custom_icon = Some("📰".into());
    }
    if let Some(id) = today_tab(state, &perso, "Example Domain")
        && let Some(t) = state.tabs.get_mut(&id)
    {
        t.custom_icon = Some("🌍".into());
    }
    travail.profile = Some(SpaceProfile::Isolated);
    // Batch C: sample routing rules.
    state.settings.routes = vec![
        SpaceRoute { id: "route-gh".into(), pattern: "github.com".into(), space_id: travail.id.clone() },
        SpaceRoute { id: "route-notion".into(), pattern: "*.notion.site".into(), space_id: travail.id.clone() },
        SpaceRoute { id: "route-wiki".into(), pattern: "wikipedia.org".into(), space_id: perso.id.clone() },
    ];
    let perso_id = perso.id.clone();
    let mut spaces = vec![perso, travail];
    // SIGNET_SPACES=<n>: extra empty spaces (icon row by pages); SIGNET_ACTIVE_SPACE=<i>: active space.
    const EXTRA: [(&str, &str); 10] = [("Lecture", "📚"), ("Sport", "🚴"), ("Cuisine", "🍳"), ("Voyages", "✈️"), ("Musique", "🎧"), ("Projets", "🧪"), ("Jeux", "🎮"), ("Famille", "🌻"), ("Finances", "💶"), ("Photo", "📷")];
    let count = var("SIGNET_SPACES").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    for n in 2..count {
        let (name, icon) = EXTRA[(n - 2) % EXTRA.len()];
        spaces.push(new_space(CleanDraft { name: name.into(), icon: icon.into(), hue: ((n * 53) % 360) as f64, theme: None }));
    }
    let active = var("SIGNET_ACTIVE_SPACE").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    state.active_space_id = spaces.get(active).map(|s| s.id.clone()).unwrap_or(perso_id);
    state.spaces = spaces;
    state.favorites = favorites;
    let archived = |title: &str, url: &str, hue: u32, hours: i64| {
        let at = now_ms() - hours * 3_600_000;
        ArchivedTab { id: Some(format!("{at}:{url}")), url: url.into(), title: title.into(), favicon: Some(letter_icon(&first_upper(title), hue)), archived_at: at }
    };
    state.archive = vec![
        archived("Électron - Documentation", "https://www.electronjs.org/fr/docs", 200, 2),
        archived("HTML - Wikipédia", "https://fr.wikipedia.org/wiki/Hypertext_Markup_Language", 215, 5),
        archived("Le Monde - Éditorial", "https://www.lemonde.fr/idees/", 0, 27),
        archived("Wikipédia · Cyclisme", "https://fr.wikipedia.org/wiki/Cyclisme", 210, 30),
        archived("Open Street Map", "https://www.openstreetmap.org/", 140, 80),
        archived("Notes de version", "https://github.com/electron/electron/releases", 270, 400),
    ];
}

/// Sample history (only with `SIGNET_SEED=1`), for the Library capture.
pub fn seed_history(history: &mut History) {
    if !enabled() || !flag("SIGNET_SEED") {
        return;
    }
    history.clear();
    let rows: [(&str, &str, u32, f64); 11] = [
        ("Wikipédia, l’encyclopédie libre", "https://www.wikipedia.org/", 210, 0.2),
        ("Hacker News", "https://news.ycombinator.com/", 25, 0.6),
        ("GitHub - Where software is built", "https://github.com/", 260, 1.5),
        ("Example Domain", "https://example.com/", 150, 3.0),
        ("W3C - Standards du Web", "https://www.w3.org/", 200, 7.0),
        ("Le Monde - À la une", "https://www.lemonde.fr/", 0, 26.0),
        ("Wikipédia · Le libre", "https://fr.wikipedia.org/", 210, 29.0),
        ("Électron - Documentation", "https://www.electronjs.org/fr/docs", 200, 33.0),
        ("HTML - Wikipédia", "https://fr.wikipedia.org/wiki/Hypertext_Markup_Language", 215, 52.0),
        ("Open Street Map", "https://www.openstreetmap.org/", 140, 75.0),
        ("Notes de version", "https://github.com/electron/electron/releases", 270, 420.0),
    ];
    let now = now_ms() as f64;
    let list: Vec<ImportedEntry> =
        rows.iter().map(|(title, url, _hue, hours)| ImportedEntry { url: (*url).into(), title: (*title).into(), visits: 1, typed: None, last_visit: now - hours * 3_600_000.0 }).collect();
    history.import_entries(&list);
    for (title, url, hue, _) in rows {
        history.update(url, None, Some(&letter_icon(&first_upper(title), hue)));
    }
}
