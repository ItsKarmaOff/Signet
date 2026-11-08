//! Seams to the screens other modules deliver: the sidebar and its hosted panels, the tab strip, the dividers of a
//! split view and the developer tools / responsive mode cards.
//!

use crate::app_state::{snapshot, ui_state};
use crate::screens::settings::{SpaceFormClosed, SpaceFormView};
use crate::screens::sidebar::{Sidebar, SidebarEvent};
use crate::screens::tabs::{Dividers, TabStrip};
use crate::screens::windows::downloads::DownloadsPanel;
use crate::theme::{ThemeExt, radius};
use gpui::{AnyView, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use std::cell::Cell;
use std::rc::Rc;

/// Content of the sidebar's downloads panel: the app's downloads, live.
struct DownloadsHost;

impl Render for DownloadsHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
        div().size_full().rounded(radius::POP).bg(cx.signet().glass_view).child(DownloadsPanel::new(snapshot(cx).downloads.clone()))
    }
}

/// Builds the sidebar and hosts its panels: the space form (edited here, closed when done) and the downloads panel.
/// `V` is the view that owns the sidebar; the returned subscription must live as long as it.
pub fn sidebar<V: 'static>(window: &mut Window, floating: bool, cx: &mut Context<V>) -> (Entity<Sidebar>, gpui::Subscription) {
    let sidebar = cx.new(|cx| if floating { Sidebar::new_floating(window, cx) } else { Sidebar::new(window, cx) });
    let downloads_open = Rc::new(Cell::new(false));
    let sub = cx.subscribe_in(&sidebar, window, move |_, sb, event: &SidebarEvent, window, cx| match event {
        SidebarEvent::EditSpace(id) => {
            downloads_open.set(false);
            let space = id.as_ref().and_then(|id| snapshot(cx).spaces.iter().find(|s| &s.id == id).cloned());
            let form = cx.new(|cx| SpaceFormView::new(space, window, cx));
            let target = sb.clone();
            cx.subscribe(&form, move |_, _, _: &SpaceFormClosed, cx| {
                target.update(cx, |s, cx| s.set_space_form(None, cx));
            })
            .detach();
            sb.update(cx, |s, cx| s.set_space_form(Some(AnyView::from(form)), cx));
        }
        SidebarEvent::ToggleDownloads => {
            let open = !downloads_open.get();
            downloads_open.set(open);
            let view = open.then(|| AnyView::from(cx.new(|_| DownloadsHost)));
            sb.update(cx, |s, cx| s.set_downloads_panel(view, cx));
        }
    });
    (sidebar, sub)
}

/// The row of tabs at the top (`horizontal` layout); `floating`: the compact mode's floating bar.
pub fn tab_strip(window: &mut Window, cx: &mut App, floating: bool) -> AnyView {
    cx.new(|cx| if floating { TabStrip::new_floating(window, cx) } else { TabStrip::new(window, cx) }).into()
}

/// One view over the whole window drawing the dividers of the displayed split view.
pub fn dividers<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Entity<Dividers> {
    cx.new(|cx| Dividers::new(window, cx))
}
