use crate::icon::IconName;
use crate::theme::{ThemeExt, space};
use gpui::{px, App, ClickEvent, Context, ElementId, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window, div};
use gpui_component::{
    Icon, Selectable,
    button::Button,
    menu::{ContextMenu, ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One entry of a menu. Cheap to clone, so the same list can rebuild the menu each time it opens.
#[derive(Clone)]
pub enum MenuEntry {
    Item { label: SharedString, icon: Option<IconName>, shortcut: Option<SharedString>, checked: bool, disabled: bool, danger: bool, on_click: Handler },
    Separator,
    Label(SharedString),
    Submenu { label: SharedString, icon: Option<IconName>, entries: Vec<MenuEntry> },
}

impl MenuEntry {
    pub fn item(label: impl Into<SharedString>, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        MenuEntry::Item { label: label.into(), icon: None, shortcut: None, checked: false, disabled: false, danger: false, on_click: Rc::new(on_click) }
    }
    pub fn separator() -> Self {
        MenuEntry::Separator
    }
    pub fn label(label: impl Into<SharedString>) -> Self {
        MenuEntry::Label(label.into())
    }
    pub fn submenu(label: impl Into<SharedString>, entries: Vec<MenuEntry>) -> Self {
        MenuEntry::Submenu { label: label.into(), icon: None, entries }
    }
    pub fn icon(mut self, i: IconName) -> Self {
        match &mut self {
            MenuEntry::Item { icon, .. } | MenuEntry::Submenu { icon, .. } => *icon = Some(i),
            _ => {}
        }
        self
    }
    /// Shortcut text shown on the right (see [`super::format_accelerator`]).
    pub fn shortcut(mut self, s: impl Into<SharedString>) -> Self {
        if let MenuEntry::Item { shortcut, .. } = &mut self {
            *shortcut = Some(s.into());
        }
        self
    }
    pub fn checked(mut self, c: bool) -> Self {
        if let MenuEntry::Item { checked, .. } = &mut self {
            *checked = c;
        }
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        if let MenuEntry::Item { disabled, .. } = &mut self {
            *disabled = d;
        }
        self
    }
    /// Destructive action: danger color.
    pub fn danger(mut self, d: bool) -> Self {
        if let MenuEntry::Item { danger, .. } = &mut self {
            *danger = d;
        }
        self
    }
}

/// Fills a gpui-component [`PopupMenu`] from entries.
pub fn build_menu(menu: PopupMenu, entries: &[MenuEntry], window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let mut menu = menu;
    for e in entries {
        menu = match e.clone() {
            MenuEntry::Separator => menu.separator(),
            MenuEntry::Label(l) => menu.label(l),
            MenuEntry::Submenu { label, icon, entries } => {
                menu.submenu_with_icon(icon.map(Icon::from), label, window, cx, move |m, w, cx| build_menu(m, &entries, w, cx))
            }
            MenuEntry::Item { label, icon, shortcut, checked, disabled, danger, on_click } => {
                let item = if shortcut.is_some() || danger {
                    let l = label.clone();
                    PopupMenuItem::element(move |_w, cx| {
                        let t = cx.signet();
                        let mut row = div().flex().items_center().w_full().gap(space::S3).child(div().flex_1().child(l.clone()));
                        if danger {
                            row = row.text_color(t.danger);
                        }
                        if let Some(s) = shortcut.clone() {
                            row = row.child(div().text_size(t.type_scale.cap).text_color(t.text_2).child(s));
                        }
                        row
                    })
                } else {
                    PopupMenuItem::new(label)
                };
                let item = match icon {
                    Some(i) => item.icon(Icon::from(i)),
                    None => item,
                };
                menu.item(item.checked(checked).disabled(disabled).on_click(move |e, w, cx| on_click(e, w, cx)))
            }
        };
    }
    menu
}

/// Adds a right-click menu to an element.
///
/// ```ignore
/// context_menu(div().id("row").child("Tab"), vec![MenuEntry::item("Close", |_, _, _| {})])
/// ```
pub fn context_menu<E>(el: E, entries: Vec<MenuEntry>) -> ContextMenu<E>
where
    E: InteractiveElement + ParentElement + Styled + IntoElement + 'static,
{
    el.context_menu(move |m, w, cx| build_menu(m, &entries, w, cx))
}

/// Turns a button into a menu trigger (opens below it on click).
pub fn dropdown_menu(id: impl Into<ElementId>, button: Button, entries: Vec<MenuEntry>) -> impl IntoElement {
    let _ = id.into();
    button.dropdown_menu(move |m, w, cx| {
        // Long menus scroll inside the window (like a native select) instead of running off its edges.
        let cap = (w.viewport_size().height - px(32.0)).max(px(120.0));
        build_menu(m.scrollable(true).max_h(cap), &entries, w, cx)
    })
}

#[allow(dead_code)]
fn _selectable_bound<T: Selectable>() {}
