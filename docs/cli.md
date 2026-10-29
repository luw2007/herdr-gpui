# Command line

`herdr-gpui` is also a command line for the running Herdr UI. Herdr's own
`herdr` CLI drives the daemon: workspaces, terminal tabs, panes, agents,
worktrees, and notifications. `herdr-gpui COMMAND` covers what only this
client knows or draws: its windows, its own tabs (pages, code, reviews, the
orchestrator, Devices), worktree notes, checkpoints, forwarded ports, and its
settings.

Commands reach the app over a socket of its own,
`$XDG_STATE_HOME/herdr/gpui/control.sock` (default `~/.local/state/`), which
only your user can open. An agent on an SSH host has no path to it; there,
the app's shim at `~/.local/state/herdr/gpui/bin/herdr-gpui` offers
`browser open` and `browser reload` through Herdr workspace metadata (see
[remote agents](../crates/herdr-gpui/README.md#letting-agents-open-pages)),
and every other command exits 8. A few commands work from files alone and
need no running app; they say so below.
Windows builds have no control socket yet: commands that need the app exit 8.

```sh
herdr-gpui --help              # every command
herdr-gpui COMMAND --help      # one command
```

## Options and output

Every command accepts:

- `--json`: print one JSON document instead of text. Errors become
  `{"error": {"code", "message"}}` on standard output.
- `--window ID`: act in that window (IDs come from `window list`). By default
  a command acts in the window showing the caller's workspace
  (`HERDR_WORKSPACE_ID`, which Herdr sets in every pane). When no window shows
  it, as for a session the app does not show, the command exits 5 rather than
  act on another session's window. Outside Herdr, it acts in the frontmost
  window.

| Exit | Meaning |
| --- | --- |
| 0 | Done |
| 1 | Refused or failed |
| 2 | Invalid arguments, or an address or file that cannot be shown |
| 3 | Herdr UI is not running |
| 4 | Nothing yet: no notes, no event, no pull request, condition not met |
| 5 | The window, workspace, tab, host, or file is not there |
| 6 | A dialog is open in the window, or the app is busy |
| 7 | The app did not answer in time |
| 8 | Not available on this platform or in this version of the app |

Exit 1 also covers a name that fits more than one thing (`project` commands:
two projects or devices share it); `--json` then reports the error code
`ambiguous`.

## Commands

### App, windows and Spaces

`window` commands act on native OS windows. `space` commands act on
Spaces: global, named sets of sessions shared by every native window.
`--window ID` names the native window whose view a command uses (which Space
`focus` changes, which one `list` marks with `*` and a bare `attach` targets).
A SPACE argument is a Space ID from `space list` or its exact name.

| Command | What it does |
| --- | --- |
| `status` | Whether the app runs, its version and control protocol, windows, and hosts |
| `app focus\|hide\|quit` | Bring the app forward, hide it, or quit it (Herdr keeps running) |
| `window [list]` | Windows with their IDs, size, workspace, and hosts |
| `window new [--session NAME]` | Another window, of your own Herdr session or a named local session |
| `window focus ID` | Bring a window forward |
| `window fullscreen [on\|off\|toggle]` | |
| `window size [WIDTHxHEIGHT]` | Print or set the window's size; windows cannot be moved |
| `window detach\|reconnect` | Detach from, or reconnect to, the selected host |
| `space [list]` | Spaces with IDs, names, grouping, their sessions and connection status; `*` marks the one this native window shows. The answer is bounded: a Space whose sessions do not fit says how many it left out (`sessions_total` in `--json`) |
| `space overview` | Open Space Overview in the targeted native window; refuses while another dialog owns input |
| `space new [--title TITLE]` | Add an empty Space and show it here; no daemon is started |
| `space focus SPACE` | Show a Space in this native window only |
| `space rename SPACE TITLE` | Rename a Space; names are unique, 1-64 printable characters |
| `space close SPACE` | Ask to close an empty Space for every native window: the app opens its close confirmation in the targeted window and the command answers `close_pending`, so nothing is closed yet. Confirming closes only that Space, and the last Space is replaced by an empty default. A Space that lists sessions is refused (exit 1), so detach them first |
| `space move SPACE POSITION` | Move a Space to a 1-based place in the list |
| `space group SPACE none\|project\|status\|mr` | Record how the Space's sessions are grouped; the sidebar groups by `project`, `status`, or `mr` (pull request state, derived from each session's saved workspaces; github.com on this machine and SSH devices only, others show Unsupported). The Space's right-click menu offers the same four values. While the shown Space groups by `mr`, its member sessions in the Sessions list carry a compact badge (bucket, PR number, mixed/stale); nothing is shown otherwise |
| `space attach SESSION [--device ID] [--in SPACE]` | Attach a session of the shown device (or of `--device`, an ID from `host list`) to the shown Space (or `--in SPACE`). View only: it creates, stops, and deletes nothing, so create sessions with `herdr` or the Sessions manager |
| `space detach SESSION [--device ID] [--in SPACE]` | Remove a session from a Space's list; the session keeps running |
| `space move-session SESSION POSITION [--device ID] [--in SPACE]` | Move a session to a 1-based place in a Space's list |
| `ui sidebar\|statusbar [on\|off\|toggle]` | For this window and session |
| `ui font-size +\|-\|reset\|POINTS` | The terminal font size for this session |
| `run COMMAND` | Any app command by its `[keybindings]` name, as the palette runs it |
| `commands` | The names `run` accepts (no app needed) |
| `sessions [QUERY...] [--device DEVICE] [--project PROJECT \| --no-project]` | Open the Sessions manager with an optional saved-project-context search and Device / Project filters |

Space management in the app: right-click a Space tab in the strip, or use the
`…` on the selected tab, to rename it, close it, or group it by none, project,
status or mr. Closing asks first: **Cancel** keeps the Space and only
**Close Space** closes it. A Space that lists sessions is not offered for
close at all (the menu says to detach them first), and the tab's × follows the
same rules. These use the same operations as `space rename`, `close` and
`group`, and `space close` cannot skip the confirmation.

The top-right grid button opens **Space Overview**. Its configurable command
is `space_overview` (`Cmd-Shift-E` on macOS, `Ctrl-Shift-E` on Linux).
The Space creation/stepping keybinding names are `new_space`, `next_space`,
and `previous_space`; replace the former `*_container` names in custom
`[keybindings]`. The CLI family is now `space`, not `container`; native
`window` commands and saved Space IDs, names and files are unchanged.

`sessions` opens the same manager as the app's Sessions command, in the
targeted window. Its query matches project, path and labels from saved
workspaces, cached Git branches, and session and device names. Saved context
comes from `session.json`; read-only Git probes resolve linked worktrees to
their repository and cache the branch for 30 seconds. Neither is a live task
activity feed. Missing, damaged or oversized metadata remains unclassified.
Queries may contain spaces but must be printable and at most 4096 UTF-8 bytes.
`--device` takes a device ID or exact label (as `project bind --device` does),
`--project` a project ID or exact name, and `--no-project` keeps only sessions
that resolve to no project; the three combine with the query and the manager's
All/Running/Stopped filter. Without `--device` the manager starts on the device
the window's footer shows (All Devices when it shows that), exactly as opening
it from the app does; an explicit `--device` wins over that. An unknown device
or project exits 5 (not found),
an ambiguous one exits 1, both before anything opens. In the app, right-click a Space tab to
rename it, close it (asking first, and only when it lists no sessions) or
choose its grouping.
`windows.json` is version 2 only while a session is stopped (`paused`); a file
without stopped sessions stays version 1, and both are read, so Stop survives a
restart until the session is selected again.
The user selects a session and confirms actions in the manager. This command
does not duplicate daemon session list, stop, or delete commands. An open
dialog refuses it with exit 6; an absent target never falls back to another
window. `--json` reports that the manager opened, not a session inventory.

### The app's own tabs

| Command | What it does |
| --- | --- |
| `tab [list]` | Pages, code, reviews, the orchestrator, and Devices tabs with IDs |
| `tab focus\|close ID` | |
| `browser open\|reload\|feedback` | See [browser tabs](../crates/herdr-gpui/README.md#browser-tabs) |
| `browser list` | Page tabs only |
| `browser navigate ID URL` | Load another address in a page tab |
| `browser back\|forward ID` | |
| `browser text ID` | The page's visible text |
| `browser screenshot ID PATH` | A PNG of the visible page, at most 2000 px a side (macOS) |
| `code open FILE[:LINE]` | A read-only code tab at a line |
| `code edit FILE[:LINE]` | Your terminal editor, in a new pane beside yours |
| `devices` | The Devices overview |
| `orchestrator [--repo PATH]` | The orchestrator of your checkout, or of PATH |
| `orchestrator items\|issues\|prs` | What it last synced, from its local database (no app needed) |
| `review` | The review of your workspace's uncommitted changes |

### Worktrees and agents

| Command | What it does |
| --- | --- |
| `notes [list]` | Every worktree note |
| `notes get\|clear`, `notes set TEXT` | The note on your checkout (its repository and branch) |
| `feedback [--wait S]` | Notes the user sent your pane from pages, reviews, or terminals |
| `checkpoint [list]` | Checkpoints of the current directory's checkout, newest first (no app needed) |
| `checkpoint create [LABEL]` | Take one now (no app needed) |
| `checkpoint diff A [B]` | What changed between two, or since one (no app needed) |
| `checkpoint restore [ID]` | Open the checkpoints dialog; you confirm there |
| `scripts [list]` | The repository's `.herdr/worktree.toml` scripts (no app needed) |
| `scripts setup\|run` | Run one in a new tab; an unseen script still asks first. Archive removes the worktree, so it stays behind the app's delete dialog |
| `teleport --to HOST` | Open the teleport dialog for your linked worktree, reviewing HOST; you confirm there |
| `fan-out PROMPT [--agent KIND[:N]]...` | Open the fan-out dialog prefilled; you start it there |

### Projects

| Command | What it does |
| --- | --- |
| `project [list]` | Every project: id, name, repository (or task project), per-device folder bindings, and how many sessions are assigned |
| `project manage` | Open the Sessions manager's Projects screen, even with no projects or sessions; no records change until the user submits an edit |
| `project create NAME [--repo KEY\|URL \| --task]` | A git project from a normalized key or a remote URL, or (default) a task project |
| `project rename PROJECT NEW_NAME` | |
| `project bind PROJECT PATH [--device ID]` | Tie an absolute folder of a device (default: the window's) to a project |
| `project unbind PATH [--device ID]` | Drop that binding |
| `project assign PROJECT [--session NAME] [--device ID]` | Put a session in a project (default: the session the window is attached to) |
| `project unassign [--session NAME] [--device ID]` | Drop its explicit assignment |
| `project show [--session NAME] [--device ID]` | The session's project and why: assigned, folder binding, or repository |
| `project delete PROJECT` | Open the app's delete-project confirmation in the Sessions manager; you confirm there |

`project manage` opens the same Projects manager as **Sessions → Projects**
or the command palette's **Projects** entry (`run project_manager`).
The Projects entry remains available when the Sessions list is empty. Use
**Create project** to add a logical project, then choose a saved device (including
an offline device) and enter an absolute folder path to bind it. Select a project
to rename it, add or remove device-folder bindings, or open its deletion
confirmation. These edits record associations in Herdr UI only: they never
create, move, or delete folders on a server. Opening the manager returns a bounded
acknowledgement, not a project inventory or confirmation of a saved edit. Like
every command, it accepts `--window ID` and `--json`; a busy dialog or an unmatched
caller window is refused instead of acting elsewhere.

A project is a logical folder across devices: a git repository (identified by
its normalized remote, so clones on every device join it) or a task codename.
Background session scans automatically register repositories and conservative
device-local path projects for saved workspaces without a usable remote. These
discoveries persist across restarts. Manual folder bindings and explicit session
assignments take priority; inferred paths do not override later repository
metadata. Workspaces retain saved order: the first workspace that resolves
wins, with manual binding, repository, then inferred path checked within it.
No remote path is probed on the local machine. A session is in at most
one project, or none. `PROJECT` is an id (the UUID `project list` prints) or an
exact name, compared without regard to case; an unknown one exits 5, and a
name two projects share exits 1 with code `ambiguous` and the ids to choose
from. `--repo` takes a remote URL (`https://`, `ssh://`, `git://`, or
`git@host:owner/repo`) or a key such as `github.com/owner/repo`, and anything
else is refused (exit 1) before a project is made. `--device` is an endpoint
id (`local`, `ssh:ID`, `wsl:DISTRO`) or a device's label as `host list`
prints it; without it the device showing your workspace, or the window's
selected one outside Herdr, is used. `--session` is a session name; without
it the session the device is attached to, which the command cannot tell for
a device reached by a bare socket path (name it with `--session`).

`project show` reads the saved workspaces the background session scan last saw;
it starts no scan, so a session not yet scanned resolves only by assignment and
the answer says so. `unassign` only removes an explicit assignment: a session can
still be placed by its folder or repository. `unbind` removes manual bindings,
not inferred path discovery. Deleting a project does not suppress discovery;
a later successful scan may recreate it. Historical path projects are retained
when later repository metadata places their sessions elsewhere. `list` shows up to 200 projects and
16 bindings each, with totals when more exist. A duplicate project name or
repository, a folder already bound to another project, a relative path, and
any value that is empty, too long (names and references 256 bytes, paths and
repositories 1024, session names 64), or holds control characters are
refused (exit 1).

`project delete` never deletes. It opens the Sessions manager on the window's
own delete-project dialog and reports that confirmation is pending (`--json`:
`{"kind": "delete_pending", "project": {...}}`); the user confirms or cancels
there. An open dialog refuses it with exit 6. The sessions stay running, and
no folder or worktree is touched.

### Hosts

| Command | What it does |
| --- | --- |
| `host [list]`, `host select ID` | The window's hosts; show another one |
| `pr [status]\|checks\|comments\|open` | Your branch's pull request, on the window's selected host. `comments` needs your workspace focused |
| `ports [list]` | Ports your workspace listens on, and forwards |
| `ports forward\|unforward PORT` | Forward a port of the SSH host to this machine |
| `ports open PORT` | `localhost:PORT` in a tab, forwarded first when remote |
| `usage`, `system` | Plan usage and load of the selected host |
| `cloud [list]` | Saved cloud machines (no app needed) |
| `cloud add` | Open the settings that add one |
| `processes [--pane ID]` | The processes under a pane, with CPU and memory (no app needed) |
| `search symbol\|file QUERY` | Go to Symbol or Go to File over the current checkout (no app needed) |

### Settings and services

| Command | What it does |
| --- | --- |
| `config [path]` | The settings files (no app needed) |
| `config get\|set\|unset KEY [VALUE]` | Read or change `config-gpui.local.toml`, keeping its comments. A value that would not load, or an unknown key, is refused and nothing is written; the app applies the rest at once (no app needed) |
| `config check` | Report invalid settings, exit 1 if any |
| `config reload` | Reread Herdr's config and this app's |
| `theme [list]`, `theme set NAME` | The app theme. A device's terminal theme is `config set devices.ID.theme NAME`, with the ID from `host list` minus its `ssh:` prefix |
| `font [list]`, `font set NAME`, `font reset` | |
| `keybinds [list]`, `keybinds check` | Shortcuts; custom Herdr commands this client cannot reach |
| `caffeine [status]`, `caffeine on\|off\|toggle [--for 30m]` | Keep the machine awake |
| `sound [list]`, `sound play [done\|request]`, `bell` | Sounds play at `sound.volume` percent (`config set sound.volume 30`) |
| `update [status]`, `update check` | Never downloads or installs |
| `integrations [list]` | What Herdr offers the selected host |
| `integrations install-skill` | Install the agent skill (no app needed) |
| `logs [--lines N] [--path]` | The app's log (no app needed) |
| `diagnostics bundle PATH` | A redacted report for a bug (no app needed) |

Boat's sandbox lifetime uses the existing config commands:

```sh
herdr-gpui config set boat.auto_stop never
herdr-gpui config set boat.auto_stop '"7200"'  # seconds, stored as a string
herdr-gpui config get boat.auto_stop --json
herdr-gpui cloud add                         # open Cloud Devices, then choose Boat
```

`never` is the default: billing continues until the sandbox is stopped in Boat.
Numeric deadlines are fixed, even during active SSH work; trials require at
most 7200 seconds. The setting applies on create and resume. In Boat settings,
**Finish setup** extends a running sandbox from now or disables its deadline.
**Recover creation** retries a persisted creation request with its original
options and key; forgetting an unresolved request requires confirmation there.

Focus-on-hover between split terminal panes is on by default. Disable it with
`herdr-gpui config set focus_follows_mouse false`.
The change applies live; `config get focus_follows_mouse` reads it back.

### Events

`events` prints what happened in the app: windows opening, closing, and
gaining focus; the app's tabs opening and closing; notes kept for a pane;
worktree notes changing; and host status changes. Each event has a sequence
number; pass back the `INSTANCE:SEQ` cursor a call prints with `--after` to
get only newer events. A cursor from an earlier run of the app gets every
event the new run has kept, so a restart never hides events.

```sh
herdr-gpui events --follow --kind tab_closed
herdr-gpui wait tab-closed 12 --timeout 600
herdr-gpui wait window            # also waits for the app to start
```

## What stays in the app

Commands never bypass a confirmation. Restoring a checkpoint, teleporting a
worktree, fanning out a prompt, running a worktree script the app has not
seen, and adding a cloud machine open the app's own dialog (archiving a
worktree has no command at all), already pointed
at your checkout, and the user confirms there. Nothing here installs,
starts, stops, or upgrades Herdr, and `update` never installs the app.

Adding a device has no command either: the Add Device dialog runs it. Its
setup workspace runs `herdr-gpui __device-setup-attach SOCKET`, an internal
helper that shows the setup the app runs (see the Native Shell README's
"Local Network access"); it is not a command to call yourself, and the socket
it names is single use.

Scrollback search is not repeated here: `herdr pane read` covers it. Moving a
window, capturing a whole window, and destroying a cloud machine have no
command because the app cannot do them.

## The protocol

One JSON request line, one JSON response line, then the socket closes. A
request is `{"method", "caller", "op"}`, where `caller` carries the pane,
workspace, daemon socket, and window, and `op` is tagged with `"op"`. Browser
requests keep their original flat form (`browser.open`, `browser.reload`,
`browser.feedback`, `notes.send`). `status` reports the protocol version,
currently 10. Requests and responses are bounded to 256 KiB, a request that
waits says for how long (at most 600 seconds), and the app answers at most a
few requests at a time.
