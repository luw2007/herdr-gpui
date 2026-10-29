//! The manager's own state: the search field, the run-state filter and the
//! session the selection is pinned to. Selection is pinned by identity because
//! the list is rebuilt by asynchronous scans, and an ordinal would slide onto a
//! different session whenever a row appeared or went away above it.
use super::catalog::{Facets, Scope};
use super::{Entry, Facet, Filter, Page, Row};
use crate::{
    HerdrWindow,
    search_input::{Changed, SearchInput},
};
use gpui::{prelude::*, *};

pub(in crate::menu) struct View {
    pub(super) search: Entity<SearchInput>,
    /// Run state and the explicit device and project filters.
    pub(super) scope: Scope,
    /// Whether the manager paints at page size rather than as a popover.
    pub(super) expanded: bool,
    /// The facet whose choices are listed under the toolbar.
    pub(super) facet_open: Option<Facet>,
    /// The search text as of the last edit, so building the list never reads
    /// the entity.
    pub(super) query: String,
    /// The row the selection follows.
    pub(super) pinned: Option<Row>,
    _subscription: Subscription,
    /// Zero-based page of matches; reset by every query or filter change.
    pub(super) page: usize,
}

impl HerdrWindow {
    /// Open the manager with `query` already typed into its search.
    #[cfg(unix)]
    pub(crate) fn open_sessions_with_query(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.sessions_anchor.get();
        self.open_sessions(anchor, window, cx);
        if self.menu.page != Some(Page::Sessions) {
            return;
        }
        let search = self
            .menu
            .sessions_view
            .as_ref()
            .map(|view| view.search.clone());
        if let Some(search) = search {
            search.update(cx, |input, cx| input.set_text_selected(query, cx));
            if let Some(view) = self.menu.sessions_view.as_mut() {
                view.query = query.to_owned();
            }
        }
        self.reselect_first_session();
        cx.notify();
    }

