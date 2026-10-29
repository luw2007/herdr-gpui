use super::columns;
use crate::{HerdrWindow, config::corners, fonts::StyledFont};
use gpui::{prelude::*, *};

impl HerdrWindow {
    pub(crate) fn render_space_overview(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let overview = self.space_overview.as_ref()?;
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 32.).clamp(0., 1080.);
        let cols = columns(width);
        // A fresh ScrollHandle drops scroll_to_item before its first layout.
        // Wait for measured rows, then reveal the current Space once; later
        // wheel scrolling must remain under the user's control.
        if !overview.revealed.get() {
            if overview.scroll.bounds().size.height > px(0.) {
                overview.revealed.set(true);
                overview
                    .scroll
                    .scroll_to_item(self.containers.active_index() / cols);
            }
            cx.defer_in(window, |_, _, cx| cx.notify());
        }
        let theme = &self.theme;
        let mut rows = div()
            .id("space-overview-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&overview.scroll)
            .flex()
            .flex_col()
            .gap(px(10.))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation());
        for spaces in self.containers.containers().chunks(cols) {
            let mut row = div().flex_none().flex().gap(px(10.));
            for space in spaces {
                let id = space.id;
                let current = id == self.containers.shown_id();
                let selected = overview.selected == Some(id);
                let selector = format!("space-overview-card-{id}");
                let mut members = div()
                    .id(SharedString::from(format!("space-overview-members-{id}")))
                    .flex_none()
                    .max_h(px(140.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation());
                for member in &space.members {
                    let endpoint = self
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.session_ref() == *member);
                    let (label, status) = endpoint.map_or_else(
                        || {
                            (
                                format!("{} · {}", member.device, member.session),
                                "unavailable",
                            )
                        },
                        |endpoint| {
                            (
                                format!("{} · {}", endpoint.label, member.session),
                                endpoint.status(),
                            )
                        },
                    );
                    members = members.child(
                        div()
                            .flex_none()
                            .min_w_0()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .child(
                                div()
                                    .max_w(px(110.))
                                    .truncate()
                                    .text_color(rgb(theme.muted))
                                    .child(status),
                            ),
                    );
                }
                if space.members.is_empty() {
                    members =
                        members.child(div().text_color(rgb(theme.muted)).child("No sessions"));
                }
                row = row.child(
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .w(px(
                            (width - 32. - (cols - 1) as f32 * 10.).max(0.) / cols as f32
                        ))
                        .min_w_0()
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .border_1()
                        .border_color(rgb(if selected { theme.active } else { theme.muted }))
                        .rounded(px(corners::PANEL))
                        .bg(rgb(theme.background))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.activate_overview_space(id, window, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(space.title.clone()),
                                )
                                .when(current, |title| {
                                    title.child(
                                        div()
                                            .flex_none()
                                            .text_color(rgb(theme.muted))
                                            .child("Current"),
                                    )
                                }),
                        )
                        .child(members),
                );
            }
            rows = rows.child(row);
        }
        let panel = div()
            .id("space-overview-panel")
            .debug_selector(|| "space-overview-panel".into())
            .w(px(width))
            .h((viewport.height - px(32.)).max(px(0.)).min(px(640.)))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .overflow_hidden()
            .bg(rgb(theme.surface))
            .text_color(rgb(theme.foreground))
            .text_font(&self.config.ui)
            .text_size(px(self.config.ui.size))
            .line_height(px(self.config.ui.line_height()))
            .border_1()
            .border_color(rgb(theme.active))
            .rounded(px(corners::PANEL))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().child("Space Overview"))
                    .child(
                        div()
                            .id("space-overview-new")
                            .debug_selector(|| "space-overview-new".into())
                            .cursor_pointer()
                            .child("+ New Space")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_space_overview(window, cx);
                                this.new_container_reporting(cx);
                            })),
                    ),
            )
            .child(rows)
            .child(
                div()
                    .text_color(rgb(theme.muted))
                    .child("Arrow keys to select · Enter to switch · Escape to dismiss"),
            );
        Some(
            div()
                .id("space-overview-overlay")
                .debug_selector(|| "space-overview-overlay".into())
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .track_focus(&overview.focus)
                .on_key_down(cx.listener(Self::overview_key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.dismiss_space_overview(window, cx);
                    }),
                )
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(panel)
                .into_any_element(),
        )
    }
}
