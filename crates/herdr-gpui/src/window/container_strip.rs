//! The strip of logical Windows under the title bar, and the empty state a
//! Window shows until a session is created in it or an existing one added.
use crate::{HerdrWindow, containers::ContainerId};
use gpui::{prelude::*, *};

const STRIP_HEIGHT: f32 = 26.;
/// A tab never grows past this; a longer title is cut with an ellipsis and
/// shown in full by the tab's tooltip, so the menu and close buttons stay in
/// reach whatever the title holds.
const TAB_MAX_WIDTH: f32 = 220.;
/// Title of the toast a refused New Window raises; a later success removes it.
const NEW_WINDOW_FAILED: &str = "New Window failed";
/// What the strip remembers between frames: its scroll position, and the
/// Window last brought into view, so a scroll by hand is not undone each frame.
struct StripScroll {
    handle: ScrollHandle,
    revealed: Option<ContainerId>,
}

impl HerdrWindow {
    /// Shown in every non-Zen window. Besides the tabs it carries the fixed
    /// Add-session entry, so a lone populated Window keeps a visible way to add
    /// to itself. Keep its room during the entering chrome slide, just like
    /// the header; only the settled Zen layout hides it.
    pub(crate) fn container_strip_shown(&self) -> bool {
        !self.zen_layout()
    }

