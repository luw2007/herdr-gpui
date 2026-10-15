//! The strip of logical Windows under the title bar, and the empty state a
//! Window shows until a session is created in it or an existing one added.
use crate::HerdrWindow;
use gpui::{prelude::*, *};

const STRIP_HEIGHT: f32 = 26.;

impl HerdrWindow {
    /// Shown whenever there is more than one logical Window, or the one there
    /// is lists no session; a lone populated Window needs no strip, and the
    /// New Window command still adds a second.
    pub(crate) fn container_strip_shown(&self) -> bool {
        self.containers.containers().len() > 1 || self.container_is_empty()
    }

    pub(crate) fn render_container_strip(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let font = &self.config.ui;
        let active = self.containers.active_index();
        let mut strip = div()
            .id("container-strip")
            .debug_selector(|| "container-strip".into())
            .flex_none()
            .h(px(STRIP_HEIGHT))
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .text_size(px(font.size * 0.9))
            .border_b_1()
            .border_color(rgb(theme.active));
        for (index, container) in self.containers.containers().iter().enumerate() {
            let selected = index == active;
            let id = container.id;
            let title = container.title.clone();
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("container-tab-{id}")))
                    .debug_selector(move || format!("container-tab-{id}"))
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
                    .child(title)
                    .child(
                        div()
                            .id(SharedString::from(format!("container-close-{id}")))
                            .debug_selector(move || format!("container-close-{id}"))
                            .child("\u{d7}")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if let Some(index) = this.containers.index_of(id) {
                                    let _ = this.close_container(index, cx);
                                }
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(index) = this.containers.index_of(id) {
                            this.switch_container(index, cx);
                        }
                        window.focus(&this.focus, cx);
                    })),
            );
        }
        strip.child(
            div()
                .id("container-new")
                .debug_selector(|| "container-new".into())
                .px(px(8.))
                .cursor_pointer()
                .text_color(rgb(theme.muted))
                .hover(|s| s.text_color(rgb(theme.foreground)))
                .child("+ New Window")
                .on_click(cx.listener(|this, _, window, cx| {
                    let _ = this.new_container(None, cx);
                    window.focus(&this.focus, cx);
                })),
        )
    }

    /// What an empty Window shows in place of a terminal.
    pub(crate) fn render_container_empty(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
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
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        button("container-create-session", "Create session").on_click(cx.listener(
                            |this, event: &ClickEvent, window, cx| {
                                this.open_sessions_creating(event.position(), window, cx);
                            },
                        )),
                    )
                    .child(
                        button("container-add-session", "Add existing session").on_click(
                            cx.listener(|this, event: &ClickEvent, window, cx| {
                                this.open_sessions(event.position(), window, cx);
                            }),
                        ),
                    ),
            )
    }
}
