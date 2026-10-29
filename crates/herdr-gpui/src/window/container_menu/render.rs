//! Painting of the Window tab menu: a full-window backdrop that closes it and
//! a panel snapped inside the viewport. The close step draws that same panel
//! with Cancel and Close Window, so Escape and the backdrop keep the Window.
use super::{ContainerMenu, MARGIN, Mode, ROWS, Row, WIDTH};
use crate::{HerdrWindow, config::corners, containers::Container, fonts::StyledFont};
use gpui::{Size, prelude::*, *};

impl HerdrWindow {
    /// The menu overlay, or nothing when it is closed or its Window is gone.
    pub(crate) fn render_container_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.container_menu.as_ref()?;
        let Some(container) = self.containers.set().get(menu.id) else {
            // Its Window was closed, here or elsewhere: forget the menu once
            // this frame is done, returning the keyboard to the terminal.
            cx.defer_in(window, |this, window, cx| {
                let stale = this
                    .container_menu
                    .as_ref()
                    .is_some_and(|menu| this.containers.set().get(menu.id).is_none());
                if stale {
                    this.dismiss_container_menu(window, cx);
                }
            });
            return None;
        };

        let viewport = window.viewport_size();
        let panel = match menu.mode {
            Mode::Actions => self.container_menu_actions(menu, container, viewport, cx),
            Mode::Rename => self.container_menu_rename(menu, viewport, cx),
            Mode::ConfirmClose => self.container_menu_confirm_close(menu, container, viewport, cx),
        };
        let overlay = div()
            .id("container-menu-overlay")
            .debug_selector(|| "container-menu-overlay".into())
            .absolute()
            .inset_0()
            .occlude()
            .track_focus(&menu.focus)
            .on_key_down(cx.listener(Self::container_menu_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.dismiss_container_menu(window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.dismiss_container_menu(window, cx);
                }),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                anchored()
                    .position(menu.anchor)
                    .snap_to_window_with_margin(Edges::all(px(MARGIN)))
                    .child(panel),
            );
        Some(overlay.into_any_element())
    }

    fn container_menu_panel(
        &self,
        selector: &'static str,
        viewport: Size<Pixels>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let font = &self.config.ui;
        div()
            .id(selector)
            .debug_selector(move || selector.into())
            .w(px(WIDTH * font.size / 12.))
            .max_w((viewport.width - px(2. * MARGIN)).max(px(0.)))
            // The menu can never outgrow the window: it snaps to its margin
            // first, and its body scrolls inside this cap under a large UI
            // font. `snap_to_window` keeps the panel's box inside the margin,
            // so the cap is the full viewport minus one margin each side.
            .max_h((viewport.height - px(2. * MARGIN)).max(px(0.)))
            .flex()
            .flex_col()
            .overflow_hidden()
            .p(px(5.))
            .bg(rgb(theme.surface))
            .text_color(rgb(theme.foreground))
            .text_font(font)
            .text_size(px(font.size))
            .line_height(px(font.line_height()))
            .border_1()
            .border_color(rgb(theme.active))
            .rounded(px(corners::PANEL))
            .shadow_lg()
            .occlude()
            // The panel is not the backdrop: clicks inside it stay inside.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
    }

    fn container_menu_actions(
        &self,
        menu: &ContainerMenu,
        container: &Container,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let create_reason = self.local_create_unavailable_reason();
        let panel = self.container_menu_panel("container-menu-panel", viewport);
        let mut rows = div()
            .id("container-menu-rows")
            .debug_selector(|| "container-menu-rows".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&menu.scroll)
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col();
        for (index, row) in ROWS.iter().copied().enumerate() {
            let refusal = row.refusal(container, create_reason);
            let checked = row == Row::Group(container.grouping);
            // The "Group by" heading lives inside the first group row, not as
            // its own child: the scroll handle's child index must match the
            // row index so keyboard scroll_to_item lands on the right line.
            let heading = (row == Row::Group(crate::containers::Grouping::None)).then(|| {
                div()
                    .px(px(8.))
                    .pt(px(6.))
                    .text_color(rgb(theme.muted))
                    .child("Group by")
            });
            let selector = row.selector();
            let (foreground, surface) = (theme.foreground, theme.surface);
            let row_el = div()
                .id(SharedString::from(selector))
                .debug_selector(move || row.selector())
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(corners::SMALL))
                .text_color(rgb(if refusal.is_none() {
                    theme.foreground
                } else {
                    theme.muted
                }))
                .when(menu.highlight == index, |line| line.bg(rgb(theme.active)))
                .when(refusal.is_none(), |line| {
                    line.cursor_pointer()
                        .hover(|s| s.bg(rgb(theme.active)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.container_menu_activate(row, window, cx);
                        }))
                })
                .child(div().min_w_0().truncate().child(row.label()))
                .when(checked, |line| {
                    line.child(
                        div()
                            .flex_none()
                            .debug_selector(move || format!("{}-check", row.selector()))
                            .child("\u{2713}"),
                    )
                })
                .when_some(refusal, |line, reason| {
                    line.tooltip(move |_, cx| {
                        cx.new(|_| crate::usage::Hint {
                            text: reason.into(),
                            foreground,
                            surface,
                        })
                        .into()
                    })
                });
            rows = rows.child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .when_some(heading, |cell, heading| cell.child(heading))
                    .child(row_el),
            );
        }
        panel
            .child(rows)
            .when_some(menu.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("container-menu-error")
                        .debug_selector(|| "container-menu-error".into())
                        .flex_none()
                        .px(px(8.))
                        .pt(px(4.))
                        .text_color(rgb(theme.palette[1]))
                        .child(error),
                )
            })
    }

    fn container_menu_rename(
        &self,
        menu: &ContainerMenu,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let font = &self.config.ui;
        menu.input.update(cx, |input, cx| {
            input.set_appearance(font.clone(), theme.clone(), cx);
        });
        let button = |selector: &'static str, label: &'static str, primary: bool| {
            div()
                .id(selector)
                .debug_selector(move || selector.into())
                .px(px(10.))
                .py(px(3.))
                .rounded(px(corners::SMALL))
                .cursor_pointer()
                .bg(rgb(if primary { theme.active } else { theme.surface }))
                .border_1()
                .border_color(rgb(theme.active))
                .child(label)
        };
        self.container_menu_panel("container-menu-rename-panel", viewport)
            .gap(px(6.))
            .child(
                div()
                    .px(px(3.))
                    .text_color(rgb(theme.muted))
                    .child("Rename Window"),
            )
            .child(menu.input.clone())
            .when_some(menu.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("container-menu-error")
                        .debug_selector(|| "container-menu-error".into())
                        .px(px(3.))
                        .text_color(rgb(theme.palette[1]))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        button("container-menu-rename-cancel", "Cancel", false).on_click(
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.dismiss_container_menu(window, cx);
                            }),
                        ),
                    )
                    .child(
                        button("container-menu-rename-submit", "Rename", true).on_click(
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.submit_container_rename(window, cx);
                            }),
                        ),
                    ),
            )
    }

    /// The second step of closing a Window: Cancel keeps it, Close Window
    /// closes only it, and Escape or the backdrop keep it too.
    fn container_menu_confirm_close(
        &self,
        menu: &ContainerMenu,
        container: &Container,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let button = |selector: &'static str, label: &'static str, primary: bool| {
            div()
                .id(selector)
                .debug_selector(move || selector.into())
                .px(px(10.))
                .py(px(3.))
                .rounded(px(corners::SMALL))
                .cursor_pointer()
                .bg(rgb(if primary { theme.active } else { theme.surface }))
                .border_1()
                .border_color(rgb(theme.active))
                .child(label)
        };
        self.container_menu_panel("container-menu-close-panel", viewport)
            .gap(px(6.))
            .child(
                div()
                    .px(px(3.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Close Window?"),
            )
            .child(
                div()
                    .px(px(3.))
                    .min_w_0()
                    .truncate()
                    .child(container.title.clone()),
            )
            .child(
                div()
                    .px(px(3.))
                    .text_color(rgb(theme.muted))
                    .child("Only this Window goes away; Herdr and its sessions keep running."),
            )
            .when_some(menu.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("container-menu-error")
                        .debug_selector(|| "container-menu-error".into())
                        .px(px(3.))
                        .text_color(rgb(theme.palette[1]))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        button("container-menu-close-cancel", "Cancel", false).on_click(
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.dismiss_container_menu(window, cx);
                            }),
                        ),
                    )
                    .child(
                        button("container-menu-close-confirm", "Close Window", true).on_click(
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.submit_container_close(window, cx);
                            }),
                        ),
                    ),
            )
    }
}