    pub(crate) fn render_container_strip(
        &self,
        in_titlebar: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let font = &self.config.ui;
        let active = self.containers.active_index();
        let scroll = window.use_keyed_state("container-strip-scroll", cx, |_, _| StripScroll {
            handle: ScrollHandle::new(),
            revealed: None,
        });
        let active_id = self.containers.containers().get(active).map(|c| c.id);
        let handle = scroll.update(cx, |state, _| {
            // The handle learns its viewport during the first paint; a request
            // made before then is dropped, so wait for one and draw again.
            if state.revealed != active_id {
                if state.handle.bounds().size.width > px(0.) {
                    state.revealed = active_id;
                    state.handle.scroll_to_item(active);
                }
                window.refresh();
            }
            state.handle.clone()
        });
        let strip = div()
            .id("container-strip")
            .debug_selector(|| "container-strip".into())
            .flex_none()
            .h(px(crate::titlebar::strip_height(STRIP_HEIGHT, in_titlebar)))
            .flex()
            .items_center()
            .px(px(8.))
            .when(in_titlebar, |strip| strip.pl(px(crate::titlebar::LEADING)))
            .text_size(px(font.size * 0.9))
            .border_b_1()
            .border_color(rgb(theme.active));
        let mut tabs = div()
            .id("container-tabs")
            .debug_selector(|| "container-tabs".into())
            // The tabs take the room the two fixed entries leave and scroll
            // sideways inside it, so Add and New stay reachable in a 640-wide
            // window whatever the titles or the UI font size.
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .overflow_x_scroll()
            .track_scroll(&handle);
        for (index, container) in self.containers.containers().iter().enumerate() {
            let selected = index == active;
            let id = container.id;
            let title = container.title.clone();
            let tip = SharedString::from(container.title.clone());
            let (tip_foreground, tip_surface) = (theme.foreground, theme.surface);
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("container-tab-{id}")))
                    .debug_selector(move || format!("container-tab-{id}"))
                    .flex_none()
                    .max_w(px(TAB_MAX_WIDTH))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .h_full()
                    .cursor_pointer()
                    .text_color(rgb(if selected {
                        theme.foreground
                    } else {
                        theme.muted
                    }))
                    .when(selected, |tab| tab.bg(rgb(theme.active)))
                    .child(div().min_w_0().truncate().child(title))
                    .tooltip(move |_, cx| {
                        cx.new(|_| crate::usage::Hint {
                            text: tip.clone(),
                            foreground: tip_foreground,
                            surface: tip_surface,
                        })
                        .into()
                    })
                    .when(selected, |tab| {
                        tab.child(
                            div()
                                .id(SharedString::from(format!("container-menu-{id}")))
                                .debug_selector(move || format!("container-menu-{id}"))
                                .flex_none()
                                .px(px(3.))
                                .rounded(px(crate::config::corners::SMALL))
                                .hover(|s| s.bg(rgb(theme.surface)))
                                .child("\u{2026}")
                                .on_click(cx.listener(
                                    move |this, event: &ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        let at = event.position();
                                        this.open_container_menu(id, at, window, cx);
                                    },
                                )),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("container-close-{id}")))
                            .debug_selector(move || format!("container-close-{id}"))
                            .flex_none()
                            .child("\u{d7}")
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                // A Window that lists sessions is not closed by a
                                // stray click: the menu says why instead.
                                if this.close_container(id, cx).is_err() {
                                    this.open_container_menu(id, event.position(), window, cx);
                                }
                            })),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_container_menu(id, event.position, window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_container(id, cx);
                        window.focus(&this.focus, cx);
                    })),
            );
        }
        strip
            .child(tabs)
            .child(
                div()
                    .id("container-strip-add-session")
                    .debug_selector(|| "container-strip-add-session".into())
                    .flex_none()
                    .px(px(8.))
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .text_color(rgb(theme.muted))
                    .hover(|s| s.text_color(rgb(theme.foreground)))
                    .child("+ Add session")
                    .tooltip({
                        let (foreground, surface) = (theme.foreground, theme.surface);
                        let keymap = self.keymap().clone();
                        move |_, cx| {
                            let key = keymap.primary(crate::controls::Command::Sessions);
                            let text = if key.is_empty() {
                                "Add a session to this Window".to_owned()
                            } else {
                                format!("Add a session to this Window ({key})")
                            };
                            cx.new(|_| crate::usage::Hint {
                                text: text.into(),
                                foreground,
                                surface,
                            })
                            .into()
                        }
                    })
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        this.open_sessions(event.position(), window, cx);
                    })),
            )
            .child(
                div()
                    .id("container-new")
                    .debug_selector(|| "container-new".into())
                    .flex_none()
                    .px(px(8.))
                    .cursor_pointer()
                    .text_color(rgb(theme.muted))
                    .hover(|s| s.text_color(rgb(theme.foreground)))
                    .child("+ New Window")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.new_container_reporting(cx);
                        window.focus(&this.focus, cx);
                    })),
            )
    }

    /// Adds an empty Window for a menu, palette or strip action. A refusal
    /// (the registry is full) is a toast, which shows whatever the connection
    /// or the status bar is doing and leaves the connection's own error alone;
    /// a success removes the failure an earlier attempt left.
    pub(crate) fn new_container_reporting(&mut self, cx: &mut Context<Self>) {
        match self.new_container(None, cx) {
            Ok(_) => {
                for endpoint in &mut self.endpoints {
                    endpoint.toasts.entries.retain(|(_, notice)| {
                        !(notice.is_local_feedback() && notice.title == NEW_WINDOW_FAILED)
                    });
                }
                cx.notify();
            }
            Err(error) => {
                self.endpoints[self.selected_endpoint]
                    .toasts
                    .entries
                    .retain(|(_, notice)| {
                        !(notice.is_local_feedback() && notice.title == NEW_WINDOW_FAILED)
                    });
                self.local_transfer_notice(NEW_WINDOW_FAILED, error.to_string(), cx);
            }
        }
    }
    /// What an empty Window shows in place of a terminal.
    pub(crate) fn render_container_empty(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let create_reason = self.local_create_unavailable_reason();
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .px(px(14.))
                .py(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(theme.active))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme.active)))
                .child(label)
        };
        let create = match create_reason {
            Some(reason) => div()
                .id("container-create-session")
                .debug_selector(|| "container-create-session".to_owned())
                .px(px(14.))
                .py(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(theme.active))
                .text_color(rgb(theme.muted))
                .tooltip({
                    let (foreground, surface) = (theme.foreground, theme.surface);
                    move |_, cx| {
                        cx.new(|_| crate::usage::Hint {
                            text: reason.into(),
                            foreground,
                            surface,
                        })
                        .into()
                    }
                })
                .child("Create session"),
            None => button("container-create-session", "Create session").on_click(cx.listener(
                |this, event: &ClickEvent, window, cx| {
                    this.open_sessions_creating(event.position(), window, cx);
                },
            )),
        };
        div()
            .id("container-empty")
            .debug_selector(|| "container-empty".into())
            .flex_1()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .text_color(rgb(theme.foreground))
            .child(
                div()
                    .text_color(rgb(theme.muted))
                    .child("This Window has no sessions"),
            )
            .child(div().flex().gap(px(10.)).child(create).child(
                button("container-add-session", "Add existing session").on_click(cx.listener(
                    |this, event: &ClickEvent, window, cx| {
                        this.open_sessions(event.position(), window, cx);
                    },
                )),
            ))
    }
}
