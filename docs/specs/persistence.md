# Canvas persistence

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [walking-skeleton.md](walking-skeleton.md) (the canvas and its terminal blocks) and
  [config.md](config.md) (the shell's start folder, and how `config.toml` is written safely)

## Goal

Closing Templum keeps the canvas. Opening it again brings back:

- the camera;
- every terminal block, with its place, its size and which block is in front;
- a fresh shell in each block's last folder;
- the conversation of a coding agent (Claude Code, Codex) that was open in a block.

The saved file is the first piece of a bigger plan. Templum will have workspaces (one canvas each, such as "work" and
"personal") and blocks that aren't terminals (notes, a kanban, images, shapes, text, groups). The format leaves room
for them, so adding them later needs no migration. Only one workspace and terminal blocks exist in this spec.

**Success criteria**

1. Quitting and reopening restores everything in [What comes back](#what-comes-back) on Linux, macOS and Windows.
   CI checks this on every OS, and Linux (Hyprland) is also checked by hand.
2. A crash loses at most the last 5 seconds of changes.
3. A block where Claude Code or Codex was in the foreground reopens on that folder's latest conversation.
4. No content of the saved file can crash Templum or stop it from starting. A file Templum can't read is kept, never
   overwritten.
5. All tests in [Testing](#testing) pass in CI on the 3 OS, and `--bench` stays at p95 ≤ 16.7 ms.

## Out of scope

- **Workspaces as a feature:** creating, naming and switching them. Here there is one, `default`.
- **Other block kinds:** notes, kanban, images, shapes, text and groups.
- **What was on screen:** the text and scrollback of a terminal.
- **Programs other than the two agents:** vim, htop or a server don't restart; their block gets a shell.
- **Resuming each agent's exact conversation:** two agents in one folder both resume that folder's latest one. Exact
  resume needs each block to know its agent's session, which belongs to a later agent integration.
- A setting to start with an empty canvas, and more than one window.

## What comes back

When Templum starts and a saved canvas exists:

- **Camera:** its position and zoom. The zoom snaps to the nearest of the canvas's fixed zoom steps.
- **Blocks:**
  - Each terminal block comes back at its place and size, in the same front-to-back order. They pop in like new
    blocks do.
  - The block that had the focus has it again. The focus raises a block, so it is the front one.
- **Shells:** each block gets a new shell, started in the block's last folder, not in `[shell] working_directory`. A
  folder that no longer exists falls back to the home folder, and one notification lists the blocks it happened to.
- **Agents:** see [Agents](#agents).

What doesn't come back: the text that was on screen, and programs other than the agents.

**Other starts**

| Situation | Start |
|---|---|
| No saved canvas (first run) | One terminal in the middle, as today. |
| A saved canvas with no blocks (the user closed them all) | The empty canvas and its hint. |
| A saved file Templum can't read ([Errors](#errors)) | As on a first run. |
| `--bench` | As on a first run. Nothing is read or written. |

## Agents

- **Detection:** while saving, each block reports whether Claude Code or Codex is open in it.
  - **Linux and macOS:** the PTY's foreground process (the one `cwd()` already finds) is checked.
  - **Windows:** ConPTY has no foreground process, so the shell and its descendant processes are searched.
- **What counts as an agent:**

  | The process | Agent |
  |---|---|
  | Named `claude` (`claude.exe` on Windows), or with `claude` as its first argument (npm's Claude Code sets its process title, which macOS shows there) | Claude Code |
  | Named `codex` (`codex.exe`), or with `codex` as its first argument | Codex |
  | `node` (`node-MainThread` on Linux with Node 26) whose script, its first argument that isn't an option, has `@anthropic-ai/claude-code` in its path | Claude Code (npm install) |
  | `node` whose script has `@openai/codex` in its path | Codex (npm install) |
  | `node` whose script is named `claude` or `codex` with no extension (an npm bin link) | That agent |
  | Anything else (`vim`, `node server.js`, `node claude.js`, the shell itself) | None |

- **The block's folder:** with an agent open, it is the agent process's folder. This matters on Windows, where
  PowerShell's `cd` doesn't move PowerShell's own folder, but the programs it starts do run in the new one.
- **Restore:** right after the block's shell starts, Templum types the agent's resume command and Enter into it,
  only when the block starts in its saved folder. Anywhere else (a folder that's gone, or wasn't known) it would
  resume another folder's conversation.
  - Claude Code: `claude --continue`, which continues the most recent conversation in the folder.
  - Codex: `codex resume --last`, which continues the most recent session in the folder.

  The terminal buffers the typed text until the shell is ready. When the agent exits, the user is back in the shell.
  If the agent isn't installed, the shell says so ("command not found") and nothing else happens.
- **Safety:** the file stores only the agent's name, never a command. A `"agent"` value other than `"claude"` or
  `"codex"` is ignored, so an edited file can't make Templum type anything else.

## The file

- **Where:** one file per workspace, `<data folder>/workspaces/default.json`. The data folder is:

  | OS | Data folder |
  |---|---|
  | Linux | `$XDG_DATA_HOME/templum`, falling back to `~/.local/share/templum` |
  | macOS | `~/Library/Application Support/Templum` |
  | Windows | `%APPDATA%\templum` (next to `config.toml`) |

  With no data folder (no HOME or APPDATA), nothing is read or written, and nothing says so: the canvas simply
  isn't kept.
- **Format:** JSON (`serde_json`, already in the tree).

  ```json
  {
    "version": 1,
    "camera": { "x": -120.0, "y": 40.0, "zoom": 1.0 },
    "items": [
      {
        "kind": "terminal",
        "rect": { "x": 0.0, "y": 0.0, "w": 720.0, "h": 460.0 },
        "folder": "/home/winx/proj",
        "agent": "claude"
      }
    ],
    "focused": 0
  }
  ```

  - **`version`:** goes up only when an older Templum couldn't read the new file correctly. Additions it can ignore
    don't change it.
  - **`camera`:** the world point at the top left of the canvas, and the zoom factor. During a zoom glide
    ([polish.md](polish.md#camera-flights)) it is where the camera is going.
  - **`items`:** back to front, so the last is drawn on top. `kind` names the item's type. Only `"terminal"` exists
    now, and later kinds add their own fields.
  - **A terminal's fields:**
    - `folder` is missing when the OS wouldn't say (another user's process, already gone), and the shell then starts
      in the home folder;
    - `agent` is `"claude"`, `"codex"` or missing.
  - **`focused`:** the index of the focused item, or `null`.
- **Unknown content:** unknown fields are ignored. An unknown `kind` or a newer `version` makes the whole file
  unreadable ([Errors](#errors)), so an older Templum never drops what a newer one wrote.

## When it saves

- **Every 5 seconds:** Templum takes a snapshot (camera, blocks, folders, agents) and writes it only when it differs
  from the last one written. A move, a resize, a pan, a `cd` or an agent opening all show up in the next snapshot,
  with no event for each.
- **On quit:** when the window closes or Templum quits, a last snapshot is written at once, next to the pending
  `config.toml` save.
- **Off the UI thread:** the UI thread only gathers each block's rect and process ids. Reading the processes
  (folders, agents) and writing the file run in a background task, so a snapshot never costs a frame. On quit the
  same work runs at once, before Templum exits.
- **Safely:** the write goes through the temporary file and rename that `config.toml` uses, so a crash never leaves
  half a file. Writes take turns, so a quit during a background write can't mix the two.

## Errors

| Situation | Behavior |
|---|---|
| The file doesn't parse, or has an unknown `kind` | It is renamed to `default.json.bak` (replacing an older `.bak`). Templum starts as on a first run, and a notification says where the old file went. |
| `version` is newer than this Templum knows | The same, so a newer Templum's canvas survives a downgrade. |
| Reading fails (permissions) | The same, without renaming, and nothing is written for the rest of the session, so the file isn't replaced. |
| Writing fails (disk full, read-only) | A notification the first time. Templum keeps trying every 5 seconds without new notifications, and says when a write works again. |
| A block's folder no longer exists | The shell starts in the home folder. One notification lists those blocks. |
| A block's shell can't start | The block is skipped, with the notification a new block already gives. |

## Architecture

| File | Responsibility |
|---|---|
| `session.rs` (new) | The data folder (`dir()`, tested like `config::dir`), the file's serde types, `load(dir)` (with the `.bak` handling) and `save(dir, &Saved)`, and the conversion between a `Saved` and the canvas. |
| `config.rs` | `write_whole` (temporary file, rename, read-only check) becomes `pub(crate)` and names its temporary file and error after the file it writes. |
| `workspace.rs` | `Workspace::with` gets the data folder. It restores the canvas, starts the snapshot every 5 seconds (and runs it at once on quit), and holds the last written snapshot. Restoring starts each block in its folder and types an agent's resume command into it. |
| `main.rs` | Passes `session::dir()`, or nothing for `--bench`. |
| `templum-terminal` | `Terminal::processes()`, the block's process ids (`Send`), and `inspect(ids)`, which reads a block's folder and agent from any thread. `Agent` (`Claude`, `Codex`, with `resume_command()`) and the pure `agent_of(name, args)` that matches the table in [Agents](#agents). The folder is the agent's when one is open. |
| `templum-canvas` | `Camera::set_zoom(f32)`, which snaps to the nearest step. |

`templum-canvas` stays free of serde: the file's types live in `session.rs`. No new dependency.

## Testing

Tests are written first, as always. Everything runs in CI on Linux, macOS and Windows.

**Unit:**

- **`session`:**
  - a `Saved` written and read back is equal;
  - item order and `focused` survive;
  - a zoom between steps snaps to the nearest;
  - a broken file becomes `.bak` and loads as nothing;
  - `version: 2` and `kind: "note"` do the same;
  - an unknown `agent` loads as no agent;
  - unknown fields are ignored;
  - `dir()` follows each platform's convention.
- **`agent_of`:** every row of the table in [Agents](#agents), including `node server.js` and the shell as none.
- **`Camera::set_zoom`:** exact steps, values between steps, and values beyond the ends.

**With a real process (`templum-terminal`):**

- A copy of a harmless long-running program named `claude` (`sleep` on Unix, `cmd.exe` on Windows), run in a
  terminal, makes `inspect` say Claude Code and give its folder.

**Headless UI** (`#[gpui_kit::test]`, temporary data folder):

- Two blocks moved, one raised, the canvas panned and zoomed, then saved: a new workspace opened on the same folder
  has the same rects, order, camera and focus.
- A saved canvas with no items opens empty, with the hint.
- A saved block with `"agent": "claude"` gets `claude --continue` typed into its terminal (the test shell echoes it).
- A saved folder that doesn't exist starts its shell in the home folder, with one notification.
- A broken file: the notification shows, `default.json.bak` holds the old bytes, and one terminal opens.
- The snapshot writes only when something changed (two ticks with no change: one write).
- With no data folder (`--bench`), nothing is written.

**Manual (Hyprland):**

- Three terminals in different folders, one running `claude`. Quit and reopen: places, folders and the Claude
  conversation come back.
- Kill Templum (`kill -9`) a few seconds after a move: the move is kept.

## Delivery

Three pull requests, each green in CI and useful on its own:

1. **This spec.**
2. **Save and restore the canvas:**
   - `session.rs`, the data folder and the file;
   - the 5-second snapshot in the background (`Terminal::processes`, `inspect` for folders) and the save on quit;
   - restoring camera, blocks, order, focus and folders;
   - the errors table;
   - README ("Your canvas is kept") and ARCHITECTURE.
3. **Agents:** `Agent`, `agent_of`, agents in `inspect`, the folder rule, typing the resume command, and the docs.

## Risks

- **Process reads on quit.** On Windows, the agent search walks the whole process list (milliseconds with a few
  hundred processes). It runs in the background during use, but on quit it delays closing by that much, which is
  acceptable once.
- **Typed-ahead input.** Some shell setups clear input typed during startup (zsh plugins with an "instant prompt").
  The resume command would then be lost: the user sees a plain shell and types it. The manual check covers zsh.
- **Agents' resume flags** are other programs' interface (`claude --continue`, `codex resume --last`, both checked on
  2026-10-10). If they change, only `Agent::resume_command` does.
