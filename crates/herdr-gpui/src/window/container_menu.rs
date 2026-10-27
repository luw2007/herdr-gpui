//! The menu of a Window tab: adding or creating a session in it, rename,
//! close (only an empty Window), and how its sessions are to be grouped. Every
//! write goes through the same methods the `container` CLI uses, so the rules
//! live in one place.
use crate::{
    HerdrWindow,
    containers::{Container, ContainerError, ContainerId, Grouping},
    search_input::SearchInput,
};
use gpui::{prelude::*, *};

mod render;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Actions,
    Rename,
}

pub(super) const CLOSE_REFUSED: &str = "Detach its sessions first";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    CreateSession,
    AddSession,
    Rename,
    Close,
    Group(Grouping),
}

/// The rows in display order.
const ROWS: &[Row] = &[
    Row::CreateSession,
    Row::AddSession,
    Row::Rename,
    Row::Close,
    Row::Group(Grouping::None),
    Row::Group(Grouping::Project),
    Row::Group(Grouping::Status),
    Row::Group(Grouping::Mr),
];

const WIDTH: f32 = 240.;
const MARGIN: f32 = 8.;

impl Row {
    fn selector(self) -> String {
        match self {
            Self::CreateSession => "container-menu-create-session".into(),
            Self::AddSession => "container-menu-add-session".into(),
            Self::Rename => "container-menu-rename".into(),
            Self::Close => "container-menu-close".into(),
            Self::Group(grouping) => format!("container-menu-group-{}", grouping.as_str()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CreateSession => "Create session",
            Self::AddSession => "Add existing session",
            Self::Rename => "Rename Window",
            Self::Close => "Close Window",
            Self::Group(Grouping::None) => "None",
            Self::Group(Grouping::Project) => "Project",
            Self::Group(Grouping::Status) => "Status",
            Self::Group(Grouping::Mr) => "Merge request",
        }
    }

    /// Why the row cannot act on `container` right now, if it cannot. Add
    /// existing works everywhere; Create borrows the one shared rule.
    fn refusal(
        self,
        container: &Container,
        create_reason: Option<&'static str>,
    ) -> Option<&'static str> {
        match self {
            Self::CreateSession => create_reason,
            Self::Close if !container.members.is_empty() => Some(CLOSE_REFUSED),
            _ => None,
        }
    }
}

/// The open menu of one Window. It holds the Window's identity only, so a
/// Window closed elsewhere leaves nothing to draw.
pub(crate) struct ContainerMenu {
    id: ContainerId,
    anchor: Point<Pixels>,
    mode: Mode,
    highlight: usize,
    /// Scrolls the action list when it grows past the viewport (large UI font),
    /// so the keyboard-highlighted row is always visible.
    scroll: ScrollHandle,
    error: Option<String>,
    focus: FocusHandle,
    input: Entity<SearchInput>,
}

impl HerdrWindow {
    /// Opens the menu of Window `id` at `anchor`, taking the keyboard.
    pub(crate) fn open_container_menu(
        &mut self,
        id: ContainerId,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.containers.set().get(id).is_none() {
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.container_menu = Some(ContainerMenu {
            id,
            anchor,
            mode: Mode::Actions,
            highlight: 0,
            scroll: ScrollHandle::new(),
            error: None,
            focus,
            input: cx.new(SearchInput::new),
        });
        cx.notify();
    }

    fn dismiss_container_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.container_menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn container_menu_failed(&mut self, error: &ContainerError, cx: &mut Context<Self>) {
        if let Some(menu) = self.container_menu.as_mut() {
            menu.error = Some(error.to_string());
            cx.notify();
        }
    }

    fn container_menu_activate(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, anchor)) = self
            .container_menu
            .as_ref()
            .map(|menu| (menu.id, menu.anchor))
        else {
            return;
        };
        let create_reason = self.local_create_unavailable_reason();
        let Some(container) = self.containers.set().get(id) else {
            self.dismiss_container_menu(window, cx);
            return;
        };
        if let Some(reason) = row.refusal(container, create_reason) {
            if let Some(menu) = self.container_menu.as_mut() {
                menu.error = Some(reason.to_owned());
                cx.notify();
            }
            return;
        }
        match row {
            Row::CreateSession | Row::AddSession => {
                // Hand over: close the tab menu before the picker opens, so
                // only one overlay owns the screen and the keyboard.
                self.container_menu = None;
                self.switch_container(id, cx);
                if row == Row::CreateSession {
                    self.open_sessions_creating(anchor, window, cx);
                } else {
                    self.open_sessions(anchor, window, cx);
                }
            }
            Row::Rename => self.begin_container_rename(id, window, cx),
            Row::Close => match self.close_container(id, cx) {
                Ok(()) => self.dismiss_container_menu(window, cx),
                Err(error) => self.container_menu_failed(&error, cx),
            },
            Row::Group(grouping) => {
                match self.edit_containers(cx, |set| set.set_grouping(id, grouping)) {
                    Ok(()) => {
                        cx.notify();
                        self.dismiss_container_menu(window, cx);
                    }
                    Err(error) => self.container_menu_failed(&error, cx),
                }
            }
        }
    }

    fn begin_container_rename(
        &mut self,
        id: ContainerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self
            .containers
            .set()
            .get(id)
            .map(|container| container.title.clone())
            .unwrap_or_default();
        let Some(menu) = self.container_menu.as_mut() else {
            return;
        };
        menu.mode = Mode::Rename;
        menu.error = None;
        let input = menu.input.clone();
        input.update(cx, |input, cx| input.set_text_selected(&title, cx));
        let focus = input.read(cx).focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn submit_container_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, title)) = self
            .container_menu
            .as_ref()
            .map(|menu| (menu.id, menu.input.read(cx).text().to_owned()))
        else {
            return;
        };
        match self.rename_container(id, &title, cx) {
            Ok(()) => self.dismiss_container_menu(window, cx),
            Err(error) => self.container_menu_failed(&error, cx),
        }
    }

    fn container_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.container_menu else {
            return;
        };
        let renaming = menu.mode == Mode::Rename;
        // The OS owns Enter and Escape while a composition is open.
        if renaming && menu.input.read(cx).is_composing() {
            return;
        }
        let highlight = menu.highlight;
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        match key {
            "escape" => self.dismiss_container_menu(window, cx),
            "enter" if renaming => self.submit_container_rename(window, cx),
            "enter" => {
                if let Some(row) = ROWS.get(highlight) {
                    self.container_menu_activate(*row, window, cx);
                }
            }
            "up" | "down" if !renaming => {
                let count = ROWS.len();
                if let Some(menu) = self.container_menu.as_mut() {
                    menu.highlight = if key == "down" {
                        (highlight + 1) % count
                    } else {
                        (highlight + count - 1) % count
                    };
                    // The list may scroll under a large UI font; keep the row
                    // the user is about to activate in view.
                    menu.scroll.scroll_to_item(menu.highlight);
                }
                cx.notify();
            }
            // Typed text is the field's: it reaches the input method on its
            // way, and nothing else may reach the terminal behind the menu.
            _ if renaming
                && event.keystroke.key_char.is_some()
                && !modifiers.control
                && !modifiers.platform =>
            {
                return;
            }
            _ => {}
        }
        cx.stop_propagation();
        window.prevent_default();
    }
}
