//! Ports `packages/shared/src/model.ts`: factories and sanitizers of the data model (spaces, tabs).
//! (Named `factory` because `signet_core::model` is the port of `main/model`.)

use crate::ids::{new_id, now_ms};
use crate::types::{Space, SpaceDraft, SpaceTheme, TabInfo};

/// Draft already cleaned by `clean_space_draft`.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanDraft {
    pub name: String,
    pub icon: String,
    pub hue: f64,
    pub theme: Option<SpaceTheme>,
}

pub fn new_space(draft: CleanDraft) -> Space {
    Space { id: new_id(), name: draft.name, icon: draft.icon, hue: draft.hue, theme: draft.theme, ..Default::default() }
}

fn clean_hue(h: f64) -> Option<f64> {
    if h.is_finite() {
        let r = (h + 0.5).floor();
        Some(((r % 360.0) + 360.0) % 360.0)
    } else {
        None
    }
}

/// Space draft coming from the UI, validated: non-empty name and emoji, hue 0-359, gradient of 2 or 3 hues
/// (otherwise solid color). With a gradient, `hue` is its first hue. `default_name`: (translated) name of an unnamed space.
pub fn clean_space_draft(draft: &SpaceDraft, default_name: &str) -> CleanDraft {
    let name = if draft.name.trim().is_empty() { default_name.to_string() } else { draft.name.trim().chars().take(40).collect() };
    let icon = if draft.icon.trim().is_empty() { "\u{2728}".to_string() } else { draft.icon.trim().chars().take(16).collect() };
    let hues: Vec<f64> = draft.theme.as_ref().map(|t| t.hues.iter().filter_map(|h| clean_hue(*h)).take(3).collect()).unwrap_or_default();
    if hues.len() >= 2 {
        let grain = draft.theme.as_ref().map(|t| t.grain).unwrap_or(0.0);
        let grain = if grain.is_finite() { grain.clamp(0.0, 1.0) } else { 0.0 };
        return CleanDraft { name, icon, hue: hues[0], theme: Some(SpaceTheme { hues, grain }) };
    }
    CleanDraft { name, icon, hue: clean_hue(draft.hue).unwrap_or(250.0), theme: None }
}

pub fn new_tab(url: &str, title: &str) -> TabInfo {
    TabInfo { id: new_id(), url: url.into(), title: title.into(), last_active: now_ms(), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factories() {
        let s = new_space(CleanDraft { name: "N".into(), icon: "i".into(), hue: 3.0, theme: None });
        assert_eq!((s.name.as_str(), s.icon.as_str(), s.hue), ("N", "i", 3.0));
        assert!(s.pinned.is_empty() && s.today.is_empty() && s.active_tab_id.is_none());
        assert_eq!(s.id.len(), 36);
        let t = new_tab("https://a.com", "A");
        assert_eq!((t.url.as_str(), t.title.as_str()), ("https://a.com", "A"));
        assert!(!t.loading && !t.loaded && !t.muted);
        assert_eq!(new_tab("x", "").title, "");
    }

    fn draft(name: &str, icon: &str, hue: f64, theme: Option<SpaceTheme>) -> SpaceDraft {
        SpaceDraft { name: name.into(), icon: icon.into(), hue, theme }
    }

    #[test]
    fn draft_defaults_and_bounds() {
        let d = clean_space_draft(&SpaceDraft::default(), "Personal");
        assert_eq!((d.name.as_str(), d.icon.as_str(), d.hue, d.theme.is_none()), ("Personal", "\u{2728}", 0.0, true));
        let d = clean_space_draft(&draft("  Perso  ", " \u{1f3e0} ", -10.0, None), "Personal");
        assert_eq!((d.name.as_str(), d.icon.as_str(), d.hue), ("Perso", "\u{1f3e0}", 350.0));
        assert_eq!(clean_space_draft(&draft(&"x".repeat(100), "i", 725.6, None), "Personal").name.chars().count(), 40);
        assert_eq!(clean_space_draft(&draft("a", "i", 725.6, None), "Personal").hue, 6.0);
        assert_eq!(clean_space_draft(&draft("a", "i", f64::NAN, None), "Personal").hue, 250.0);
    }

    #[test]
    fn draft_gradient() {
        let d = clean_space_draft(&draft("a", "i", 1.0, Some(SpaceTheme { hues: vec![10.0, 370.0, 30.0, 40.0], grain: 5.0 })), "Personal");
        assert_eq!(d.hue, 10.0);
        assert_eq!(d.theme, Some(SpaceTheme { hues: vec![10.0, 10.0, 30.0], grain: 1.0 }));
        let one = clean_space_draft(&draft("a", "i", 99.0, Some(SpaceTheme { hues: vec![10.0], grain: 0.5 })), "Personal");
        assert_eq!((one.hue, one.theme), (99.0, None));
        let nan = clean_space_draft(&draft("a", "i", 1.0, Some(SpaceTheme { hues: vec![1.0, 2.0], grain: f64::NAN })), "Personal");
        assert_eq!(nan.theme.unwrap().grain, 0.0);
    }
}