    /// The footer's device filter is copied in, not shared: this list starts
    /// where the sidebar is and then moves on its own.
    pub(super) fn open_sessions_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(SearchInput::new);
        search.update(cx, |input, cx| {
            input.set_placeholder("Search project, branch, path, session, device...", cx);
            input.set_appearance(self.config.ui.clone(), self.theme.clone(), cx);
        });
        let subscription = cx.subscribe(&search, |this, search, _: &Changed, cx| {
            let text = search.read(cx).text().to_owned();
            if let Some(view) = &mut this.menu.sessions_view {
                view.query = text;
            }
            this.reselect_first_session();
            cx.notify();
        });
        window.focus(&search.read(cx).focus.clone(), cx);
        self.menu.sessions_view = Some(View {
            search,
            scope: Scope {
                facets: Facets {
                    device: self.device_filter.clone(),
                    ..Default::default()
                },
                ..Default::default()
            },
            expanded: false,
            facet_open: None,
            query: String::new(),
            pinned: None,
            page: 0,
            _subscription: subscription,
        });
    }

    pub(super) fn focus_session_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = &self.menu.sessions_view {
            window.focus(&view.search.read(cx).focus.clone(), cx);
        } else {
            window.focus(&self.menu.focus, cx);
        }
    }

    pub(super) fn set_session_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.change_scope(|scope| scope.filter = filter, cx);
    }

    /// Apply `change` to the scope; a scope that really changed starts the list
    /// again from its first match.
    pub(in crate::menu) fn change_scope(
        &mut self,
        change: impl FnOnce(&mut Scope),
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = &mut self.menu.sessions_view {
            let before = view.scope.clone();
            change(&mut view.scope);
            if view.scope == before {
                return;
            }
        }
        self.reselect_first_session();
        cx.notify();
    }

    /// A changed query or filter starts again from the first match.
    fn reselect_first_session(&mut self) {
        if let Some(view) = &mut self.menu.sessions_view {
            view.page = 0;
        }
        self.pin_first_session();
        self.menu.sessions_scroll.set_offset(Point::default());
    }

    /// Re-find the pinned session in a freshly built list. Called after every
    /// scan answer. A pinned session that has gone drops the selection rather
    /// than handing it to whichever row slid into its place.
    pub(crate) fn sync_session_selection(&mut self) {
        if self.menu.page != Some(Page::Sessions) {
            return;
        }
        let Some(pinned) = self
            .menu
            .sessions_view
            .as_ref()
            .and_then(|view| view.pinned.clone())
        else {
            // Nothing pinned yet, as when the first scan had not answered at
            // open: take the first session that has arrived.
            if self.menu.selected.is_none() && self.sessions.departure.is_none() {
                self.pin_first_session();
            }
            return;
        };
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        let ordinal = children
            .iter()
            .position(|child| entries.get(*child).and_then(Entry::row) == Some(&pinned));
        if ordinal.is_some() {
            self.menu.selected = ordinal;
        } else if let Some(view) = &mut self.menu.sessions_view {
            // The session left the list: no row inherits its selection.
            view.pinned = None;
            self.menu.selected = None;
        }
    }

    /// Record `row` as what the selection follows.
    pub(in crate::menu) fn pin_session(&mut self, row: Option<Row>) {
        if let Some(view) = &mut self.menu.sessions_view {
            view.pinned = row;
        }
    }
    pub(in crate::menu) fn session_page(&self) -> usize {
        self.menu.sessions_view.as_ref().map_or(0, |view| view.page)
    }

    pub(super) fn set_session_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.menu.sessions_view {
            view.page = page;
        }
        self.reselect_first_session_keep_page();
        cx.notify();
    }

    fn reselect_first_session_keep_page(&mut self) {
        self.pin_first_session();
        self.menu.sessions_scroll.set_offset(Point::default());
    }

    /// Pin the first real session of the current page. The Add action is never
    /// pinned this way: before the first scan answers it would be all there is,
    /// and Enter would open its form instead of attaching.
    pub(super) fn pin_first_session(&mut self) {
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        let first = children
            .iter()
            .position(|child| entries[*child].row().is_some_and(Row::is_session));
        self.menu.selected = first;
        let row = first.and_then(|ordinal| entries[children[ordinal]].row().cloned());
        self.pin_session(row);
    }

    pub(super) fn session_filter(&self) -> Filter {
        self.session_scope().filter
    }

    pub(in crate::menu) fn sessions_expanded(&self) -> bool {
        self.menu
            .sessions_view
            .as_ref()
            .is_some_and(|view| view.expanded)
    }

    pub(super) fn toggle_sessions_expanded(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.menu.sessions_view {
            view.expanded = !view.expanded;
        }
        cx.notify();
    }

    pub(in crate::menu) fn session_facet_open(&self) -> bool {
        self.menu
            .sessions_view
            .as_ref()
            .is_some_and(|view| view.facet_open.is_some())
    }
    pub(in crate::menu) fn session_scope(&self) -> Scope {
        self.menu
            .sessions_view
            .as_ref()
            .map(|view| view.scope.clone())
            .unwrap_or_default()
    }

    /// The manager's own device filter, for tests outside this module that
    /// open it through the control socket.
    #[cfg(test)]
    pub(crate) fn session_device_filter(&self) -> Option<String> {
        self.session_scope().facets.device
    }

    /// Open the manager with `query` typed and `facets` chosen, as
    /// `herdr-gpui sessions` asks.
    #[cfg(unix)]
    pub(crate) fn open_sessions_filtered(
        &mut self,
        query: &str,
        facets: Facets,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_sessions_with_query(query, window, cx);
        if self.menu.page == Some(Page::Sessions) {
            self.change_scope(|scope| scope.facets = facets, cx);
        }
    }

    pub(in crate::menu) fn session_query(&self) -> &str {
        self.menu
            .sessions_view
            .as_ref()
            .map_or("", |view| view.query.as_str())
    }

    /// Search bar and the three run-state filters above the list.
    pub(super) fn render_session_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let mut filters = div().flex().flex_wrap().gap(px(4.)).items_center();
        let expanded = self.sessions_expanded();
        let current = self.session_filter();
        for filter in Filter::EACH {
            filters = filters.child(
                div()
                    .id(("session-filter", filter as usize))
                    .debug_selector(move || format!("sessions-filter-{}", filter.label()))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(crate::config::corners::CONTROL))
                    .cursor_pointer()
                    .when(filter == current, |s| s.bg(rgb(theme.active)))
                    .text_color(rgb(if filter == current {
                        theme.foreground
                    } else {
                        theme.muted
                    }))
                    .child(filter.label())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_session_filter(filter, cx);
                        this.focus_session_search(window, cx);
                    })),
            );
        }
        filters = filters
            .child(
                div()
                    .id("sessions-new-project")
                    .debug_selector(|| "sessions-new-project".into())
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(crate::config::corners::CONTROL))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme.active)))
                    .text_color(rgb(theme.muted))
                    .child("New project…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        // The form owns the keyboard: its name field is focused
                        // by `open_project_create`, and taking focus back to the
                        // search here would swallow the typed name.
                        this.open_project_create(None, window, cx);
                    })),
            )
            .child(
                div()
                    .id("sessions-expand")
                    .debug_selector(|| "sessions-expand".into())
                    .ml_auto()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(crate::config::corners::CONTROL))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(theme.active)))
                    .text_color(rgb(theme.muted))
                    .child(if expanded { "Compact" } else { "Expand" })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_sessions_expanded(cx);
                        this.focus_session_search(window, cx);
                    })),
            );
        // The toolbar yields to the sessions below it: only the open facet's
        // scroller can shrink, so a short window squeezes the choices first.
        let mut bar = div()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(6.))
            .debug_selector(|| "sessions-toolbar".into());
        if let Some(view) = &self.menu.sessions_view {
            bar = bar.child(
                div()
                    .debug_selector(|| "sessions-search".into())
                    .child(view.search.clone()),
            );
        }
        let (_, pages) = self.session_page_entries();
        let page = self.session_page().min(pages - 1);
        let mut bar = bar.child(filters).children(self.render_session_facets(cx));
        if pages > 1 {
            let step = |label: &'static str, selector: &'static str, target: Option<usize>| {
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(crate::config::corners::CONTROL))
                    .when(target.is_some(), |s| {
                        s.cursor_pointer().hover(|s| s.bg(rgb(theme.active)))
                    })
                    .text_color(rgb(if target.is_some() {
                        theme.foreground
                    } else {
                        theme.muted
                    }))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(target) = target {
                            this.set_session_page(target, cx);
                            this.focus_session_search(window, cx);
                        }
                    }))
            };
            bar = bar.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(rgb(theme.muted))
                    .child(step("Previous", "sessions-page-prev", page.checked_sub(1)))
                    .child(
                        div()
                            .debug_selector(|| "sessions-page".into())
                            .child(format!("Page {} of {pages}", page + 1)),
                    )
                    .child(step(
                        "Next",
                        "sessions-page-next",
                        (page + 1 < pages).then_some(page + 1),
                    )),
            );
        }
        bar
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
