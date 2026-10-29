//! The sessions popup: the named sessions on this machine and the ones each
//! saved device reports, grouped under the device they run on and led by the
//! device this window is on.
//! Both lists are probes — a session socket outlives its daemon, and a device's
//! sessions cost an SSH round trip — so this page is the only thing that asks.
use super::{Page, colors};
use crate::{HerdrWindow, sessions::DeviceScan};
use gpui::{prelude::*, *};
use herdr_client::{ConnectTarget, LocalSession, SessionWorkspace};
mod catalog;
mod entries;
mod facets;
mod management;
mod projects;
mod row;
mod view;
pub(super) use catalog::Filter;
#[cfg(any(unix, test))]
pub(crate) use catalog::{Facets, ProjectPick};
pub(super) use facets::Facet;
pub(super) use management::Edit;
#[cfg(test)]
pub(in crate::menu) use management::Target as AddTarget;
pub(super) use projects::ProjectEdit;
pub(super) use view::View;

/// The manager is wide: a row carries a project, a branch and a state.
pub(super) const SESSIONS_WIDTH: f32 = 440.;

/// The page-sized layout's widest extent.
pub(super) const EXPANDED_WIDTH: f32 = 960.;

/// A line in the popup, in the order it is painted. Only a row can be selected,
/// so navigation walks the `Row` entries and skips headers and notes.
#[derive(Clone)]
pub(super) enum Entry {
    /// A section title. It carries a selector because the order of the sections
    /// is what says which device the list is leading with, and a device's own
    /// title is named after it.
    Header {
        selector: String,
        text: String,
    },
    /// A muted note, carrying the selector diagnostics and tests look for.
    Muted {
        selector: String,
        text: String,
    },
    Error(String),
    /// A selectable line, carrying what it paints: a row resolved again at paint
    /// time could describe a session its own scan has already replaced.
    Row {
        row: Row,
        spec: Spec,
    },
}

/// What a row attaches to. A device keeps its endpoint id rather than an index,
/// so a catalog change between paint and click cannot pick the wrong one, and it
/// names the session itself, because a device has many.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Local(String),
    Device { id: String, session: String },
    Add(management::Target),
}

/// What one row paints. Both kinds share one shape so they cannot drift apart.
#[derive(Clone)]
pub(super) struct Spec {
    label: SharedString,
    pub(in crate::menu) detail: SharedString,
    /// The saved workspace context line, when the session has any.
    context: Option<SharedString>,
    /// Running locally, or connected remotely.
    online: bool,
    checked: bool,
    enabled: bool,
}

impl Row {
    /// A real session, as opposed to the Add action.
    fn is_session(&self) -> bool {
        !matches!(self, Self::Add(_))
    }
}

impl Entry {
    pub(super) fn row(&self) -> Option<&Row> {
        match self {
            Self::Row { row, .. } => Some(row),
            _ => None,
        }
    }
}

use crate::containers::session_ref;

/// The state word a session paints, whether it was probed on this machine or
/// reported by its host.
fn state_word(running: bool) -> &'static str {
    if running { "running" } else { "stopped" }
}

impl HerdrWindow {
    pub(super) fn finish_session_departure(&mut self) {
        // Selection is an ordinal shared with keyboard navigation. Once a row
        // disappears it must not transfer to the action now occupying its slot.
        if self.sessions.departure.is_some() && self.menu.page == Some(Page::Sessions) {
            self.menu.selected = None;
        }
        self.sessions.finish_departure();
    }

    /// Open the local and remote session list above the footer button.
    pub(crate) fn open_sessions(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open_menu(window, cx) {
            return;
        }
        // All entry points hand the screen and keyboard to this picker,
        // including the native Window menu while a tab menu is open. Drop the
        // old overlay without restoring its focus over the new search input.
        self.container_menu = None;
        // Anchor to the control, not the pointer: every click position leaves the
        // same clear gap above the button.
        self.menu.anchor = anchor;
        // Probe again on every open. The list from the last open paints at once,
        // so the popup is never blank while the new scan runs.
        self.sessions.refresh();
        self.sync_projects(cx);
        self.menu.page = Some(Page::Sessions);
        self.open_sessions_view(window, cx);
        self.pin_first_session();
    }

    /// Opens the picker straight on the new-session name form for this machine.
    pub(crate) fn open_sessions_creating(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_sessions(anchor, window, cx);
        if self.menu.page == Some(Page::Sessions) {
            self.open_session_create(management::Target::Local, window, cx);
        }
    }

    pub(super) fn render_sessions(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.menu.session_edit.is_some() {
            return self.render_session_edit(cx).into_any_element();
        }
        self.render_session_list(cx)
    }

    pub(super) fn render_session_list(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .when(!self.sessions_expanded(), |list| {
                // An open facet adds its scroller and a few rows on top, so
                // the sessions below keep room however small the window is.
                let facet = if self.session_facet_open() {
                    self.facet_room() + px(self.config.ui.line_height() * 3.)
                } else {
                    px(0.)
                };
                // Chrome bounds the picker to the chosen side of its anchor;
                // this content cap must not assume the anchor is in the footer.
                list.max_h(px(540.) + facet)
            })
            .child(self.render_session_toolbar(cx))
            .child(self.session_rows(cx))
            .into_any_element()
    }

