//! Opening the native manager is independent of project and session inventory.
use super::*;

pub(super) fn open(caller: &Caller, cx: &mut App) -> Response {
    in_free_window(caller, cx, |view, window, cx| {
        if !view.open_project_manager(window, cx) {
            return Response::error(
                ErrorCode::Failed,
                "The Projects manager did not open; close other menus and try again",
            );
        }
        bring_forward(window, cx);
        Response::done(
            "Opened the Projects manager; no project records or server folders were changed",
        )
    })
}

#[cfg(test)]
mod tests;
