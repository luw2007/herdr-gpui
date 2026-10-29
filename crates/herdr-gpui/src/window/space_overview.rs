//! A read-only overview of saved Spaces. Selection is an identity, never an
//! index into the registry: concurrent reordering cannot change Enter's target.
use crate::{HerdrWindow, containers::ContainerId};
use gpui::{prelude::*, *};

mod render;
#[cfg(test)]
mod tests;

pub(crate) struct SpaceOverview {
    selected: Option<ContainerId>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    revealed: std::cell::Cell<bool>,
}

fn columns(width: f32) -> usize {
    if width >= 900. {
        3
    } else if width >= 600. {
        2
    } else {
        1
    }
}

impl HerdrWindow {
    pub(crate) fn toggle_space_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.space_overview.is_some() {
            self.dismiss_space_overview(window, cx);
        } else {
            self.show_space_overview(window, cx);
        }
    }

    pub(crate) fn show_space_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // In particular, never hide a pending destructive confirmation.
        if self.space_overview.is_some()
            || self.menu.page.is_some()
            || self.container_menu.is_some()
            || crate::native_close::is_prompting(window, cx)
        {
            return;
        }
        self.shift_taps.cancel();
        self.discard_composition(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let scroll = ScrollHandle::new();
        self.space_overview = Some(SpaceOverview {
            selected: Some(self.containers.shown_id()),
            focus,
            scroll,
            revealed: std::cell::Cell::new(false),
        });
        cx.notify();
    }

    fn dismiss_space_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.space_overview.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn activate_overview_space(
        &mut self,
        id: ContainerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A stale click/Enter must not switch to the item now at an old index.
        let exists = self.containers.set().get(id).is_some();
        self.dismiss_space_overview(window, cx);
        if exists {
            self.switch_container(id, cx);
        }
    }

    pub(super) fn overview_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(overview) = &self.space_overview else {
            return;
        };
        let selected = overview.selected;
        match event.keystroke.key.as_str() {
            "escape" => self.dismiss_space_overview(window, cx),
            "enter" => {
                if let Some(id) = selected {
                    self.activate_overview_space(id, window, cx);
                }
            }
            key @ ("up" | "down" | "left" | "right") => {
                let spaces = self.containers.containers();
                if !spaces.is_empty() {
                    let count = spaces.len();
                    let current =
                        selected.and_then(|id| spaces.iter().position(|space| space.id == id));
                    let cols = columns(f32::from(window.viewport_size().width) - 32.);
                    let index = match (current, key) {
                        (None, _) => 0,
                        (Some(index), "left") => index.saturating_sub(1),
                        (Some(index), "right") => (index + 1).min(count - 1),
                        (Some(index), "up") => index.saturating_sub(cols),
                        (Some(index), _) => (index + cols).min(count - 1),
                    };
                    let id = spaces[index].id;
                    if let Some(overview) = self.space_overview.as_mut() {
                        overview.selected = Some(id);
                        overview.scroll.scroll_to_item(index / cols);
                    }
                    cx.notify();
                }
            }
            _ => {}
        }
        cx.stop_propagation();
        window.prevent_default();
    }
}