    fn session_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let font = &self.config.ui;
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        let selected = self.menu.selected;
        let mut view = div()
            .id("sessions-scroll")
            .debug_selector(|| "sessions-list".into())
            .flex_1()
            .min_h(px(if self.sessions_expanded() { 48. } else { 80. }))
            .overflow_y_scroll()
            .track_scroll(&self.menu.sessions_scroll)
            .flex()
            .flex_col()
            .gap(px(4.));
        for (child, entry) in entries.iter().enumerate() {
            let ordinal = children.iter().position(|index| *index == child);
            view = match entry {
                Entry::Header { selector, text } => {
                    let selector = selector.clone();
                    view.child(
                        div()
                            .debug_selector(move || selector.clone())
                            .p(px(8.))
                            .text_color(rgb(theme.muted))
                            .child(text.clone()),
                    )
                }
                Entry::Muted { selector, text } => {
                    let selector = selector.clone();
                    view.child(
                        div()
                            .debug_selector(move || selector.clone())
                            .px(px(8.))
                            .py(px(2.))
                            .text_size(px(font.size * 0.85))
                            .text_color(rgb(theme.muted))
                            .child(text.clone()),
                    )
                }
                Entry::Error(text) => view.child(
                    div()
                        .debug_selector(|| "sessions-error".into())
                        .px(px(8.))
                        .py(px(2.))
                        .text_size(px(font.size * 0.85))
                        .text_color(colors::danger(theme))
                        .child(text.clone()),
                ),
                Entry::Row { row, spec } => view.child(self.sessions_row(
                    ordinal.unwrap_or_default(),
                    row.clone(),
                    ordinal == selected,
                    spec,
                    cx,
                )),
            };
        }
        view.into_any_element()
    }

    /// Attach to a row's session. The row is what the caller painted, not an
    /// index into a list computed again here: a device list that refreshed in
    /// between would otherwise have the click name a different session.
    fn choose_session(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.mutation.is_some() {
            return;
        }
        match row {
            Row::Add(target) => self.open_session_create(target, window, cx),
            Row::Local(name) => {
                self.dismiss_menu(window, cx);
                self.attach_session(session_ref(crate::endpoint::LOCAL, name), cx);
                self.reset_sidebar_reveal();
            }
            // A device row is one of that device's sessions. It becomes a
            // workspace row of the current logical Window beside the others.
            Row::Device { id, session } => {
                // A disabled device has nothing to attach to, and the picker
                // refuses the same click.
                if !self.endpoints.iter().any(|endpoint| {
                    endpoint.slot.is_none() && endpoint.id == id && endpoint.enabled
                }) {
                    return;
                }
                self.dismiss_menu(window, cx);
                self.attach_session(session_ref(&id, session), cx);
                self.reset_sidebar_reveal();
            }
        }
    }

    /// The new row's tree is not the old one's: scroll and reveal restart.
    fn reset_sidebar_reveal(&self) {
        for scroll in &self.sidebar_scroll {
            scroll.set_offset(Point::default());
        }
        for revealed in &self.sidebar_revealed {
            revealed.set(None);
        }
        // The row a reveal left for the next frame is in the old list.
        self.sidebar_pin_reveal.set(None);
    }

    pub(super) fn sessions_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(&self.menu.session_edit, Some(Edit::Create { input, .. })
            if input.read(cx).is_composing() || !matches!(event.keystroke.key.as_str(), "enter" | "escape"))
            || matches!(&self.menu.session_edit, Some(Edit::Project(edit))
                if edit.input().is_some_and(|input| input.read(cx).is_composing())
                    || (edit.input().is_some() && !matches!(event.keystroke.key.as_str(), "enter" | "escape")))
        {
            return;
        }
        // Everything else, Backspace and Delete included, belongs to the search
        // field: it edits text and can never reach a destructive action. The
        // field takes focus back when a closed modal left it on the menu.
        let search_focused = self
            .menu
            .sessions_view
            .as_ref()
            .is_some_and(|view| view.search.read(cx).focus.is_focused(window));
        if self.menu.session_edit.is_none() && !search_focused {
            self.focus_session_search(window, cx);
        }
        let own = matches!(
            event.keystroke.key.as_str(),
            "up" | "down" | "enter" | "escape" | "tab" | "pagedown" | "pageup"
        );
        if self.menu.session_edit.is_none() && !own {
            return;
        }
        // An IME composition owns Enter, arrows and Escape until it commits.
        if self.menu.session_edit.is_none()
            && self
                .menu
                .sessions_view
                .as_ref()
                .is_some_and(|view| view.search.read(cx).is_composing())
        {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        if self.menu.session_edit.is_some() {
            self.session_edit_key(event, window, cx);
            return;
        }
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        match event.keystroke.key.as_str() {
            "up" | "down" if !children.is_empty() => {
                let count = children.len();
                let up = event.keystroke.key == "up";
                let index = match self.menu.selected {
                    Some(index) => (index.min(count - 1) + if up { count - 1 } else { 1 }) % count,
                    None if up => count - 1,
                    None => 0,
                };
                self.menu.selected = Some(index);
                let pinned = children
                    .get(index)
                    .and_then(|child| entries.get(*child))
                    .and_then(Entry::row)
                    .cloned();
                self.pin_session(pinned);
                self.menu.sessions_scroll.scroll_to_item(children[index]);
                cx.notify();
            }
            "enter" if !children.is_empty() => {
                let row = self
                    .menu
                    .selected
                    .and_then(|selected| children.get(selected))
                    .and_then(|child| entries.get(*child))
                    .and_then(Entry::row)
                    .cloned();
                if let Some(row) = row {
                    self.choose_session(row, window, cx);
                }
            }
            "escape" => self.dismiss_menu(window, cx),
            "tab" => {
                let next = self.session_filter().next();
                self.set_session_filter(next, cx);
            }
            "pagedown" | "pageup" => {
                let (_, pages) = self.session_page_entries();
                let page = self.session_page().min(pages - 1);
                let target = if event.keystroke.key == "pagedown" {
                    (page + 1).min(pages - 1)
                } else {
                    page.saturating_sub(1)
                };
                self.set_session_page(target, cx);
            }
            _ => {}
        }
    }
}
