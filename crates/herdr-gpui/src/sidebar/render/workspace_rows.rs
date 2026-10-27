//! Workspace and host rows, including their reorder preview bookkeeping.

use super::*;

pub(super) struct WorkspaceRows {
    pub(super) spaces: Stateful<Div>,
    pub(super) highlighted: [Option<usize>; 2],
    pub(super) host_rows: Vec<(usize, usize)>,
    pub(super) drop_rows: Vec<(usize, usize, Pixels)>,
    pub(super) drop_requests: Vec<Option<reorder::MoveBlock>>,
    pub(super) drop_dragged: usize,
    pub(super) sliding: bool,
    pub(super) filtered: bool,
}

/// One row group of the spaces list: a group header, or a host with its rows.
enum Step<'a> {
    Group(&'a super::super::grouping::Section),
    Host(usize),
}

impl HerdrWindow {
    pub(super) fn append_workspace_rows(
        &self,
        mut spaces: Stateful<Div>,
        indicators: Indicators,
        look: SidebarLook,
        width: f32,
        cx: &mut Context<Self>,
    ) -> WorkspaceRows {
        let rows = layout_for(self.config.layout.mode);
        // The row a workspace menu was opened for keeps looking hovered while
        // the pointer is over the menu.
        let menu_target = self.workspace_menu_target();
        let layout = look.density;
        let content_x = look.content_x();
        let font = &self.config.sidebar;
        let spaces_custom = self.config.usage.inline
            && self.config.sidebar_layout.spaces != crate::config::SpaceLayout::default();
        let theme = &self.theme;
        let devices = !self.config.layout.mode.lists_workspaces();
        let multi = self.sidebar_multi();
        let headed = multi || devices;
        let tree_search = self.config.show_sidebar_search && devices;
        let query = if tree_search {
            self.device_tree_query(cx)
        } else {
            String::new()
        };
        // A plugin view hid every agent, rather than there being none.
        let mut filtered = false;
        // Child positions of the highlighted rows, for the one-time reveal below.
        // Agent rows are counted by `agent_count`, which indexes that list.
        let mut space_rows = 0usize;
        let mut highlighted = [None; 2];
        // Each host header's endpoint index and child position in the spaces
        // list, for the header pinned over it.
        let mut host_rows = Vec::new();
        // A lifted workspace row: each drop unit's rows in the spaces list, and
        // the move every gap makes, for the pointer handler below.
        let mut drop_rows = Vec::new();
        let mut drop_requests = Vec::new();
        let mut drop_dragged = 0;
        let now = std::time::Instant::now();
        let mut sliding = false;
        // Under a grouping the hosts come in group order, each group under a
        // header, and a folded group lists only its header.
        let sections = self.sidebar_sections(|index| !devices || self.device_listed(index, &query));
        let steps: Vec<Step> = match &sections {
            Some(sections) => sections
                .iter()
                .flat_map(|section| {
                    std::iter::once(Step::Group(section)).chain(
                        section
                            .hosts
                            .iter()
                            .filter(|_| !section.folded)
                            .map(|&index| Step::Host(index)),
                    )
                })
                .collect(),
            None => (0..self.endpoints.len()).map(Step::Host).collect(),
        };
        for step in steps {
            let endpoint_index = match step {
                Step::Group(section) => {
                    spaces = spaces.child(self.group_header(section, look, width, cx));
                    space_rows += 1;
                    continue;
                }
                Step::Host(index) => index,
            };
            let endpoint = &self.endpoints[endpoint_index];
            if !self.endpoint_listed(endpoint_index)
                || (devices && !self.device_listed(endpoint_index, &query))
            {
                continue;
            }
            let selected = endpoint_index == self.selected_endpoint;
            let endpoint_id = endpoint.id.clone();
            let mark = wash::HostMark::resolve(
                &self.config.sidebar_style,
                &endpoint.label,
                selected,
                theme,
            );
            if headed {
                host_rows.push((endpoint_index, space_rows));
                spaces = spaces.child(self.host_row(endpoint, selected, look, width, false, cx));
                space_rows += 1;
            }
            if devices {
                let (list, rows, focused) =
                    self.append_device_agents(spaces, endpoint_index, indicators, look, width, cx);
                spaces = list;
                if let Some(row) = focused {
                    highlighted[0] = Some(space_rows + row);
                }
                space_rows += rows;
                continue;
            }
            let row_cx = RowContext {
                indicators,
                font,
                theme,
                look,
                width,
                nest: if multi { look.nest_indent() } else { 0. },
                mark,
                host: (multi && endpoint_id != crate::endpoint::LOCAL)
                    .then_some(endpoint.label.as_str()),
            };
            let daemon = crate::listening_ports::Daemon::of(&endpoint.connection.target);
            let live = if selected { &self.live } else { &endpoint.live };
            let Some(snapshot) = &live.snapshot else {
                continue;
            };
            let collapsed_repos = if endpoint_index == 0 {
                &self.collapsed_repos
            } else {
                &endpoint.collapsed_repos
            };
            let entries = visible_workspace_entries(&snapshot.workspaces, collapsed_repos);
            let drag = self
                .workspace_drag
                .as_ref()
                .filter(|drag| selected && drag.previewing(&snapshot.workspaces));
            let floating = drag.is_some_and(|drag| drag.floating());
            let plan = drag.and_then(|drag| Plan::new(&snapshot.workspaces, &drag.workspace));
            // Each unit's shift for the drop in preview, from the heights the
            // last frame laid out: shifting moves rows, never resizes them.
            let base = space_rows;
            let shifts = plan.as_ref().map_or_else(Vec::new, |plan| {
                drop_requests = (0..=plan.len())
                    .map(|slot| plan.request(&snapshot.workspaces, slot))
                    .collect();
                drop_dragged = plan.dragged();
                let mut heights = vec![0.; plan.len()];
                for (position, entry) in entries.iter().enumerate() {
                    if let (Some(unit), Some(bounds)) = (
                        plan.unit_of(entry.0),
                        self.sidebar_scroll[0].bounds_for_item(base + position),
                    ) {
                        heights[unit] += f32::from(bounds.size.height);
                        if spaces_custom && !entry.1 {
                            heights[unit] += f32::from(self.config.sidebar_layout.spaces.row_gap)
                                * line_height(font);
                        }
                    }
                }
                let slot = drag
                    .and_then(|drag| drag.target.as_ref())
                    .map_or(plan.dragged(), |target| target.slot);
                crate::reorder::preview(&heights, plan.dragged(), slot)
            });
            // A child closes the group when no child follows it.
            let closes: Vec<bool> = (0..entries.len())
                .map(|position| {
                    entries[position].1 && !entries.get(position + 1).is_some_and(|next| next.1)
                })
                .collect();
            // Gap separates one repository from the next, not a parent from
            // the linked worktrees under it. Reset per host.
            let mut previous_indented = None;
            for (position, (index, indented, group)) in entries.into_iter().enumerate() {
                if multi && endpoint.collapsed {
                    break;
                }
                let workspace = &snapshot.workspaces[index];
                if selected && workspace.focused {
                    highlighted[0] = Some(space_rows);
                }
                let unit = plan.as_ref().and_then(|plan| plan.unit_of(index));
                // The lifted row and the rows it carries, such as its group's
                // children, follow the pointer together while it floats.
                let carried = unit.is_some_and(|unit| unit == drop_dragged) && floating;
                let shift = match (drag, unit) {
                    (Some(drag), Some(_)) if carried => {
                        drag.pin(&workspace.workspace_id, f32::from(drag.offset()), now);
                        drag.offset()
                    }
                    (Some(drag), Some(unit)) => {
                        let (at, moving) = drag.slide(&workspace.workspace_id, shifts[unit], now);
                        sliding |= moving;
                        px(at)
                    }
                    _ => px(0.),
                };
                if let Some(unit) = unit {
                    drop_rows.push((unit, space_rows, shift));
                }
                space_rows += 1;
                let id = workspace.workspace_id.clone();
                let press_id = id.clone();
                let context_id = id.clone();
                let hover_id = id.clone();
                let context_endpoint = endpoint_id.clone();
                let navigate_endpoint = endpoint_id.clone();
                let collapse_endpoint = endpoint_id.clone();
                let grouped = group.is_some() || indented;
                let tree = match (indented, closes[position]) {
                    (false, _) => RowTree::None,
                    (true, false) => RowTree::Child,
                    (true, true) => RowTree::LastChild,
                };
                let fold = group.map(|key| Fold {
                    id: SharedString::from(format!("collapse-{endpoint_id}-{id}")).into(),
                    index,
                    collapsed: collapsed_repos.contains(&key),
                    toggle: Box::new(cx.listener(move |this, _, _, cx| {
                        let collapsed = if collapse_endpoint == crate::endpoint::LOCAL {
                            &mut this.collapsed_repos
                        } else if let Some(endpoint) = this
                            .endpoints
                            .iter_mut()
                            .find(|e| e.id == collapse_endpoint)
                        {
                            &mut endpoint.collapsed_repos
                        } else {
                            return;
                        };
                        if !collapsed.remove(&key) {
                            collapsed.insert(key.clone());
                        }
                        cx.notify();
                    })),
                });
                let label = workspace_label(workspace, indented);
                let shown_status = if self.config.usage.inline {
                    displayed_workspace_status(&snapshot.workspaces, workspace, collapsed_repos)
                } else {
                    workspace.agent_status
                };
                let lines = if spaces_custom {
                    tokens::space_rows(
                        &self.config.sidebar_layout.spaces,
                        SpaceContext {
                            label,
                            branch: workspace.branch.as_deref(),
                            status: shown_status,
                            ahead_behind: workspace.git_ahead_behind,
                            tokens: &workspace.tokens,
                            indented,
                        },
                    )
                } else {
                    Vec::new()
                };
                let gap = if spaces_custom {
                    match previous_indented.replace(indented) {
                        Some(_) if !indented => {
                            f32::from(self.config.sidebar_layout.spaces.row_gap) * line_height(font)
                        }
                        _ => 0.,
                    }
                } else {
                    0.
                };
                let removing_row = selected
                    && self.live.status.is_connected()
                    && self.removal.as_ref().is_some_and(|removal| {
                        removal.pending_for(
                            (self.selection_epoch, endpoint.generation),
                            &snapshot.boot_id,
                            &workspace.workspace_id,
                        )
                    });
                // Lines under the row start under its label column, clear of
                // the status dot.
                let under_left = content_x
                    + row_cx.nest
                    + if indented {
                        layout.child_indent() + indicators.width(font) - STATUS_WIDTH
                    } else {
                        0.
                    }
                    + indicators.width(font)
                    + layout.gap();
                let note = workspace_note(
                    workspace,
                    crate::worktree_notes::Notes::of(cx),
                    &endpoint.id,
                )
                .map(SharedString::from);
                let note_line = note.clone().map(|note| {
                    let endpoint = endpoint_id.clone();
                    let workspace = id.clone();
                    crate::sidebar::note_line::element(
                        &format!("{endpoint_id}-{id}"),
                        note,
                        (under_left, content_x),
                        font,
                        theme,
                    )
                    .on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.edit_worktree_note(
                                &endpoint,
                                &workspace,
                                event.position(),
                                window,
                                cx,
                            );
                        },
                    ))
                });
                let ports = (self.config.show_listening_ports && !removing_row)
                    .then_some(daemon.as_ref())
                    .flatten()
                    .and_then(|daemon| self.listening_ports.get(daemon, &workspace.workspace_id))
                    .map(|listed| {
                        div()
                            .debug_selector(|| format!("ports-{endpoint_id}-{id}"))
                            .h(px(line_height(font)))
                            .flex_none()
                            .w_full()
                            .min_w_0()
                            .pl(px(under_left))
                            .pr(px(content_x))
                            .text_size(px((font.size * 0.85).round()))
                            .child(crate::listening_ports::chips(
                                listed,
                                (&endpoint_id, &workspace.workspace_id),
                                &self.tunnels,
                                theme,
                                (font.size * 0.85).round(),
                                cx,
                            ))
                    });
                let element = Cell::new(
                    rows,
                    RowData::Workspace(WorkspaceRow {
                        workspace,
                        label,
                        tree,
                        icon: if indented || !self.config.show_repository_icons {
                            RowIcon::None
                        } else {
                            self.avatars
                                .as_ref()
                                .filter(|_| endpoint_index == 0)
                                .and_then(|avatars| avatars.image(&workspace.new_workspace_cwd))
                                .map_or(RowIcon::Mark, RowIcon::Avatar)
                        },
                        fold,
                        grouped,
                        badge: workspace_badge(
                            workspace,
                            (endpoint_index == self.selected_endpoint)
                                .then_some(&self.menu.pr_cache),
                            &self.git,
                            (&self.teleport_marks, &endpoint.id),
                            note.as_deref(),
                            theme,
                        ),
                        removing: removing_row,
                        status: shown_status,
                        lines,
                    }),
                    &row_cx,
                )
                .selected(selected && workspace.focused)
                .highlighted(selected && menu_target == Some(id.as_str()))
                .lift(match (carried, floating) {
                    (true, _) => RowLift::Lifted,
                    (false, true) => RowLift::Passed,
                    (false, false) => RowLift::Resting,
                })
                .row()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if this.navigate_endpoint(
                            &context_endpoint,
                            NavigationTarget::Workspace(&context_id),
                            cx,
                        ) {
                            this.open_workspace_menu(&context_id, event.position, window, cx);
                            this.menu.opening_right_click =
                                this.menu.page == Some(crate::menu::Page::Workspace);
                        }
                    }),
                )
                .when(
                    self.menu.page == Some(crate::menu::Page::Workspace),
                    |row| {
                        let view = cx.entity().downgrade();
                        let endpoint = endpoint_id.clone();
                        let workspace = id.clone();
                        row.child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    let bounds = bounds.intersect(&window.content_mask().bounds);
                                    window.on_mouse_event(
                                        move |event: &MouseDownEvent, phase, window, cx| {
                                            // The overlay dismisses first in bubble order. Use
                                            // clipped row geometry because it occludes our hitbox.
                                            if phase == DispatchPhase::Bubble
                                                && event.button == MouseButton::Right
                                                && bounds.contains(&event.position)
                                            {
                                                let _ = view.update(cx, |this, cx| {
                                                    cx.stop_propagation();
                                                    if this.navigate_endpoint(
                                                        &endpoint,
                                                        NavigationTarget::Workspace(&workspace),
                                                        cx,
                                                    ) {
                                                        this.open_workspace_menu(
                                                            &workspace,
                                                            event.position,
                                                            window,
                                                            cx,
                                                        );
                                                        this.menu.opening_right_click = this
                                                            .menu
                                                            .page
                                                            == Some(crate::menu::Page::Workspace);
                                                    }
                                                });
                                            }
                                        },
                                    );
                                },
                            )
                            .absolute()
                            .inset_0()
                            .size_full(),
                        )
                    },
                )
                .id(SharedString::from(format!("workspace-{endpoint_id}-{id}")))
                .when(multi, |row| {
                    row.debug_selector(|| format!("workspace-{endpoint_id}-{id}"))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.navigate_endpoint(
                        &navigate_endpoint,
                        NavigationTarget::Workspace(&id),
                        cx,
                    );
                    window.focus(&this.focus, cx);
                }))
                // Only the selected endpoint's rows arm the hover menu:
                // another endpoint's menu would have to select it first, and
                // resting the pointer must not switch which daemon is shown.
                // The feature is opt-in, so rows stay unarmed without it.
                .when(selected && self.config.features.sidebar_hover_menu, |row| {
                    row.on_hover(cx.listener(move |this, hovered: &bool, window, _| {
                        this.hover_workspace(&hover_id, *hovered, window);
                    }))
                })
                // Holding a press lifts the row for reordering. Another
                // endpoint's rows would have to select it first.
                .when(selected, |row| {
                    row.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if event.click_count == 1 {
                                this.press_workspace(&press_id, event.position, cx);
                            }
                        }),
                    )
                })
                .when(carried, |row| row.cursor_grabbing());
                // The note and ports ride under their row as one list item, so
                // the drop preview still measures one height per workspace.
                let element = if note_line.is_none() && ports.is_none() {
                    element
                        .when(gap > 0., |row| row.mt(px(gap)))
                        .when(shift != px(0.), |row| row.top(shift))
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .w_full()
                        .relative()
                        .when(gap > 0., |unit| unit.mt(px(gap)))
                        .when(shift != px(0.), |unit| unit.top(shift))
                        .child(element)
                        .children(note_line)
                        .children(ports)
                        .into_any_element()
                };
                spaces = if carried {
                    // Painted last so it floats over the rows it passes, while
                    // its layout slot keeps the others' positions stable.
                    spaces.child(deferred(element).with_priority(1))
                } else {
                    spaces.child(element)
                };
            }
            // Agent rows come after every host: the panel interleaves them.
            // Only a host the panel lists can make the view a filtered one.
            if self.config.show_agents && endpoint.enabled {
                filtered |= snapshot.agent_view_label.is_some();
            }
        }
        WorkspaceRows {
            spaces,
            highlighted,
            host_rows,
            drop_rows,
            drop_requests,
            drop_dragged,
            sliding,
            filtered,
        }
    }
}
