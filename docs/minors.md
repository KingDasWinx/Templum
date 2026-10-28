# Minor issues to fix later

Small issues found by the final reviews of the Git block's second version (`docs/specs/git.md`), batches 1 and 2,
and left for later on purpose: none loses work or breaks a main path. Each line says where it is, what a person sees,
and a way to fix it. Every minor left for later is added here; when one is fixed (with a test that fails without it,
AGENTS.md), its entry is deleted in the same commit.

Line numbers are those of `development` at a5fed2d (2026-10-13).

## Diff panel

- [ ] **An empty new file gets an empty "Line 1" hunk row.** `new_file_diff` makes `@@ -0,0 +1,0 @@` for an empty
  file, drawn as a hunk with no lines. Fix: no hunk for an empty file; a note row "Empty file".
- [ ] **An untracked nested repository shows an empty name.** `status -uall` lists it as `dir/`, and the file row's
  name (the part after the last `/`) is empty. Fix: trim the trailing `/` for the name, and say "Repository" in a note.
- [ ] **A binary's size is the working copy's in every section.** A deleted binary says "Binary file · 0 B"; a staged
  binary shows the working file's size, not the staged one. Fix: say "Binary file" without a size when the file is
  gone, and read the index blob's size (`git cat-file -s :path`) for the Staged section.
- [ ] **A drag released outside the panel stays on.** `dragging` is cleared only by the panel's mouse-up
  (`crates/templum/src/workspace/git_changes.rs:513`), which fires only over the panel; a later drag over line rows
  (button held from elsewhere) moves the old selection's head. Fix: `on_mouse_up_out` on the panel, or clear
  `dragging` on any mouse-up in the workspace.
- [ ] **The diff keeps its colors after a theme switch.** The highlight cache is cleared only when the diff changes
  (`crates/templum/src/git_block/diff_view/mod.rs:137`); a light/dark switch keeps the old syntax colors until then.
  Fix: clear every workspace's Git's `panel.colors` where the theme is applied.
- [ ] **Any change re-reads everything: the selection and every hunk's colors go.** `DiffPanel::set`
  (`crates/templum/src/git_block/diff_view/mod.rs:137-138`) clears both whenever any file changed, so while an agent
  is writing, a selection vanishes every 2 seconds. Fix: key the colors by (section, path, hunk header) instead of
  indices, and keep a selection whose rows still hold the same lines.
- [ ] **A blame's panel shows Ignore whitespace, which changes nothing.** The header keeps the button for every
  source (`crates/templum/src/git_block/diff_view/mod.rs`, `panel`); with a blame shown, `reload_shown_diff` has no
  commit to read again, so the button lights up and the lines stay. Fix: hide it for `Source::Blame`, or read the
  blame again with `git blame -w`.
- [ ] **In Split, a press on an empty right cell selects on the left.** `DiffPanel::side_at`
  (`crates/templum/src/git_block/diff_view/mod.rs`) reads the side from the right cell as painted; an empty cell has
  none, so the selection starts at the end of the left text and a drag selects left lines. Fix: decide the side from
  the row's origin and the half's width, not from a painted cell.

## Hunks and the keyboard

- [ ] **A refused hunk offers a useless Run in a terminal.** `apply_hunk`'s failure goes through `notify_git_failure`
  (`crates/templum/src/workspace/git_changes.rs:603`), whose Run in a terminal types `git apply … /tmp/templum-….patch`,
  a file already deleted. Fix: a plain error notification for a refused patch (as Undo commit's refusal does).
- [ ] **The Git's keys swallow a custom Alt or Shift shortcut.** `git_key` passes on only Ctrl, Cmd and Fn
  (`crates/templum/src/workspace/git_keys.rs:42`); a `[keys]` binding like `previous_prompt = "alt-up"` is taken by
  the Git while it has the keyboard. Every default shortcut uses Ctrl/Cmd, so only custom ones collide. Fix: return
  early when `self.keymap.command(&e.keystroke)` is `Some`.
- [ ] **Holding `s` repeats the stage of a stale patch.** The keys ignore `e.is_held` and `acting > 0`
  (`crates/templum/src/workspace/git_keys.rs:34`), so a held key queues the same patch again and again, and each
  refusal leaves a sticky "The file changed; try again". Fix: ignore held keys, and do nothing while `acting > 0`
  (the buttons already hide then).
- [ ] **With Ignore whitespace on, `s` stages the cursor hunk's whole file without asking.** The hunk can't go alone
  then (`crates/templum/src/workspace/git_keys.rs:184`), and `s` falls back to the file silently (`d` asks). Fix: say
  so in a notification, or ask.

## Commit

- [ ] **Commit & Push offers no Undo commit, even when the push fails or Publish is cancelled.** The Undo button is
  left out when `push` is set (`crates/templum/src/workspace/git_changes.rs:74`). Undo already checks whether the
  commit reached the upstream, so attaching it every time is safe. Fix: attach Undo commit in both cases.
- [ ] **Commit all changes adds an untracked nested repository as a gitlink.** `stage_all` runs `git add --all`
  (`crates/templum-git/src/repo.rs:407`), which adds `nested/` as an embedded repository with a warning nobody sees,
  and the label counts it as one file. Fix: leave out untracked entries ending in `/` (and say so), or ask.

## Branches, Sync and conflicts

From batches 3 and 4 (line numbers of `development` at 5046612).

- [ ] **The branch dialog's rows have lost their dim second line.** `branch_lines`
  (`crates/templum/src/git_block/branches.rs:359`) draws the commit's subject, its age and ↑↓ at the text's full color
  on every row of the dialog (`None => el`), not only on the selected one, so a person sees two lines of the same
  weight of color; only size and weight tell them apart. Fix: in the `CommandItem` closure, read the selected index of
  the `CommandState` and dim with `title_dim` every row but the selected one.
- [ ] **A branch can still be renamed, or a worktree made, with a remote branch's full name.** `rename_branch`
  (`crates/templum-git/src/repo.rs:1430`) and `add_worktree` (`:519`, whose name may hold `/`) make
  `refs/heads/origin/main` without the refusal `new_branch` has, so `origin/main` turns ambiguous and rebase, merge
  and Compare quietly use the stale local one. Fix: move `new_branch`'s `show-ref --verify refs/remotes/<name>`
  check into a helper the three call.
- [ ] **The branch dialog's Remote group can't be folded, as the Branches tab's can.**
  `crates/templum/src/git_block/switcher.rs`, `open`: a palette heading (`CommandGroup::label`) is a plain row with
  no click, so Remote is always open; with hundreds of remote branches a person sees 50 rows and "N more". Fix: a
  `Pick::Fold` item row ("Remote · N") that flips an `Rc<Cell<bool>>` kept by `open`, without closing the dialog, and
  that folds only while nothing is typed (a query that matches only remote branches must not hide them).

- [ ] **A comparison's "⋯ 40 unchanged lines" does nothing when clicked.** `fold_row`
  (`crates/templum/src/git_block/diff_view/paint.rs:460`) is inert on a read-only panel: Compare's diff is read once
  with git's 3 lines of context. Fix: read that file of the range again with `-U` and its length (as `show_whole`
  does for the working changes).
- [ ] **Changes left on a detached HEAD don't come back by themselves.** The switch question names the stash after
  the short hash (`templum: left on a1b2c3d`, `crates/templum/src/workspace/git_switch.rs:68`) and `Repo::switch`
  pops only a stash named after the branch it goes to (`crates/templum-git/src/repo.rs:1152`), so checking that
  commit out again leaves them in Stashes. Fix: pop by the commit on a detached checkout too, or offer Leave them
  only on a branch.
- [ ] **The default branch is read again after every action and tab change.** `refresh_lists_at` queues
  `load_default` each time (`crates/templum/src/workspace/git_ops.rs:254`): a few `rev-parse`s more per action.
  Fix: read it when the popup opens and after a fetch, a sync or a branch change only.
- [ ] **A remote or branch name with an unusual character is read wrong.** `words::local_name`
  (`crates/templum/src/git_block/words.rs:91`) and `Repo::switch` take what follows the first `/` as the local name
  (a remote named `team/x` breaks Back to and Compare with), and `unmerged_count`'s `--exclude`
  (`crates/templum-git/src/repo.rs:1023`) reads `*?[` in a branch name as a pattern. Fix: match the remote's name
  against `git remote`; escape the glob characters.

- [ ] **A switch that fails after leaving the changes may keep them stashed without saying so.** When the checkout
  fails partway, putting them back is `let _ = self.pop_whole(…)` (`crates/templum-git/src/repo.rs:1170`, at
  340439a): if that pop conflicts with what the checkout left, the changes stay in Stashes and the notification
  shows only the switch's error. Fix: return the pop's failure with the switch's, and say "Your changes are in
  Stashes".
- [ ] **A region choice checks only its two parts.** `resolve_region`
  (`crates/templum/src/workspace/git_conflicts.rs:79`, at 340439a) compares the parts' lines with what was drawn, not
  the `<<<<<<<` and `>>>>>>>` lines: a region whose labels changed (another merge started meanwhile with the same
  text) is still resolved. Fix: carry the labels in `shown` and compare them too.
- [ ] **A comparison's binary file reads "Binary file · 0 B".** The size comes from the working file
  (`crates/templum/src/git_block/diff_view/paint.rs:515`, at 340439a), which a range doesn't read. Fix: leave the size
  out when `bytes` is None.

- [ ] **Publish is dropped without a word after a workspace switch.** Its remotes read waits in the Git's queue;
  when it comes back with another workspace on screen, `publish`'s early `return`
  (`crates/templum/src/workspace/git_sync.rs:174`, at 150c656) drops it. Fix: let `publish_to` and `git_sync` run by
  `GitAt` off screen, or say it was dropped.
- [ ] **A conflict choice with no trash goes ahead without asking.** Without a data folder, or past the trash's cap,
  `resolve_region` and `resolve_file` (`crates/templum/src/workspace/git_conflicts.rs:88,141`, at 150c656) write
  the file with no copy and no question, where the trash rule (docs/specs/git.md, "When a copy can't be kept") asks
  first. Fix: ask as discards do ("This can't be undone").

## History

- [ ] **Load more in a file's history reads the earlier pages again.** `--skip` with `--follow` counts commits that
  don't touch the file (git 2.55), so `Repo::file_log` (`crates/templum-git/src/repo.rs`) reads the first
  `skip + count` entries and drops `skip` of them. A file with thousands of commits makes each Load more slower.
  Fix: remember where the last page ended (its commit and the path then) and go on from there with
  `git log --follow <hash>^ -- <path then>`.
- [ ] **File history and Blame are offered where git can't answer.** `menu/build.rs` leaves them out only for an
  untracked row: a staged new file (no history, "no such path in HEAD" for a blame), a file deleted in the working
  tree, and an untracked file in an editor's tree all end in an error under the top bar. Fix: leave the items out
  there, or blame those files at HEAD.
- [ ] **A Code search trims its text.** `run_search` (`crates/templum/src/workspace/git_history.rs`) trims the field,
  so `"fn "` searches `-Sfn`. Fix: trim only to decide whether the search is empty.
- [ ] **Two search-state slips.** `set_search_mode` runs the last searched text again, not the field's; and typing a
  character then deleting it (no search on) reads page 1 again, losing the pages Load more added. Fix: run with the
  field's text; skip a clear when nothing was searched.
- [ ] **In a file's history, a commit's other files look clickable and do nothing.** The panel holds that file's
  change alone, so `show_commit_file` finds no row for the others. Fix: dim them, or open that commit whole.
- [ ] **Restore this version… takes the old path's attributes.** `Repo::file_at` (`crates/templum-git/src/repo.rs`)
  runs `cat-file --filters <rev>:<then>`, so a rename that changed the extension gets the old name's line-ending
  rules. Fix: `--path=<now>`.
- [ ] **Restore this version… over a symbolic link, or a file past the trash's cap.** `restore_now` writes through
  a link at the path (the trash keeps only the link), and over 100 MB keeps no copy though the question said the file
  goes to Recently discarded. Fix: refuse a link; word the question from `trash::fits`, as discards do.
- [ ] **A restored file that was gone comes back without its executable bit.** `restore_now` writes plain bytes.
  Fix: read the mode from `git ls-tree` and set it.
- [ ] **History goes back to its first page after every action.** `refresh_lists_at` reads page 1 again (older than
  batch 5), so the pages Load more added go after a commit, a fetch or a switch. Fix: read as many commits as are
  loaded.

## Stashes

- [ ] **The stash shown keeps its index after a drop.** `stash_shown` is `stash@{N}`'s N (older than batch 5): after a
  drop in a terminal, N names the next stash while the panel still shows the old one. Fix: keep the stash's hash.
- [ ] **A stash's diff ignores `diff.srcPrefix` and `diff.dstPrefix`.** `Repo::stash_diff` resets `diff.noprefix` and
  `diff.mnemonicPrefix` (git 2.55 mangles `--src-prefix` with `--include-untracked`) but not the two prefix settings of
  git 2.45+. Fix: `-c diff.srcPrefix=a/ -c diff.dstPrefix=b/`, checked against the same mangling.

## Trash and discards

- [ ] **Discard all filters its files with `Vec::contains`.** `discard` (`crates/templum/src/workspace/git_discard.rs:69`)
  is O(n·m) on the UI thread, slow for thousands of files. Fix: a `HashSet` of the paths.
- [ ] **Two trash corner cases.** The counter in a discard's id isn't zero-padded
  (`crates/templum-git/src/trash.rs:101`), so two discards in the same millisecond past the 9th can sort wrong; and an
  entry left half-made by a crash has no manifest, so `list` skips it and `prune` never deletes it
  (`crates/templum-git/src/trash.rs:165`, `:239`). Fix: pad the counter (`{:06}`), and prune folders without a
  manifest older than a day.
- [ ] **A `resolve` through the API leaves the file conflicted.** `Op::Resolve` (`crates/templum-git/src/ops/run.rs`)
  only checks out a side, as the popup does before its Mark resolved; a token that repeats it makes a copy each time
  until the trash refuses at 50 for the repository. Fix: refuse a `resolve` of a file whose working bytes equal the
  side's already, with no copy.
- [ ] **Recently discarded doesn't name the files.** A row says "Discarded 7 files"
  (`crates/templum/src/git_block/discarded.rs:51`). Fix: the file names under it, folded past a few.

- [ ] **Two answers from batches 1 and 2 act on the Git of whichever workspace is shown.** Undo commit's
  "Revert {short}?" (`crates/templum/src/workspace/git_changes.rs`) and Restore's "changed since" question
  (`crates/templum/src/workspace/git_discard.rs`) call `git_action`/`restore_discarded`, which take the Git on
  screen: answered after a switch, they act on the other workspace's repository. Fix: through `show_git(&at, …)`, as
  the batches 3 and 4 questions do.

## Tests

- [ ] **A few tests still look at a glide mid-flight on the real clock.** `navigation.rs` (the tests around
  `restart_glide` at lines 231 and 312, and the two-notch zoom at line 770) and `editor.rs:437` set the glide's start
  and draw a frame in the same update, so the frame lands mid-glide only if it comes within about 110 ms of
  `Instant::now()`. The window is far narrower than the one `lod.rs` had (an update apart). `blocks.rs:99`
  (`shortcuts_open_and_close_terminals`, "the view glides" after a new terminal is shown) fails in full runs on the
  maintainer's machine (three of three on 2026-10-26, at 93cf3b5b too), and passes alone, even 20 times beside 12
  busy loops. A person sees nothing; a contributor may see CI fail once on a very busy
  runner. Fix: draw again from the same
  camera while the frame landed past the glide, as `zoom_mid_glide` in `lod.rs` does.
- [ ] **`the_welcome_opened_again_shows_the_system_and_changes_only_what_was_chosen` failed once in a full run.**
  (`crates/templum/src/workspace/tests/welcome.rs`, seen by the orchestrator under load; not reproduced in 30 runs of
  `welcome:: themes::` with 16 busy loops beside, and the failing assertion wasn't kept.) The welcome is not a dialog, so
  it isn't the slide-in clock that made the checklist and gallery tests flaky (fixed with `no_motion`). A person sees
  nothing; a contributor may see CI fail once. Fix: loop the test with busy loops beside, keep the panic, then make
  the test wait on the state it checks (`cx.run_until_parked()` after the autostart write), not on the clock.

## Release and benches

- [ ] **`--bench-git` measures only the default Git popup size.** The bench has no data folder, so the popup is
  760×540, but a person can now drag it to the whole window (`git_size`, `crates/templum/src/workspace/input.rs`):
  about 2.5× the diff rows on a 1440p window, and longer lines on each Split side. Fix: a bench variant, or a one-off
  run, with `git_size` set to the window's room, checking p95 <= 16.7 ms.
- [ ] **The CHANGELOG will list three features twice.** Batch 2 has a `feat` commit for the library and another for
  the UI of the same feature: 726079b with c7e13c6 (hunks), 1d5bc9a with cb26cfd (the trash), efdd911 with 6362120
  (commit all, undo commit). Fix: edit the generated section at the next release so each feature reads once.
  Batch 6's `fix(git)` commit after its review fixes Generate ✨ before any release: drop it from the section too.
- [ ] **templum.exe may run without a Windows manifest.** gpui-kit turns on gpui-fast's
  `windows-manifest-provided-by-host`, so GPUI embeds none, and Templum's resources (`crates/templum/icons/templum.rc`)
  carry only the icon. Without one Windows may treat Templum as not aware of each monitor's DPI and scale its window
  as a bitmap (blurry on a 150% display). Check on Windows: Task Manager ▸ Details ▸ the "DPI awareness" column. Fix:
  add `1 RT_MANIFEST "templum.manifest.xml"` to the `.rc`, with gpui-fast's `resources/windows/gpui.manifest.xml`
  (Apache-2.0) beside it.
- [ ] **A prerelease's PKGBUILD doesn't build.** cargo-packager writes the version as it is, so a `-rc.N` tag gives
  `pkgver=1.2.0-rc.1`, and makepkg refuses a `pkgver` with a hyphen. Seen
  with the PKGBUILD the release attaches (`[package.metadata.packager.pacman]` in `crates/templum/Cargo.toml`). Fix:
  in release.yml, after `cargo packager`, rewrite a prerelease's `pkgver` with `_` for `-` and set the source URL's
  version from `$GITHUB_REF_NAME` instead of `${pkgver}`.
- [ ] **install.ps1 answers a version with no release with PowerShell's own error.** `$env:TEMPLUM_VERSION = "9.9.9"`
  prints "Response status code does not indicate success: 404 (Not Found)." (`install.ps1`, the first
  `Invoke-WebRequest`), where install.sh says "couldn't download … of Templum 9.9.9". Fix: catch it around the
  download and throw "templum: no Templum $version at https://github.com/KingDasWinx/Templum/releases".
- [ ] **install.sh on macOS replaces a running Templum.app under it.** The `Darwin` branch deletes the bundle and
  copies the new one, and macOS can kill an app whose signed code changes while it runs. Fix: copy to
  `.Templum.app.update` beside it and swap the folders, as `templum-update`'s `apply.rs` does, or ask to quit
  Templum first when `pgrep -x templum` finds it.
- [ ] **install.sh in a Rosetta shell on Apple Silicon installs the Intel build.** `uname -m` says `x86_64` there,
  so the Mac gets `Templum_<v>_x64.dmg`, which runs translated, slower. Fix: take `aarch64` when
  `sysctl -n hw.optional.arm64` prints 1.

## Commit messages

- [ ] **An agent installed while Templum runs gets no ✨ until the config changes.** Whether its program is
  installed is read when the config changes (`Workspace::commit_agent`, `crates/templum/src/workspace/mod.rs`),
  not each frame. Fix: look again when the settings panel opens, or when the Git popup opens.
- [ ] **On Windows, Details may say "exit code 1" for any failing agent.** PowerShell's `-Command` reports 1, not
  the program's code, and a native program's standard input through it is unchecked (`login_shell`,
  `crates/templum-git/src/message.rs`). Fix: append `; exit $LASTEXITCODE`, and test Claude Code's preset on
  Windows.
- [ ] **A cancelled or timed-out agent may run on under fish or PowerShell.** They keep the command as a child,
  and only the shell is killed (`until_cancelled`, `crates/templum-git/src/repo.rs`); bash and zsh `exec` it. The
  agent then finishes on its own, using the person's account. Fix: kill the process group (needs a dependency or
  `unsafe`).
- [ ] **A login profile that prints something comes before Claude Code's answer.** The command runs through
  `$SHELL -l -c`, and Claude Code's preset answers on the standard output, where a profile's greeting lands too.
  Fix: drop what comes before the answer's first line when it matches the profile's output read once, or ask
  Claude Code for a file (it has no such flag today).
- [ ] **The Git pointed at another folder (or its workspace deleted) while Generate runs leaves its agent running to
  the timeout.** The answer is dropped (`generated`, `crates/templum/src/workspace/git_message.rs`), the agent isn't
  stopped. Fix: cancel `generating` in `set_git_folder` and when the workspace is deleted.
- [ ] **Clearing a repository's instructions leaves an empty `[git.repositories."…"]` line.** `remove_key` takes
  the key, not its table (`set_repo_instructions`). Fix: remove a table in `save_edits` when its last key goes.
- [ ] **A nested table's last key removed may move its comments too far.** `remove_key` looks for the next header
  among the top-level tables only (`crates/templum/src/config.rs`). Fix: walk every table for the next header.
- [ ] **Generate's answer can't be undone in the fields.** It replaces what they held at the click with
  `set_value`, which clears their undo history (`generated`, `crates/templum/src/workspace/git_message.rs`): a draft
  typed before clicking ✨ is gone. Fix: `replace_all`, which keeps it, on both fields.
- [ ] **With Amend, Generate describes only what is newly staged.** The request is the staged diff
  (`message::changes`), and the answer then replaces the whole amended commit's message. Fix: with Amend, diff the
  index against `HEAD~1` (the empty tree for the first commit), or hide ✨ while Amend is on.
- [ ] **Emptying Settings ▸ Git ▸ Prompt puts the default back at once.** Empty text removes the key and the next
  frame fills the area with `PROMPT`, the caret at its start (`set_commit_prompt`, `crates/templum/src/settings/git.rs`);
  select all and type still works. Fix: keep a draft while the area is empty, as Another model does.
- [ ] **A new file's diff for the request is built twice.** `message::changes` reads untracked files and builds
  their diff as `untracked_diff` (`crates/templum-git/src/repo.rs`) does. Fix: one helper both call.
- [ ] **The docs don't say all of it.** `docs/specs/agents.md` doesn't list `[agents]`'s `generate` and `models`, and
  the README doesn't say Codex's models come from its `~/.codex/models_cache.json` (else one is typed). Fix: a line
  in each.
- [ ] **Claude Code's provider set only in its settings isn't used by Generate.** Its preset runs with
  `--setting-sources=` (`preset`, `crates/templum/src/agents.rs`), so an `env` block in `~/.claude/settings.json`
  (Bedrock, Vertex, a proxy's `ANTHROPIC_BASE_URL`) isn't read, and Generate fails where Claude Code itself works.
  Fix: read that `env` and pass it to the command, or say so in the failure's Details when the answer is an auth
  error.
- [ ] **A saved Claude Code generate command keeps the old behavior.** Someone who edited the preset before it got
  `--tools= --setting-sources=` keeps a command that starts a whole session (hooks, tools) and may ask questions
  instead of answering. Fix: a note in Settings ▸ Agents when a saved command lacks them, or ↺ back to the preset.

## Far zoom (level of detail)

- [ ] **A floating label lets the pointer's hover through to the block under it.** Under 25% a label reads past its
  card's edge over its neighbours (`crates/templum/src/block.rs`, the frame at the end of `render`), but it doesn't
  block the mouse: the person who points at the part of a label over an earlier block sees that block lift and, at
  the Card level, its close button and resize strips appear under the label. Presses are right (the label takes the
  left and right ones, a middle one pans as anywhere). Fix: `block_mouse_except_scroll` on the floating label, with
  the card's middle-press pan and pinch on it too.

- [ ] **A soft-wrapped editor tab's bars keep the wrap of the zoom they were taken at.** An editor tab's wrap width
  keeps gpui-kit's fixed padding and line-number margin (`crates/templum/src/editor.rs`, `code` in `files`), so with
  Soft wrap on its words wrap a little differently at each zoom; far away its bars show the rows of its last Full
  frame, and back at 50% a word or two can move to the next row. Notes don't: their box makes up for those parts
  (`note.rs`, `editor_box`). Fix: give the editor tab's area the note's compensation (`FIXED_LEFT`, `FIXED_RIGHT`).
- [ ] **The wheel on the one unseen frame scrolls a far editor or note instead of the canvas.** When a far body has no
  word map, or a stale one (first drawn far away, resized, its file reloaded), it lays its editor out once at opacity
  0 (`crates/templum/src/silhouette.rs`, `body`), with its hitboxes: a wheel event on that frame scrolls the hidden
  text (gpui-base's editor takes the wheel and stops it) instead of moving the canvas, and the text shows scrolled
  when the block is next drawn in full. Fix: a capture-phase wheel listener on the unseen editor's wrapper that moves
  the canvas and stops the event before the editor sees it.
- [ ] **Ctrl+F under 25% opens a find field nobody sees.** A focused terminal under 25% is its card
  (`crates/templum/src/block.rs`, `render`: focus lifts only the Silhouette level), and its find field is drawn at the
  Full level only, so the field takes the keyboard and what you type goes nowhere visible until you zoom back in.
  Fix: let `Workspace::open_find` (`crates/templum/src/workspace/find.rs`) frame the terminal (`frame_block`) when its
  block is below the Silhouette level.
- [ ] **A block opened under 25% is opened as a card or a speck.** Ctrl+Shift+T at 4% puts a new terminal in the view
  and focuses it, but the camera stays at 4% (`Workspace::show`, `crates/templum/src/workspace/view.rs`, only zooms
  out), so it is a few pixels and its prompt can't be seen. Fix: `show` flies to at least 25% when the new block
  would be drawn below the Silhouette level.

## Selecting blocks

- [ ] **A pasted terminal starts a plain shell.** Paste drops a copied terminal's agent (`paste_blocks`,
  `crates/templum/src/workspace/selection.rs`): resuming it would put two agents on one conversation, so a copied
  Claude Code terminal pastes as a shell in its folder. Fix: type the agent's own command (not its resume command)
  into the pasted shell.
- [ ] **A title click that slips nudges the group.** A press on a selected block's title (`block.rs`, the title's
  `on_mouse_down` → `begin_group_move`) let go within the click slop moves the blocks by the pixels the pointer
  moved (`MoveGroup` has no threshold) and `end_placing` keeps a Ctrl+Z step for it; a body's or the box's click puts
  them back (`unplace()`, `workspace/input.rs` `release`). A person sees a selection shift by a pixel or two, and an
  undo step that does nothing visible. Fix: `Drag::Held` for a title press also remembers it is a title (or `release`
  asks `unplace()` whenever a `Held` is let go under the slop), with a test pressing a title, moving 2 px, and
  checking the rects and `undo`.

## Notes

- [ ] **A note block with more tabs than fit keeps the tab on screen out of view.** The strip scrolls sideways
  (`tab_strip`, `crates/templum/src/tab_strip.rs`) but nothing scrolls it to the lit tab, so opening a note from the
  Notes dialog into a full block shows no tab lit, and the + is out of reach until the strip is scrolled. Fix: keep a
  `ScrollHandle` for the strip and `scroll_to_item` the active tab when it changes.
- [ ] **Undoing a blank note's close brings back an empty block.** A blank note goes when its block closes
  (`note_closed`, `crates/templum/src/workspace/note_tabs.rs`), so Ctrl+Z after closing it, or Ctrl+Shift+Z after
  undoing a new note block, shows "No note open here". Fix: in `add_saved`, a note item whose notes are all gone opens
  a new blank note.
- [ ] **Two canvases naming one note both show it after a restart.** `note_open_at` looks in the workspaces already
  loaded (`self.spaces()`), but `load_spaces` keeps the ones it restored in a local list
  (`crates/templum/src/workspace/spaces.rs`), so a hand-edited file that names a note in two workspaces opens it twice,
  and the last tab typed in wins. Fix: look in that list too while loading.
- [ ] **A note block copied in another Templum pastes someone else's notes.** The clipboard names notes only by their
  ids (`Clip`, `paste_blocks` in `crates/templum/src/workspace/selection.rs`), and ids start at 1 in every data folder:
  copied in the installed Templum and pasted in `--dev` (or the other way), the block shows copies of whatever notes
  have those ids there, unrelated ones, or is empty when none has. Nothing is lost: only copies are made. Fix: put each
  note's text and color in the copied item, and let `paste_blocks` always make the copies from them, never by id.
- [ ] **A focused editor or note moved off screen keeps the keyboard until a click.** GPUI sends keys to the window's
  root when the focused element isn't drawn; `release_focus` runs only when a block is closed, a note's preview
  toggles or an editor's file changes (`crates/templum/src/workspace/blocks.rs`), not when a pan or zoom takes the
  block off screen. Fix: release the focus of a block that leaves the viewport, for every kind.

- [ ] **A note's name can't be renamed from the block's header.** A double click on the title frames the block
  (`title` in `crates/templum/src/block.rs`, `frame_block`), so renaming is on the tab (double click) and in the
  block's menu (Rename…, the right click's and the ⋯'s). Fix: for a note block, give the title's text its own double
  click that calls `rename_note_tab` for the tab on screen and stops the press, so the frame stays on the bar around.
- [ ] **`block::title` takes a `cx` nothing uses.** A note's title came from its editor's text, which needed it;
  now it comes from `Workspace::notes` (`crates/templum/src/block.rs`), and the parameter is `_cx`. Dropping it frees the
  same parameter in `about` (`workspace/bus.rs`), `tail` (`menu/mod.rs`) and `swap_overlay` (`workspace/render.rs`),
  and the closures' in three tests. Fix: remove them all.

## Checklists

- [ ] **A field left editing in a hidden workspace stays drawn without the keyboard.** Switching workspaces while a
  checklist's name or item is edited hides the field, and GPUI sends no blur for an element it doesn't draw, so
  `Checklist::editing` stays set (`crates/templum/src/workspace/checklist_blocks.rs`): back there, the field shows with
  no caret, and an item left empty in it stays until the field is clicked and left. Fix: `stop_editing` on every
  checklist block of the space being hidden, in `switch_to`.
- [ ] **A click on the bare canvas right after a field opens can leave it open.** A click on a block stops a
  checklist's field (`focus_block`), but the canvas relies on GPUI's blur, which reaches the field only once it has been
  drawn again since it took the keyboard (its caret blinks every 500 ms): a click on the empty canvas in that first
  half second leaves the field drawn until the next click (`crates/templum/src/workspace/checklist_blocks.rs`). Fix: stop
  the focused block's field wherever the canvas takes the keyboard (`space.focused = None`).
- [ ] **Undoing a blank checklist's close brings back an empty block.** As for notes: a blank checklist goes when its
  block closes (`drop_checklist_if_blank`), so Ctrl+Z after closing it shows "No checklist open here". Fix: in
  `add_saved`, a checklist item whose checklist is gone opens a new blank checklist.
- [ ] **Two canvases naming one checklist both show it after a restart.** As for notes: `checklist_open_at` looks only
  in the workspaces already loaded (`crates/templum/src/workspace/persist.rs`, `add_saved`), so a hand-edited file
  that names a checklist in two workspaces opens it in both. Fix: look in `load_spaces`' list too while loading.
- [ ] **A checklist block copied in another Templum pastes someone else's checklists.** As for notes: the clipboard
  names checklists only by their ids (`paste_blocks`, `crates/templum/src/workspace/selection.rs`), which start at 1
  in every data folder, so a paste in the other Templum (`--dev` beside the installed one) shows copies of unrelated
  checklists that have those ids there, or is empty when none has. Fix: put each checklist's name and items in the
  copied item, and let `paste_blocks` always make the copies from them, never by id.
- [ ] **Quitting while an empty item is edited keeps it as an empty row.** `on_release` and `on_app_quit` call
  `save_canvas_now` (`crates/templum/src/workspace/persist.rs`), and the 5-second `snapshot` writes too, without ending
  an open field: a new, still empty item is written to `checklists.json`, and after a restart nothing edits it, so it
  stays until its × is clicked. Fix: write (and compare with `checklists_saved`) a copy of `self.checklists` without
  the blank items an open field edits, in every workspace; the quit hook has no `window` for `stop_editing`.
- [ ] **Checklist code does per-row work every frame, and note tabs repeat code.** Nothing shows. `row`
  (`crates/templum/src/checklist.rs`) looks its block up in the canvas and computes the drag preview's width for every
  item; `read_checklist` (`crates/templum/src/workspace/content.rs`) looks the list up again after `wired_checklist`,
  behind an error branch it can't reach; `note_tabs.rs` places a block at the view's center twice and removes a tab
  and fixes `active` twice. Fix: the width computed once in `list_view`; `wired_checklist` returning the `&List`; one
  helper each in `note_tabs.rs`.
- [ ] **A right-click on a long checklist's scrollbar opens no menu.** While the scrollbar shows (pointer over its
  track, or scrolling), gpui-component's scrollbar stops every press on its 16-unit strip; `list_view`
  (`crates/templum/src/checklist.rs`) picks the block and pans for the middle button in the capture phase, but the
  block's menu opens from the block's own bubble-phase handler, so a right-click there does nothing. The strip also
  covers the right 4 units of a row's ×. Fix: open the block's menu for the right button in that capture handler when
  the press is on the strip, and leave a field's own right-click alone.

## Grid mode

- [ ] **Ctrl+Z in grid mode can take back a hidden block's change.** The canvas's undo steps (`workspace/history.rs`)
  also name blocks left on the canvas, which grid mode doesn't show, so Ctrl+Z may move one there and nothing visible
  happens. Fix: skip steps whose blocks are all hidden while in grid mode, keeping them for the canvas.
- [ ] **A tile smaller than its block's minimum cuts it.** Splitting never stops (`Tile::insert`,
  `crates/templum-canvas/src/tile.rs`), so with many tiles in a small window a terminal stays at its 20 × 4 cells and
  is cut off, a kanban block's body too. Fix: refuse a split that would make a tile smaller than its block's
  `min_size`, and say so.
- [ ] **Swapping and flipping tiles aren't Ctrl+Z steps.** `swap_tile`, the title drop and `flip_tile`
  (`crates/templum/src/workspace/tiling.rs`) change the tree without a history step. Fix: a `Change::Tiles` step
  holding the tree before.
- [ ] **Grid mode comes and goes without a glide, and a last closed tile vanishes at once.** `calm` clears the ghosts
  (`tiling.rs`). Fix: animate each block from its canvas rect to its tile and back, as camera flights do.
- [ ] **The title bar shows 100% in grid mode.** It reads the camera's zoom (`title_bar.rs`), which is grid mode's.
  Fix: show "Grid" there while a workspace is tiled.
- [ ] **On a mirror in grid mode, a block home adds stays unseen until the grid ends.** (`mirror_blocks`,
  `crates/templum/src/workspace/remote.rs`.) A block added at home (by its person or an agent there) is put on the
  mirror's canvas, which grid mode hides; locally a new block joins the grid, and one added from the mirror's tile
  menu does too (`focus_awaited`). Fix: in grid mode, have `mirror_blocks` join a new block of home's to the focused
  tile's half, as `Workspace::join_grid` does.
- [ ] **A pasted group comes into the grid one block at a time.** Each pasted block splits the one before
  (`added`, `crates/templum/src/workspace/blocks.rs`), so their arrangement on the canvas is lost. Fix: tile a
  pasted group as a dwindle of its own in the focused tile's half.

## Agent usage

- [ ] **A user's own status line on Windows runs under `cmd`.** The relay (`run`,
  `crates/templum-usage/src/claude.rs`) runs it with `cmd /C`; Claude Code may run status lines with Git Bash there,
  so a bash one prints an error instead of the user's line. Fix: check by hand which shell Claude Code uses on
  Windows and run the same.
- [ ] **A status line that refreshes on a timer refreshes less.** The settings file Templum writes
  (`settings_arg`, `claude.rs`) has no `refreshInterval`, so a user's status line that showed a clock updates only
  after each answer. Fix: copy the user's `refreshInterval` into it.
- [ ] **A wrapper as Claude Code's command still gets `--settings`.** `typed` (`crates/templum/src/workspace/usage.rs`)
  adds it to any command of the built-in Claude Code, so `claude; echo done` passes it to `echo`. Fix: add it only
  when the command's program is `claude`.
- [ ] **Two Codex resumed in one folder may swap sessions, outside Linux.** Without `/proc` (`files`,
  `crates/templum-usage/src/codex.rs`), a resumed session is matched by its folder and its file's last write, and a
  resumed one older than the scanned days isn't found: its meter shows the other's numbers, and on restore each block
  may get the other's conversation (or the folder's latest). Fix: ask the OS for the process's open files on macOS
  and Windows too.
- [ ] **A Codex session from another folder asks for a folder on restore.** `codex resume <id>` (`resume_id`,
  `crates/templum-terminal/src/agent.rs`) is typed in the folder the Codex process ran in; a session Codex started
  with `-C <dir>`, or one resumed from elsewhere, has its own folder, and Codex then shows its "Choose working
  directory" picker. Fix: save the session's `cwd` from its rollout and type `codex resume -C <cwd> <id>` when it
  differs, quoted as `typed_path` does.
- [ ] **Gemini, Grok and Pi come back on the session they were opened on.** Their id is read from the process's
  arguments (`session_in`, `crates/templum-terminal/src/agent.rs`); after `/clear`, `/new` or `/resume` inside the
  agent, the block still resumes the first one. Fix: read the current session from their files (Gemini's newest
  `chats/session-*.jsonl` for the folder, Grok's `~/.grok/sessions/<cwd>/`, Pi's `~/.pi/agent/sessions/--<cwd>--/`)
  and pick the one written since the process started.
- [ ] **A Gemini killed before its first message can't come back.** Killed with SIGKILL, Gemini leaves its empty
  session file; `session_kept` (`crates/templum-usage/src/lib.rs`) finds it, so restore types `gemini --resume <id>`,
  which Gemini refuses ("Invalid session identifier") and the block shows a shell. Fix: count the file's message lines
  too.
- [ ] **Codex's context reads higher than Codex's own.** `usage` (`codex.rs`) divides the last turn's input by the
  whole window; Codex leaves out a baseline it reserves. Fix: use Codex's formula once its source says it.
- [ ] **Limits of a session closed long ago still show.** The meter (`crates/templum/src/usage.rs`) shows a window
  until its reset time, even when the last session that read it closed hours ago and the user may have used it
  elsewhere. Fix: say "as of" with the read's time in the popover.
- [ ] **Claude Code's tokens in a terminal's usage tooltip are its context, not what it used.** `relayed`
  (`crates/templum-usage/src/claude.rs`) sums `context_window.total_input_tokens` and `total_output_tokens`, which
  batch 8's spike S8-1 found are its last call's (38279 = 10 + 10 + 38259 cached); the tooltip's "Tokens: 38k" reads
  as the session's. Fix: drop `tokens` for a relayed session (its cost is shown), or name it "Context".

## Terminal history

- [ ] **Keys that scroll the history don't show the scrollbar.** Only the wheel and Jump to the bottom do
  (`Workspace::scrolled_history`, `crates/templum/src/block.rs`); Previous prompt and Next prompt
  (`Terminal::jump_to_prompt`), Find going to a match and Shift+PageUp on a mirror (`remote_page`,
  `crates/templum/src/workspace/remote.rs`) move the view with the bar hidden, so only *N lines below* says it moved.
  Fix: call `scrolled_history` after each of them.
- [ ] **The terminal's scrollbar shows on hover where the system hides scrollbars until one scrolls.** gpui-kit's
  theme picks `ScrollbarMode::Scrolling` there (macOS's automatic scrollbars), and the kanban's and checklists' bars
  follow it, but `history_bar` (`crates/templum/src/block.rs`) shows the terminal's while the pointer is over its strip
  whatever the mode. Fix: read `Theme::global(cx).scrollbar.mode()` there and leave the hover out under `Scrolling`.

- [ ] **With Find open, the scrollbar's strip lies under the find field's right end.** The field sits at the text's
  top right (`find_bar`, `crates/templum/src/block.rs`) over the strip's top 38 pixels: the pointer there shows the bar
  under the field, and the strip's capture-phase press takes a right-click there for the block's menu. Fix: start the
  strip under the field while it is open.

## Programs in a terminal

- [ ] **The Templum Agent's builds still share Templum's cgroup.** Only terminals get a systemd scope
  (`move_to_scope`, `crates/templum-terminal/src/scope.rs`); the Templum Agent's engine (`crates/templum-agent/src/engine.rs`)
  and every command its tools run, and Generate ✨'s agent (`crates/templum-git/src/message.rs`), are Templum's
  children in its own cgroup, so a heavy build the Templum Agent starts can still get the whole window killed by
  systemd-oomd (card 173). Fix: make `move_to_scope` public and call it with the engine's pid after its spawn.
- [ ] **A `kill -9` of a shell reads as the system's doing.** A block whose shell ends by SIGKILL says it "was
  stopped by the system, most likely for using too much memory" (`Workspace::render`,
  `crates/templum/src/workspace/render.rs`; `Terminal::killed`), also when the person killed it from another
  terminal. Fix: keep the scope after it fails (`CollectMode=inactive`), read its `Result` (`oom-kill`) when the
  shell ends, then `ResetFailedUnit` it.
- [ ] **What a shell forks before its scope exists stays in Templum's cgroup.** The move happens about 2 ms after the
  shell starts (`move_to_scope`, `crates/templum-terminal/src/scope.rs`), so a daemon its startup files start in that
  window (an `ssh-agent`, a `direnv` hook) is out of the scope; an agent typed in starts later, inside. Fix: start the
  shell through a gate that execs it once the scope's job is done (as `systemd-run --scope` does with itself).

- [ ] **A shell only the login shell's PATH finds is forgotten on the first restore.** `found_shell`
  (`crates/templum/src/workspace/persist.rs`) looks a saved terminal's shell up in `Workspace::shells`, read before the
  login shell's PATH arrives (`read_shell_path`, a thread); `program_dirs`
  (`crates/templum-terminal/src/agent.rs`) covers Homebrew, Nix, MacPorts and `~/.cargo/bin`, but a shell elsewhere
  (a version manager's shim, a company's folder) started from the Dock or a desktop menu is missing, and the block
  reopens the default, its saved shell forgotten for good (saved without it at the next write). Fix: write the login
  PATH to a file in the data folder once it is read, and let `program_dirs` use that file until the fresh read arrives.
- [ ] **Programs never hear the focus come and go.** Claude Code and Codex turn on focus reporting (mode 1004), but
  no block sends `CSI I` / `CSI O` (`focus_block`, `crates/templum/src/workspace/blocks.rs`), so a program that
  waits for focus to redraw or to stop notifying doesn't know. Fix: send them to the terminal losing and the one
  getting the focus when its mode asks.
- [ ] **The kitty keyboard reports presses only.** `kitty_key` (`crates/templum-terminal/src/input.rs`) never sends
  repeat or release events nor a modifier key alone, and in every-key mode the arrows and F-keys keep their legacy
  bytes; a program that asked for them (a game, a key-chord editor) misses them. Fix: pass `is_held` and the
  workspace's key-up and modifiers-changed events to `Terminal::key`.
- [ ] **A program's drag loses Ctrl and Alt.** `drag_to` (`crates/templum/src/workspace/input.rs`) reports a held
  button's moves with no modifiers, since it only gets Shift. Fix: pass the move's `Modifiers` to `drag_to`.
- [ ] **The pointer stays a text cursor over a program that uses the mouse.** The body's `cursor_text`
  (`crates/templum/src/block.rs`) doesn't look at the mode, so htop's or Claude Code's buttons look like text to
  select. Fix: show the arrow when `modes().mouse`, read in the snapshot rather than under the lock while painting.
- [ ] **Templum doesn't see an agent that runs in WSL.** `templum_terminal::inspect` reads the Windows processes under a
  block's shell, and WSL 2's Linux processes aren't among them, so the snapshot (`Space::see`,
  `crates/templum/src/workspace/mod.rs`) drops the agent `start_agent` named: an agent set to WSL (Settings ▸ Agents ▸
  Shell), or opened while Settings ▸ Shell is WSL, has no status dot, closing its block doesn't warn, and a restart
  brings back a WSL shell without it; `spawn_agent` refuses it (`Workspace::spawn_problem`) since its prompt could
  never be typed. Fix: read the distribution's processes (`wsl.exe -e ps -o pid,args`, or
  `\\wsl.localhost\<distribution>\proc`) and match them to the block by the shell's session, then resume it with its
  resume command.
- [ ] **A long "Copied 12 lines" can be cut in a narrow terminal.** `copied_pill` (`crates/templum/src/block.rs`)
  grows the pill away from the nearer side of the body, which keeps a pill narrower than half the body inside it; in a
  terminal of about 20 columns, "Copied 12 lines" let go just right of the middle passes the left edge, which clips
  it. Fix: measure the label with `window.text_system()` and clamp `left` to the body's width minus it.
- [ ] **Near the top, the pointer's I-beam covers the top of "Copied" when zoomed out.** Below the pointer the pill
  sits `GAP × zoom` under it (`copied_pill`, `crates/templum/src/block.rs`), but the I-beam keeps its size: at 50%
  its lower half overlaps the pill's top. Fix: keep that gap at least 10 screen pixels.

## Updates

- [ ] **Templum doesn't open again after an MSI update.** `Ready::Installer` for the MSI
  (`crates/templum-update/src/apply.rs`) runs `msiexec /i <msi> /passive`, which doesn't start the app it installed;
  the person opens it from the Start menu. Fix: run msiexec through a PowerShell `Start-Process -Wait`, then start
  `C:\Program Files\Templum\templum.exe`.
- [ ] **macOS updates aren't checked by hand yet.** `copy_app` and the bundle swap (`apply.rs`) are tested on CI's
  macOS (a dmg made with `hdiutil create`), never on a real Mac with Templum in /Applications. An ad-hoc-signed app is
  a new app to macOS at each version, so permissions like Full Disk Access are asked again. Fix: update a real
  install, and sign the app with a Developer ID.
- [ ] **No prereleases.** `latest` (`crates/templum-update/src/fetch.rs`) asks for `releases/latest`, which skips
  `-rc` tags, so nobody can test a release candidate through the updater. Fix: a `[updates] channel = "beta"` that
  lists releases and takes the newest one, prereleases included.
- [ ] **A proxy with its own certificate authority fails.** ureq trusts webpki-roots (Mozilla's list), not the
  system's, so a company proxy that re-signs HTTPS makes every check fail with a certificate error. Fix: ureq's
  `platform-verifier` feature (`RootCerts::PlatformVerifier` in `agent`).
- [ ] **A stalled download waits up to 30 minutes.** `agent` (`fetch.rs`) has a limit for the whole download, not
  for a pause in it, so a connection that stops sending keeps the pill at its percent until then. Fix: read the body
  through a reader that fails after 60 s without bytes.
- [ ] **Nothing is signed by Templum.** A download is trusted because GitHub serves it over HTTPS with the SHA-256
  it computed (`download`, `fetch.rs`); someone who could publish a release could publish a bad one. Fix: sign the
  files in `release.yml` (minisign) and check the signature with a key built into Templum.
- [ ] **A failed swap is only written to stderr.** `finish_update` (`crates/templum/src/workspace/update.rs`) prints
  why the new file couldn't take the old one's place (a folder made read-only after the download); Templum then
  opens the old version, which downloads again and offers Restart again. Fix: keep the error in the data folder and
  tell it at the next start.
- [ ] **The check's steps aren't tested end to end.** `check_update` (`workspace/update.rs`) calls GitHub directly,
  so the headless tests set the states by hand and never go Checking → Downloading → Ready, nor the offline case
  that keeps "available". Fix: hand `check_update` the release source, a local server in tests.

## Workspaces

- [ ] **A note open in a stopped workspace can be opened elsewhere, and then leaves the stopped one.**
  `note_open_at` (`crates/templum/src/workspace/note_tabs.rs`) looks at live blocks only, so a stopped workspace's
  note tabs (in its saved canvas) don't count as open; opening one elsewhere, then starting the stopped workspace,
  drops that tab there (`add_saved`, `persist.rs`, keeps a note in one tab at most). Fix: look in a stopped space's
  `saved` items too, and show "Open in *name* (stopped)".
- [ ] **Stop doesn't reach programs inside WSL.** `tree` (`crates/templum-terminal/src/terminal.rs`) walks Windows
  processes below the shell; what runs in a WSL distribution under `wsl.exe` isn't one of them, so a server started
  there may outlive the hangup. Fix: run `wsl.exe --terminate` for a distribution only that workspace's terminals
  use, or kill the session through `wsl.exe -e pkill -s`.
- [ ] **Stop and Delete may stutter for a moment with many processes.** `tree_of`
  (`crates/templum/src/workspace/spaces.rs`) reads the whole process table on the UI thread (and on Unix asks each
  process its session), once per Stop or Delete. Fix: read the tree on the background executor, then drop the
  workspace's blocks when it is read.
- [ ] **A program started after Stop by one that ignores the hangup keeps running.** Stop reads the process tree
  once (`tree_of`, `crates/templum/src/workspace/spaces.rs`) and two seconds later kills only what it read; a
  program that ignores the hangup and starts another meanwhile leaves that one alive. Fix: at the kill, read the
  tree again and also kill what is still in the stopped shells' sessions (Unix).
- [ ] **The Home's Waiting for you tiles cut their words in a narrow window.** (`home.rs`, `tiles`: 2 columns under
  960 px.) Under about 560 px of column the 2 columns leave "automations went wr…" cut, which the Screens standard
  says never happens. Fix: one column when a tile's longest words don't fit in half the column.
- [ ] **The Edit… and New workspace dialog is gpui-kit's alert dialog, not a `popup.rs` modal.** (`space_dialog.rs`,
  `show`.) It has the kit's title and Cancel/Save (Create) foot, a column of stacked labelled fields instead of
  sections of labelled rows, and no full screen or resize edges, unlike the Git popup's standard (CLAUDE.md). Fix:
  build it from `popup::frame`, `header`, `section`, `row` and `foot` as the Automations editor is, keeping Enter,
  Esc and the name selected on open.
- [ ] **The Home's pointer presses were checked headlessly only.** (`home.rs`: the tiles, a card's ⋯, a recent
  folder, Open folder….) On a display only its keys and looks were checked (`--dev`, keys sent to the test window
  alone): no pointer tool was at hand. Fix: click each one on a display, light and dark, beside the Git popup.

## Welcome

- [ ] **The frosted canvas shows the blocks' shapes, not their text.** GPUI's scene has no backdrop filter, so
  `frost` (`crates/templum/src/welcome.rs`) draws each block as a blurred shadow of its shape. Fix: blur the real
  canvas when GPUI gains a backdrop blur, or draws a frame to a texture outside tests.

## Themes

- [ ] **The color picker shows an alpha slider that does nothing.** gpui-kit's popover always has it, and theme
  colors are opaque (`crates/templum/src/settings/colors.rs`). Fix: hide it when gpui-kit's `ColorPicker` gains an
  option for that, or draw our own sliders.
- [ ] **Switching light and dark may show the other theme as it was for half a second.** When its file was saved
  while it wasn't in use, `refresh_theme` (`crates/templum/src/workspace/preferences.rs`) applies the copy read
  before, and `poll_theme` applies the file at the next poll. Fix: read the picked theme's file in `refresh_theme`.
- [ ] **The gallery's cache keeps themes GitHub no longer lists.** `refresh` (`crates/templum/src/gallery.rs`) only
  adds files to `<data folder>/theme-gallery/`; a theme changed upstream leaves its old file there (a few hundred
  bytes each). Fix: after a refresh, delete the `.toml` files `index.json` doesn't name.

- [ ] **The gallery test failed once in a full run.** `the_gallery_previews_restores_and_installs`
  (`crates/templum/src/workspace/tests/themes.rs:661`) found "Catppuccin Latte" where it expected "Paper" after
  clicking "gallery-theme-Paper", in one run of `cargo test -p templum` (card 69's, under load); alone, three times, and in
  the next full run, it passed. Its neighbour `an_install_that_fails_puts_the_theme_back` (`themes.rs:817`, "not the
  preview") failed the same way once in card 115's first full run, and passed alone three times and in the next. Fix:
  find what the click raced with (a frame not rendered before the read) and wait for it, as the neighbouring tests do.
- [ ] **The X of a dialog reads less than `Theme::on_accent` counts on.** It picks the hover color over the theme's
  background (`crates/templum/src/theme.rs:155`), which is where a notification's X sits, but gpui-kit paints a
  dialog with `tokens.background`, which Templum sets to `ui.canvas`, 30% darker in dark themes: under the pointer
  the dialog's X reads 3.54:1 in Mocha (3.83 counted), 3.51 in One Dark (3.89), 2.42 in Apple Dark (2.61). A
  person's own theme may fall under 3:1. Fix: pick by the worse of the two hovers (over `palette.background` and over
  `ui.canvas`), and name both backgrounds in the doc comment and in `docs/specs/design-system.md`.
- [ ] **In Apple Dark, the X of a dialog or a notification reads 2.4 to 2.6:1 under the pointer.** gpui-kit fills a ghost
  button's hover, in dark mode, with the accent at 50% over the background and draws its icon in `accent_foreground`
  (`Theme::on_accent`, `crates/templum/src/theme.rs`), the one color a selected row uses too. Apple Dark's blue is
  too bright for the light text its hover needs (white is 8:1 on the hover, but 3.65:1 on the full accent) and too
  dark for the dark text the full accent needs (black is 5.8:1 on it, the pick, and 2.6:1 on the hover). Every other
  built-in theme has 3.8:1 or more. Fix: a darker accent in `themes/apple-dark.toml` (white then reads on both), or
  ghost buttons of our own that don't share `accent_foreground` with the selected rows.

## Settings panel

- [ ] **The tabs wrap to three rows.** Since Canvas joined, the twelve pills (`Section::ALL`,
  `crates/templum/src/settings/mod.rs`) don't fit in two rows of the 480 px panel: a third row holds the last one or
  two, and every section starts lower. Fix: tighter pills (less padding) or one row that scrolls sideways.
- [ ] **A shell installed while Templum runs isn't in the shells' menus yet.** `Workspace::shells` is read at start
  and when Settings or the welcome opens (`load_lists`, `crates/templum/src/workspace/preferences.rs`); until then the
  toolbar's Terminal and Add ▸ Terminal ▸ show the older list. Fix: read it again when one of those menus opens, with
  a seam that keeps the tests' made-up list.
- [ ] **No test covers how Settings ▸ Agents' rows are wired to an agent's shell.** `agent_row`
  (`crates/templum/src/settings/agents.rs:27-31`) and the account row (`settings/accounts.rs:163-167`) are only
  unit-tested through `description` and `with_shell`: if `agent_row` went back to `templum_terminal::installed(…)`, a
  WSL agent Windows lacks would read "Yours · not found …" again and no test would fail. Fix: in
  `an_agent_that_runs_in_wsl_counts_as_installed`, open Settings ▸ Agents and check the row reads
  "Yours · templum-no-such-agent · in WSL".
- [ ] **Settings opened right after Templum starts may call an agent "Not installed".** `find_programs`
  (`crates/templum/src/workspace/blocks.rs`) looks for each agent's program when the panel opens and when the agents
  change. Before the login shell's PATH arrives (`read_shell_path`, a second or so after start), an agent only that
  PATH has (a mise install folder) isn't found, and its row says "Not installed · …" until the panel opens again.
  Fix: remember the look was early (`shell_path_pending()`, as `shells_early` does) and look again at the first frame
  after the PATH arrives.
- [ ] **Two Settings rows still look at the disk every frame.** Settings ▸ Templum Agent looks for `claude` at each
  frame (`templum_terminal::installed("claude")`, `crates/templum/src/settings/templum_agent.rs:25`), and the
  accounts' PATH note canonicalizes every PATH folder at each frame while an account has a command (`on_path` in
  `path_note`, `crates/templum/src/settings/accounts.rs:101`, also drawn in the account dialog). A person sees
  nothing on a local disk, but each frame there touches it (a slow or network folder on PATH stalls it). Fix: read
  both when the panel opens and when the agents change, as `find_programs` does, and keep the answers on the
  workspace.
- [ ] **The agent dialog's WSL note repeats `Workspace::in_wsl`.** The note under the Shell list
  (`crates/templum/src/settings/agents.rs:267-274`) decides WSL on its own, beside `in_wsl`
  (`crates/templum/src/workspace/blocks.rs:389-394`): if one changes (how a label not found falls back), the note says
  WSL while the agent opens elsewhere, or the other way round. Fix: one `Workspace` helper taking a label and whether
  the entry is an account, called by both.

- [ ] **macOS: an entry switched off in System Settings ▸ General ▸ Login Items still reads on.** `is_on`
  (`crates/templum/src/autostart.rs`) only checks that the LaunchAgent's file exists; the switch macOS keeps for a
  background item lives in launchd's database. A person sees the toggle on while the system won't start Templum.
  Fix: ask `launchctl print-disabled gui/<uid>` for the label and read it as off.
- [ ] **Windows: `reg.exe`'s output is read as UTF-8, but it prints the OEM code page.** `reg`/`reg_ok`
  (`crates/templum/src/autostart.rs`) decode stdout and stderr with `from_utf8_lossy`. Two things show: a program path
  with letters outside the OEM code page reads back garbled, so `refresh` takes the entry's file for gone and writes it
  again at each start (invisible: one `reg add` per start), and on a localized Windows the reason a change failed
  (`reg_ok`'s first stderr line, shown on the row and in the notification) has broken letters. Fix: decode with the
  OEM code page (`GetOEMCP` with `encoding_rs`, which is already in the lock file), or run `reg` through `chcp 65001`.
- [ ] **Windows: uninstalling Templum leaves its `Run` value.** The NSIS and MSI uninstallers
  (`[package.metadata.packager]` in `crates/templum/Cargo.toml`) know nothing of the `Templum` value `autostart.rs`
  writes under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (nor `Templum (dev)`), so after uninstalling,
  Windows still tries to start a program that is gone at each login, and the startup apps list keeps a dead entry.
  Fix: delete the value from both uninstallers (NSIS: `DeleteRegValue` in a custom template or hook; MSI: a
  `RegistryValue` with a remove action), and check both on a Windows machine.
- [ ] **Linux: a program path with a line break or bytes that aren't UTF-8 breaks the `.desktop` file.**
  `exec_line` and `desktop_entry` (`crates/templum/src/autostart.rs`) write `launch.program.to_string_lossy()`: a
  line break ends the `Exec=` line early and a lossy `�` names a file that doesn't exist, so the entry starts
  nothing (or something else) while the switch shows on. Rare: it takes such a name in the AppImage's path. Fix:
  refuse it in `set(true)` ("The path of Templum has a character a startup entry can't hold").
- [ ] **macOS: a Templum on another volume is taken for the disk image.** `Install::detect`
  (`crates/templum-update/src/install.rs`) takes any app under `/Volumes/` for a mounted image, so a `Templum.app` on
  an external disk or a second partition is `Manual(NotInstalled)`: `set(true)` in `crates/templum/src/autostart.rs`
  answers "Move Templum to Applications first", and the updater won't update it either. Fix: refuse only a mounted
  disk image or a read-only volume (`hdiutil info`, or `statfs` with `MNT_RDONLY`), not any `/Volumes/` path.
- [ ] **The startup rewrite can undo a switch-off made while it runs.** `refresh_autostart`
  (`crates/templum/src/workspace/autostart.rs`) reads the entry, then writes it on, and isn't counted as a change:
  switching off in Settings ▸ General between the two puts the entry back, and the switch shows off while the system
  starts Templum. Only when Templum was moved with the entry on, and within the rewrite (a millisecond on Linux, a few
  `reg.exe` runs on Windows). Fix: count the rewrite as a change (`changing`, `changes += 1`), so the switch stays
  disabled until it ends.
- [ ] **A Linux entry in Flatpak or Snap would be written inside the sandbox.** Neither is a way Templum is
  packaged today, but `XDG_CONFIG_HOME` there is the sandbox's, not the desktop's. Fix: ask the Background portal
  (`org.freedesktop.portal.Background.RequestBackground`) when `FLATPAK_ID` is set.

## GPUI engine (gpui-fast)

- [ ] **A GPU that never finishes a frame stops the UI thread for good; the watchdog restarts Templum instead of
  recovering in place** (kanban card 291). gpui-fast-wgpu 0.1.8's `surface.configure` (`wgpu_renderer.rs:1052`,
  `1177`, `1181`), its resize wait (`device.poll` with no timeout, `wgpu_renderer.rs:1031`) and `destroy` (`2183`, on
  close) reach wgpu-core 29.0.4's `configure_surface` (`maintain(PollType::wait_indefinitely())`,
  `device/resource.rs:5137`) and wgpu-hal's `vkDeviceWaitIdle` (`vulkan/swapchain/native.rs:350`), none with a
  timeout. A person sees the window frozen for 30 seconds after a sleep, then Templum restarting: the agents resume,
  but commands running in terminals end. Fix: upstream, bounded waits there that report the device lost, so
  gpui-fast's own recovery (`renderer.recover`, `linux/wayland/window.rs:2032`) recreates the renderer in place.
- [ ] **Windows has no watchdog.** wgpu waits the same way there (`crates/templum/src/watchdog.rs` is Linux only),
  so a GPU that never answers would freeze the window until Templum is ended in the Task Manager. Fix: the same
  watchdog, ending the process with `TerminateProcess` instead of `_exit`.

- [ ] **A note tab's text is saved without telling the window.** The editor subscription in `note_tab`
  (`crates/templum/src/workspace/note_tabs.rs`) writes `ws.notes` without `cx.notify()`. Today the tab still follows
  typing because `Workspace`'s render reads the note's editor; if that read goes away, gpui-fast reuses the old frame
  and the tab looks stale until something else redraws. Fix: `cx.notify()` after `set_text`.
- [ ] **Zooming a note is 5% slower on gpui-fast.** `--bench-zoom=note` p95 51.6 → 54.2 ms (ADR-0003); a person sees
  the same stutter as before, slightly longer. Fix: the note re-wrap work already owed (hold the text size while the
  zoom glides, or the upstream gpui-kit fix), which removes the cost under both engines.
- [ ] **The diff list never gets gpui-fast's scroll cache.** `top_file`
  (`crates/templum/src/git_block/diff_view/mod.rs:327`) reads the list's scroll offset while `Workspace` renders, and
  gpui-fast repaints every scrolled frame of a list whose offset `render` read. A person sees nothing; scrolling a long
  diff costs what it did before. Fix: pick the sticky header's file in a `canvas` prepaint instead of in `render`.

## Kanban

- [ ] **The card's dialog isn't built on the Popups standard.** Seen with card 258. `open_draft`
  (`crates/templum/src/kanban.rs`) opens gpui-kit's `Dialog`, centered a tenth down the window, with a header and a
  footer of its own: no tile, title and subtitle, no Full screen or Close, no resize edges. A person meets a window
  of another kind than Git, Requests, Agents and Automations, can't make the card larger than 600 wide, and closes it
  only with Done, Esc or a press outside. Fix: draw it in the dialog place card 280 made (`popup::dialog`,
  `dialog_frame`, `dialog_header`: the kanban tile, "Card #40", "In Doing on Board 3", Close) and `foot` (Delete card,
  Done), its body as it is, keeping its ids; the design system's Popups section, A dialog, says how. Its width (the
  card wants more than 520) and Full screen are that card's to decide.
- [ ] **The description's grip dragged below the card dialog's footer goes under it.** The dialog's body (gpui-kit's)
  doesn't scroll while the grip is dragged (`description_field`, `crates/templum/src/kanban.rs`): a field low in the
  body can be made taller than what shows, the grip and the field's bottom out of sight until the person lets go and
  scrolls. Fix: scroll the body by what the field's bottom passes its edge during the drag (the body's scroll handle
  is the kit's, so it needs one of ours around the body), with a test.
- [ ] **Ctrl+Z of a Done on a reopened draft loses the draft's content.** `Boards::finish_draft`
  (`crates/templum/src/board.rs`) drops the draft inside the undo step, and `undo_boards`
  (`crates/templum/src/workspace/blocks.rs`) keeps only the drafts kept now, so the card goes back as it was and the
  typed work is gone. A person who presses Ctrl+Z after Done to get their draft back finds nothing. Fix: on undo,
  also put back the drafts that `before` has and the current boards lack, unless the same card or column has a new
  one, with a test.
- [ ] **Unsent text in "Add an item" or the comment field doesn't count as a change.** `Work::changed`
  (`crates/templum/src/kanban.rs`) compares the cards only: Esc closes at once over a half-typed item or comment,
  and Save draft wouldn't keep it. A person loses the line they were typing. Fix: a non-empty field counts as a
  change, and Save and Save draft take it in first (the item as a checklist item, the comment as a comment), with a
  test.
- [ ] **A comment edit in a card's dialog is a change when nothing changed, and is lost when the window closes.** On
  a new card, a comment's pencil then Esc asks "Save your changes?" though the text is the same (`pending` in the
  dialog's `on_cancel`, `crates/templum/src/kanban.rs`); and `Work::keep` (same file), run when the window closes,
  ignores a comment edit in progress, so its text is neither kept in the draft nor posted. Fix: count the edit only
  when its trimmed text differs from that comment's, and have `Work::keep` take it in the same way, with a test.
- [ ] **Done on a card deleted meanwhile loses the person's work without saying so.** `Boards::finish_draft`
  (`crates/templum/src/board.rs`) finds nothing to change and the dialog closes; a new card whose column was deleted
  is lost the same way. Fix: keep the draft and say "That card is gone" in a notification, with a test.
- [ ] **Discard in the Save question also drops the draft the person had kept.** After reopening a draft, typing more
  and choosing Discard, the earlier draft goes too (`ask_to_leave`, `crates/templum/src/kanban.rs`); there is no way to
  drop only the newest typing. A person who wanted to keep the old draft loses it. Fix: Discard reverts to the kept
  state when the dialog was opened from a draft, and the footer's button drops the draft, with a test.
- [ ] **A draft is dropped when its card or column is deleted, even if Ctrl+Z brings the card back.**
  `Boards::prune_drafts` runs after every `edit_boards` (`crates/templum/src/board.rs`) and Ctrl+Z keeps the current
  drafts, so the restored card has no draft. A person who deleted a card with a draft and undid it loses the draft.
  Fix: keep pruned drafts in the undo snapshot and restore the ones whose card is back, with a test.
- [ ] **Esc or Enter in the title saving a comment edit's draft is untested, and Esc is unspecified.** The dialog's
  `on_cancel` and `on_ok` (`crates/templum/src/kanban.rs`) save the edit's draft when the card dialog closes by Escape
  from another field or Enter in the title; `docs/specs/kanban.md` lists only "Done, Enter in the title, a press
  outside it", and only the press outside has a test (`a_press_outside_the_open_card_asks_and_save_keeps_what_was_typed`;
  the others use Done and `window.close_dialog`). Fix: add "Escape outside the edit's field" to the spec, and a test
  that types a draft, focuses the title, presses Escape (and Enter) and checks the saved text.
- [ ] **A press outside the open card with its assignee menu open closes the card too.** The menu's own outside press
  (gpui-kit's `PopupMenu`) closes it, and the same press reaches the dialog's overlay (`open_card` in
  `crates/templum/src/kanban.rs`, card 116). A person who only meant to close the menu sees the card close, or, with
  changes, the Save question open. Fix: veto the close in the dialog's `on_cancel` (a press on the overlay asks it
  first) when the menu was open at the press (the dropdown's `on_open_change` kept in a `Cell`), with a test.
- [ ] **Two deleted comments undone in the order they were deleted come back in the wrong order.** `deleted` keeps
  each comment's index at deletion (`crates/templum/src/kanban.rs`) and `restore_comment` (`board.rs`) inserts at it:
  delete B then C from [A,B,C], Undo B then C, and the thread reads [A,C,B]; the same when an agent removes an earlier
  comment meanwhile. A person sees a later comment above an earlier one. Fix: insert by id order (ids grow with time),
  before the first comment whose id is greater, draw the Undo rows the same way, and correct `docs/specs/kanban.md`'s
  "at its own place".
- [ ] **Ctrl+Z in an open card's dialog doesn't undo an item's ✕, a label or a priority.** A deleted comment has an
  Undo row in its place (`removed_comment`, `crates/templum/src/kanban.rs`), but the dialog's other changes, like an
  item's ✕ or a label, are steps of the kanban's Ctrl+Z, and with the dialog open the key goes to the field that has
  the keyboard; closed, it undoes the whole boards' snapshot, every later change of the agents and the person with
  it. A person who removes the wrong item may not know. Fix: let the dialog's Ctrl+Z undo the board when no text
  field has the keyboard, or give an item's ✕ the comment's Undo row.
- [ ] **The comment buttons and the editor aren't seen in a window.** Card 83's pencil and trash on hover and the
  editing box (`crates/templum/src/kanban.rs`, `comment_view`, `comment_action`) were laid out and tested headless
  only. A person may meet buttons that crowd a long author name or an editor too tall at 8 rows. Fix: run
  `cargo run -p templum -- --dev`, open a card with comments, and correct what looks wrong.
- [ ] **A comment can be edited or deleted only with the mouse.** The pencil and the trash of a comment in the card's
  dialog (`comment-edit-{id}`, `comment-delete-{id}`, `comment_action`, `crates/templum/src/kanban.rs`) show on hover
  and take no focus, so a person at the keyboard can't reach them (Tab skips them). Fix: let Tab reach them (a
  `focusable` element that is shown while it has focus), or open a comment's edit from Enter on it.
- [ ] **A short card list scrolls from anywhere but its exact middle while a card is dragged over it.** A list
  shorter than its two edge zones (80 world units: a block resized near its smallest whose cards overflow) is in both
  zones at once, and `edge_scroll` gives (2 × at − length) / 40 × 900: 10 units off its middle scrolls at 450 a
  second (`drag_speed`, `crates/templum/src/kanban.rs`). Fix: when the length is under three zones, shrink the zone
  to a third of it, so the middle third stays still.
- [ ] **A column scrolled out of the board may scroll its cards while a card is dragged just past the block.** Its
  list is still laid out and hears every drag: a pointer within a zone past the block's right edge, level with that
  hidden list's top or bottom, scrolls it, and the column comes into view already scrolled (`on_edge`,
  `crates/templum/src/kanban.rs`). Fix: pass the columns row's visible bounds to the column lists' listeners and
  ignore a pointer outside them.
- [ ] **`edit_card` can answer "Card 40: ."** when its `add_items` holds only blank lines or bare markers (`"- "`):
  `parse_items` finds no item and nothing else was asked, so the answer lists nothing (`edit_card`,
  `crates/templum/src/workspace/content.rs`). Fix: refuse with the "needs something to change" sentence when nothing
  is left to do.
- [ ] **An agent refused a card for another agent can free it, then claim it.** `claim_card` refuses a card "for any
  Codex" to a Claude Code block, but its `edit_card` with `for: anyone` passes (the gate checks only a card another
  block holds, or a `for` naming the caller), and a second `claim_card` then takes it: the person sees a card they
  gave to Codex done by Claude (`edit_card`, `crates/templum/src/workspace/content.rs`, the `held || names_caller`
  check). Allowed on purpose for now, so an orchestrator can hand cards out again and Ctrl+Z undoes it, but it
  weakens decision 3 of kanban.md's version 2. Fix if wanted: refuse a `for` on a card meant for an agent the caller
  doesn't run, with `claim_card`'s "is for any …" sentence.
- [ ] **A drag move over a board asks for a redraw GPUI already makes.** `on_edge` calls `window.refresh()`, and GPUI
  refreshes on every mouse move during a drag (`crates/templum/src/kanban.rs`); nobody sees a difference. Fix: delete
  the call.
- [ ] **Two of the person's agents started through the same wrapper count as one program.** Left by the second review
  of card 40 (`runs`, `crates/templum/src/workspace/content.rs:130`): `[agents]` entries such as `codex-a = "npx …"`
  and `codex-b = "npx …"` (or `bunx`, `env`, `wsl`) share their first word, so a card "for any codex-a" can be
  claimed by a block running the other; built-in agents all differ. Fix: for a known wrapper, compare the next word.
- [ ] **A block under the sidebar can stop a board's auto-scroll.** Left by the second review of card 40
  (`block_under`, `crates/templum/src/workspace/input.rs:117`): it ignores the sidebar and other overlays, so dragging
  a card from a board at the left of the canvas over the sidebar, inside the board's one-zone margin, stops the scroll
  when a hidden block lies under the sidebar. Fix: return None when the pointer is outside the canvas area.
- [ ] **Ctrl+Z on a click in a card's dialog also undoes the typing after it.** Older than card 40, which adds more
  such clicks (`edit_typed`, `crates/templum/src/kanban.rs:748`): type in the open card's title, click a priority, an
  item or the assignee, type more, then Ctrl+Z: the later typing goes with the click. Fix: reset the `typed` flag when
  a click in the dialog makes its own undo step.
- [ ] **No headless test drops a card.** Found with card 78 (`crates/templum/src/workspace/tests/kanban.rs`): the
  tests drag a card (`hold_drag`, gpui-kit's `drag` and `drag_to`) and its `on_drag` starts, but no `on_drop` of a
  card or a column fires, at the commit before the column scrollbar too; gpui-kit's own `drag_to` test drops on a
  bare view, so something in the workspace's window (a layer over the blocks, or its mouse-up listener) swallows it
  in the harness. A change to dropping is checked by hand. Fix: find what takes the mouse-up (print in the card's
  `on_drop` and in `Workspace::release`), then add a test that drops a card on a card, on a column's end and on a
  long column's scrollbar strip.
- [ ] **A scrolled column shows other cards after a zoom.** Found by the review of card 78 (`column_view` and
  `board_view`, `crates/templum/src/kanban.rs:493`, and the columns' row at `:356`): `EdgeScroll`'s offset is in pixels
  and isn't rescaled when the zoom changes, as `keep_list_rows` does for `editor.rs`'s lists, so zooming a block whose
  column is scrolled moves its cards and the scrollbar's thumb, and breaks "zoom scales like a photo". Fix: keep the
  last frame's zoom in `EdgeScroll` and rescale the offset by the ratio.
- [ ] **A column's scroll resets when it isn't drawn.** Found by the review of card 78 (`column_view`,
  `crates/templum/src/kanban.rs:493`): the offset lives in `use_keyed_state`, which lasts only while the column is
  drawn, so switching the block's board, or the workspace, and back puts every column at its top. Fix: keep the offsets
  per (block, column) in the `Workspace`.
- [ ] **A right-click in the gap between two cards, on a long column's scrollbar strip, scrolls the column.** Left by
  the fix of the strip taking right and middle presses (`card_view`, `crates/templum/src/kanban.rs`): the card opens
  its menu from the capture phase, ahead of the scrollbar, but the 6 units between cards (and the list's padding) have
  no card to do it, so the scrollbar's bubble listener takes that right press as a jump. Without the scrollbar, the
  press would open the block's menu. Fix: open the block's menu from the list's capture handler when the pointer is
  over the strip and no card is (the cards as direct children of the scrolling box give their bounds through
  `ScrollHandle::bounds_for_item`).
- [ ] **The attachments' look and the drop and paste weren't seen in a window.** Card 152
  (`crates/templum/src/kanban.rs`, `open_draft`: the Attachments section, the ring on `card-body`) was built while the
  person slept and checked headless only. A person may meet thumbnails or rows that crowd a long name, a ring that is
  too faint in a theme, ten images that wrap badly, or a drop or a pasted screenshot
  (`grim -g "$(slurp)" - | wl-copy`) that behaves differently on Wayland, macOS or Windows. Fix: run
  `cargo run -p templum -- --dev` in light and dark themes, attach a 9 MB image, a PDF and a text file, drop from the
  file manager, paste a screenshot and copied text, and correct what looks wrong.
- [ ] **No headless test reads the face's attachment count.** The face (`meta_row`,
  `crates/templum/src/kanban.rs`) draws the paperclip and the count in a div the harness only finds by id, so
  `attach_shows_a_thumbnail_and_a_row_and_done_puts_them_on_the_card` asserts the element appears and goes, and the
  count is checked through the card's data. A wrong number on the face passes. Fix: give the div an accessibility
  label with the count (or read its text if gpui-kit gains a way) and assert "2".
- [ ] **The sweep at start reads and deletes on the UI thread.** `attachments::sweep`
  (`crates/templum/src/attachments.rs`, called from `load_spaces`) walks `<data>/attachments/` before any window can
  open, as the design chose; a data folder with thousands of attachments, on a slow disk, delays the first frame.
  Fix: sweep on the background executor after the first frame, with the boards cloned into the task.
- [ ] **A copy that fails after Save draft leaves a draft that is the card.** `land` (`crates/templum/src/kanban.rs`)
  returns on `Err` and does nothing else: the draft kept at Save draft holds no change but the copy that failed, so
  it equals its card and the face shows the draft pencil for nothing until someone opens and leaves the card. Fix: on
  `Err`, drop that card's kept draft when its `work` equals its `base` (ids aside), with a test.
- [ ] **A file copied while a draft is saved and reopened can be lost.** In `land` (`crates/templum/src/kanban.rs`,
  the `Kept` arm): Save draft while a file copies, reopen the card from the draft, press Done before the copy ends,
  and the copy finds no dialog and no draft (Done dropped it) and the card isn't its dialog's to take it: the file
  lands on neither the card nor a draft, and the next start sweeps it. Fix to consider: remember the last dialog's
  outcome per card on the workspace and follow it when no dialog and no draft is found, with a test.
- [ ] **On X11 a copy from an app that offers an image and text attaches the image.** gpui-fast-linux 0.1.8
  (`x11/clipboard.rs`, `get_any`) reads image formats before text, so copying cells from LibreOffice (which offers
  both) and pressing Ctrl+V in a card's field gives `ClipboardEntry::Image` and no text: `pasted_image`
  (`crates/templum/src/kanban.rs`) attaches "Pasted image.png" instead of pasting the words. Wayland and the other
  platforms are not known to do it. Fix: upstream, or prefer text in that reader.

## Connections

Left by batch 1 (the bus) of `docs/specs/connections.md`; line numbers are those of `development` at 2c3c809.

- [ ] **An agent that can't be given connections says nothing.** `typed` (`crates/templum/src/workspace/usage.rs:33`)
  leaves the flags out when the data folder or Templum's path can't be typed safely (a `$`, a `"`), and the Claude
  Code or Codex then starts without Templum's server: `/mcp` doesn't list it, and nothing says why. Fix: a
  notification once per session, from `launch_agent` and `resume_agent`, which have the window.
- [ ] **Two Templums sharing a data folder share one `templum`.** `templum_folder`
  (`crates/templum/src/workspace/bus.rs`) points `<data>/bin/templum` (Windows: `<data>\bin\templum.exe`, a hard link
  or a copy) at the last Templum started (an installed one and `cargo run`), so a terminal of the first runs the
  second's program; its `TEMPLUM_BUS` still names its own Templum. On Windows the second may not be able to replace
  a `templum.exe` that is the first's running program, and its terminals then get no `templum` at all. Fix: one entry
  per bus (`<data>/bin/<pid>/templum`), removed with the bus.
- [ ] **The Claude Code settings file has its old name.** `claude-statusline.json`
  (`crates/templum-usage/src/claude.rs:23`) now holds the hooks too. A person reading `ps` sees a status-line file
  carry hooks. Fix: rename it `claude-settings.json` and delete the old one at start.
- [ ] **A stopped workspace's block ids aren't checked for a clash.** `new_uid`
  (`crates/templum/src/workspace/blocks.rs:68`) checks the running workspaces only; a new block may take an id a
  stopped workspace's saved canvas holds (1 in 60 million per pair), and that block gets a new id when the workspace
  starts again. Fix: check `Space::saved`'s items too.
- [ ] **The verbs print nothing in a Windows console.** The release program has no console
  (`crates/templum/src/main.rs:3`), so `templum blocks` (`main.rs:56`) prints only into a pipe:
  `templum blocks | more`. Fix: `AttachConsole(ATTACH_PARENT_PROCESS)` before running a verb (needs `unsafe` or a
  crate), or a small console `templum-cli.exe` beside it.
- [ ] **The bus spins when it can't accept.** The accept loop (`crates/templum-mcp/src/bus.rs:63`) goes straight
  back to `accept` after an error; one that repeats (out of file descriptors) keeps a core busy until it clears.
  A person sees Templum use a whole core. Fix: sleep 100 ms after a failed `accept`.
- [ ] **A socket in the temporary folder outlives a crash.** When the data folder's path is too long for a socket,
  the bus's socket goes to a private `templum-<random>/bus.sock` in the temporary folder
  (`crates/templum-mcp/src/bus.rs`, `listen`), and `remove_stale` only clears `<data>/bus`: a crash, or a Templum
  stopped with SIGTERM, leaves that folder and its socket file there (seen while taking the website's screenshots:
  several `/tmp/templum-<hex>/bus.sock` left behind). Fix: at start, run `remove_stale` on the user's own `templum-*` folders in the
  temporary folder, and remove each one left empty.
- [ ] **Reading a terminal's last command holds the terminal for its whole output.** `SharedTerm::last_command`
  (`crates/templum-terminal/src/terminal.rs:301`, at c0597d1) turns the whole command's output into text under the
  terminal's lock before `read_terminal` keeps its last 256 KB; a command that printed the whole history makes the
  terminal (and its painting) wait while it is read. Before that, `prompt_lines`
  (`crates/templum-terminal/src/tap.rs:70`, at c0597d1) walks every line of the history, cell by cell, under the same
  lock to find where the prompts start, so even a short last command costs a walk of the whole history (10,000 lines
  by default). Fix: read at most `MAX_LINES` plus a screen of rows above the end of the command, and find the last
  finished prompt by walking up from the bottom, stopping at the first.
- [ ] **`read_terminal` with `lines` may give fewer lines, the first a piece of one.** `SharedTerm::lines`
  (`crates/templum-terminal/src/terminal.rs:285`, at c0597d1) reads `n` plus a screen of grid rows and joins wrapped
  rows into one line, so with long wrapped lines it returns fewer than `n`, and the first may start in the middle of
  a line. Fix: read rows upward until `n` lines start in the range (a row that isn't the wrapped end of the one
  above).
- [ ] **A Bash command sent to the background ends a task handed to a working Claude Code.** `claude`
  (`crates/templum-mcp/src/hooks.rs:36`, at 0415ff5) takes a `PostToolUseFailure` with `is_interrupt` for Esc and
  ends the turn, but Claude Code (2.1.286) likely sends `is_interrupt` for any tool cut short, Ctrl+B moving a
  running Bash command to the background included, and the turn goes on. The dot comes back to Working within 3
  seconds (its screen moves, `crates/templum/src/workspace/activity.rs`), but a task another agent handed it ends at
  once with "Its turn ended without an answer". Fix: confirm it with the plan's manual check 14, saving both hook
  inputs; if a field tells Esc from Ctrl+B, end the turn on Esc's only; if none does, keep Working on an interrupt and
  let the screen going still end it.
- [ ] **Each hook runs the AppImage again.** The hooks, the status line and the MCP server run Templum's program,
  which is `$APPIMAGE` when Templum runs from one (`program`, `crates/templum/src/workspace/usage.rs:331`, at
  11a2fb0), and each run mounts the image; Claude Code waits for its `PostToolUse` hook after every tool. A person
  running the AppImage may see Claude Code pause after each tool. Fix: measure (the plan's manual check 13); if slow,
  give the short-lived hook and status-line commands the running executable (they only run while Templum and its
  mount do).
- [ ] **Windows terminals lose `templum` when its copy can't be replaced.** `templum_folder`
  (`crates/templum/src/workspace/bus.rs:89`, at 9aef302) gives up when `same_or_copy` fails to remove and remake
  `<data>\bin\templum.exe` (an older Templum still running it just after an update's restart, or a read-only file),
  and the terminals then get no `<data>\bin` on PATH, although a working older `templum.exe` is still there. A person
  sees `templum` not found. Fix: return the folder when `templum.exe` exists even if replacing it failed.
- [ ] **Some invisible characters still reach an agent in a title.** `clean_title`
  (`crates/templum/src/workspace/bus.rs:374`, at 9aef302) drops LRM and RLM but keeps ALM (U+061C), U+180E,
  U+206A–U+206F, U+FFF9–U+FFFB and the variation selectors (U+FE00–U+FE0F, U+E0100–U+E01EF), which can hide text as
  tag characters do. A person sees nothing; an agent reads the hidden text in `whoami` or `list_blocks` (the
  instructions escape it). Fix: add those ranges to the characters it drops.
- [ ] **No test accepts a hello split across reads.** `first_line` (`crates/templum-mcp/src/bus.rs:143`, at 9aef302)
  joins a hello that arrives in pieces, but only its refusals are tested; a change that broke the joining would
  pass. Fix: a test that writes the hello in two parts 300 ms apart and gets its ping answered.
- [ ] **Copying `templum.exe` to another drive delays Windows' start.** `same_or_copy`
  (`crates/templum/src/workspace/bus.rs:96`, at 9aef302) copies the whole program (tens of MB) on the UI thread when
  a hard link can't be made, once per version. A person sees the window come up later after an update. Fix: make the
  entry on a background thread; terminals opened meanwhile get PATH as soon as it is there.

Left by batch 2 (wires); line numbers are those of `development` at 825507d.

- [ ] **Ctrl+Z after Connect to ▸ in a terminal's menu reaches the terminal.** The right-click focuses the block
  (`crates/templum/src/block.rs:624-637`), and the menu's wiring (`wiring`, `crates/templum/src/menu/build.rs:170`)
  leaves it focused, so the Ctrl+Z that should take the wire back goes to the terminal; the Wire tool leaves no block
  focused and doesn't have this. A wire's own **Disconnect** (`Items::Wire`, `crates/templum/src/menu/build.rs:1007`,
  at f1e6811) has it too: the right-click on a wire keeps the focused block, and the menu gives it the keyboard back.
  A person presses Ctrl+Z and sees `^Z` in the shell (it can suspend the agent), and the wire stays gone. Fix:
  wiring or unwiring from any menu leaves no block focused, as `begin_wire` does.

Left by the review of batch 2; line numbers are those of `development` at 25f662e.

- [ ] **Connect to ▸ can list several blocks under one title.** The submenus list blocks by `block::title`
  (`crates/templum/src/menu/mod.rs:174`), so several plain shells, or several Claude Code terminals, read alike. A
  person can't tell which one an entry wires. Fix: list terminals as Send to ▸ does, with `label` (agent · title ·
  folder, `crates/templum/src/workspace/blocks.rs:763`).

Left by the second review of batch 2; line numbers are those of `development` at f1e6811.

- [ ] **A wire being drawn can stay on screen after its drag ended.** `end_drag`
  (`crates/templum/src/workspace/input.rs:432`) clears `Drag::Wire` without a redraw, and it is what a move with no
  button down (`crates/templum/src/workspace/render.rs:465`) and a middle-button release (`render.rs:482`) call. A
  person who middle-clicks while drawing a wire, or whose release Templum never got, sees the accent curve stay until
  something else redraws (the marquee's rectangle lingers the same way). Fix: redraw in `end_drag` when the drag it
  clears is a `Drag::Wire`.
- [ ] **Letting go of the Wire tool throws away any drag.** `end_wire` (`crates/templum/src/workspace/wires.rs:137`)
  takes whatever drag is under way, not only `Drag::Wire`; with the Wire tool on, an Alt-pan let go over the canvas
  loses its `Drag::Canvas`. Harmless today (`release` does nothing special for a pan), but the next drag that does
  would break. Fix: take the drag only when it is `Drag::Wire`.
- [ ] **A Wire-tool press on the empty canvas takes the keyboard.** `begin_wire`
  (`crates/templum/src/workspace/wires.rs:131`) sets `focused = None` even when the press is on no block, where the
  spec says nothing happens. A person with a terminal focused who picks the Wire tool and misses a block finds their
  typing no longer reaches the terminal. Fix: unfocus only when a drag starts (plan decision 6 reads it as it is,
  matching a click on the empty canvas with Select).
- [ ] **Entering grid mode's wire reset has no test of its own.** `calm()`
  (`crates/templum/src/workspace/tiling.rs:304`) clears `selected_wire`, but `grid_on` always ends in `focus_block`,
  which clears it too, so `entering_grid_mode_lets_the_wire_go` passes without that line. A person sees nothing. Fix:
  drop the line, or keep it as a defensive reset as `calm()` does for the selected drawing.

Left by batch 3 (agents together) and its review; line numbers are those of `development` at 6aa33f6.

- [ ] **The inbox's field misses Templum's paste shortcut.** `on_inbox_key`
  (`crates/templum/src/workspace/inbox.rs`) lets every command but Copy and Paste through, so Ctrl+Shift+V does
  nothing in a question's field; the find field's branch (`on_key_down`) handles it. Fix: the same case as the find
  field's.
- [ ] **The dot stops while it is off screen on a wire that is only partly in view.** A frame is asked for only when
  the dot's point is in view (`crates/templum/src/workspace/render.rs:447`): once it leaves the view it stays there
  until something else draws, then jumps. Fix: ask for frames while the wire's curve's bounds meet the view.
- [ ] **A spawn in an untouched workspace lets the camera move.** `open_agent`
  (`crates/templum/src/workspace/spawn.rs:106`) doesn't clear `space.pristine`, as `launch_agent` does, so in a
  workspace no one has touched yet the next frame centres the camera on all the blocks. A person sees the view move
  when an agent spawns another. Fix: clear `pristine`.
- [ ] **A spawn making a worktree can't be withdrawn.** Its call is a job (`Outcome::Later`,
  `crates/templum/src/workspace/bus.rs:167`), not a wait: a cancel or the agent quitting doesn't stop it, and a
  `git worktree add` that hangs (a post-checkout hook) holds the call for good. A person sees the agent's call never
  end. Fix: keep the job in `waits` with a deadline, or run git with a time limit.
- [ ] **Each wait keeps a timer until its deadline.** `wait` (`crates/templum/src/workspace/waits.rs:96`) starts a
  detached timer per call and leaves it to run out after the call is answered: up to an hour each. A person sees
  nothing (a small cost). Fix: keep the timer's task in the `Wait`, so answering drops it.

Left by the fourth review of batch 3; line numbers are those of `development` at 242654d.

- [ ] **A spawned agent that never started is said to have ended.** `agent_left`
  (`crates/templum/src/workspace/delegate.rs:307`) answers every wait with "Its agent ended before it answered.",
  also for an agent spawned with a prompt that no snapshot found within a minute (its folder's trust declined at
  once, or a question of its shell swallowed the command). The spawner may tell the person the helper crashed, or
  spawn another. Fix: `see` reports that case apart, and its waits get "{uid}'s agent never started: look at its
  block."
- [ ] **A spawn without a prompt can be told "runs no agent" while its shell starts.** `see`
  (`crates/templum/src/workspace/mod.rs:668`) keeps the agent Templum typed only while a prompt waits; a snapshot
  taken while a slow shell (ble.sh) starts clears `seen.agent` for a spawn without one (or a menu launch). A
  `delegate` right after the spawn gets "still starting", and a retry before the next snapshot "runs no agent:
  send_to_terminal types into a terminal", whose Enter could answer Claude Code's trust question. It heals at the
  next snapshot (5 s). Fix: keep `seen.agent` while `pid` is None for `STARTS_WITHIN` after Templum typed any
  agent (a "typed at" time on `Pane`).

Left by batch 4 (content); line numbers are those of `development` at 9e33f19.

- [ ] **The first Ctrl+Z after an agent's change can take back the person's typing in a card.** `edit_typed`
  (`crates/templum/src/kanban.rs:567`) keeps the boards for undo only at the dialog's first change; when an agent
  changes the boards while the dialog is open, the person's typing after that lands in no step, and the kanban's
  Ctrl+Z (once the dialog is closed) takes it back together with the agent's change. A person sees the end of what
  they typed in a card go with an agent's move. Fix: keep a new step in `edit_typed` when `boards_undo` grew since the
  dialog's last one.
- [ ] **`open_file` reads the file twice.** `open_file_for` (`crates/templum/src/workspace/content.rs:420`) reads it
  with `read_file` to refuse what an editor can't show, then `open_tab` (`crates/templum/src/editor.rs:425`) reads it
  again, both on the main thread: up to 32 MB twice. A person sees a short stall when an agent opens a big file. Fix:
  refuse from the metadata and the first 8 KB only, or hand `open_tab` what was read.
- [ ] **`append_note` scrolls the note back to the person's caret.** `append_note`
  (`crates/templum/src/workspace/content.rs:384-386`, at 808544d) moves the caret to the end to `insert`, then puts it
  back with `set_selected_range`, whose `move_to` scrolls to it (gpui-base-0.7.3 `src/input/base/movement.rs:62-95`). A
  person who scrolled away from their caret to read the note sees the view jump back to it when an agent adds to the
  note. Fix: keep `scroll_offset()` before the moves and put it back with `set_scroll_offset` after them
  (gpui-base-0.7.3 `src/input/base/state.rs:3111-3118`), if the deferred offset wins over `move_to`'s scroll.
- [ ] **`open_file` may stall on Windows.** `open_file_for` (`crates/templum/src/workspace/content.rs:403`, at
  808544d) takes the caller's folder from `Body::folder` (`crates/templum/src/workspace/mod.rs:316`), that is
  `Terminal::cwd` → `inspect` (`crates/templum-terminal/src/terminal.rs:717`, `:384`), which on Windows reads every
  process's folder and command line, "slow enough to keep off the UI thread", on the UI thread at every call. A
  person on Windows sees Templum stop for a moment each time an agent opens a file. Fix: read the folder on the
  background executor (`Outcome::Later`), or use `seen.folder` (the last snapshot's) on Windows.
- [ ] **A file named in another case opens a second tab on Windows and macOS.** `open_file_for`
  (`crates/templum/src/workspace/content.rs:436`, at 6ab2c25) keeps the file name as the agent spelled it, and `open_tab`
  (`crates/templum/src/editor.rs:431`) finds a tab by an exact path. An agent opening `readme.md` while
  `README.md` is open gets a second tab of the same file, and so does the person clicking the tree's name after an
  agent's tab. Fix: keep the given name only when the path is itself a link
  (`symlink_metadata(..).file_type().is_symlink()`); otherwise take the name `canonicalize` returns.
- [ ] **A file in the folder above opens in an editor titled with a raw `..` path.** The fallback of
  `open_file_for` (`crates/templum/src/workspace/content.rs:457`, at 6ab2c25) opens a new editor on `given.parent()`;
  for `open_file ../a.rs` from `proj/web` that is `proj/web/..`, which has no file name, so the editor's title shows
  the whole path (`a.rs — /…/web/..`) instead of `proj`. Fix: use the canonical folder when `given.parent()` has no
  file name.

Left by batch 5; line numbers are those of `development` at f48c75c.

- [ ] **A Gemini CLI that hangs keeps Install spinning.** `gemini_run` (`crates/templum/src/workspace/usage.rs:335`)
  waits for `gemini extensions link` (or `uninstall`) with no deadline. A person sees the Install or Remove button of
  Settings ▸ Connections spin until Templum quits. Fix: kill it after 60 s, as `claude::poll`
  (`crates/templum-usage/src/claude.rs:404`) kills a `/usage` that takes over 30 s, and say so in red.
- [ ] **The Gemini row looks at the disk every frame.** `gemini_row` (`crates/templum/src/settings/connections.rs:95`)
  stats `~/.gemini/extensions/templum` at each frame while Settings ▸ Connections shows. A person sees nothing, but
  every frame there touches the disk (a slow or network home folder stalls it). Fix: check when the tab opens and
  after Install or Remove ends, and keep the answer on the workspace.
- [ ] **After Rename wire… Delete and Esc do nothing to the selected wire.** The dialog of Rename wire…
  (`crates/templum/src/menu/build.rs:1008-1020`), as the one of Rename board, leaves no element with the keyboard once
  it closes. A person who renamed a wire presses Delete or Esc and nothing happens until they click the canvas. Fix:
  focus the workspace (`window.focus(&ws.focus_handle, cx)`) in the dialog's apply and on cancel.
- [ ] **A wire's name comes back with a block that left and returned.** `move_blocks`
  (`crates/templum/src/workspace/spaces.rs:274`) leaves the labels of a moved block's wires in the workspace it left
  (`Space::labels`, keyed by block ids, `crates/templum/src/workspace/mod.rs:511`). A person who moves a block to
  another workspace, back again, and wires it to the same block sees the old wire's name come back. Fix: drop the
  labels whose blocks left in `move_blocks`.
- [ ] **A watched shell that exits in a hidden window keeps its `wait_for` waiting.** `look`
  (`crates/templum/src/workspace/waits.rs:493`) answers a closed block at once, but a block whose shell exited is only
  taken off while the window draws (`crates/templum/src/workspace/render.rs:300-309`), so `look` still finds it. A
  person whose `templum wait <block> --for command_finished` watches a terminal that exits while Templum is hidden sees
  it wait until the window shows or the timeout passes. Fix: treat `pane.term.exited()` as closed in `look`.

Left by batch 6; line numbers are those of `development` at 476b772.

- [ ] **Claude Code's `[1m]` models can't be asked for.** `Agent::launch_args`
  (`crates/templum-terminal/src/agent.rs:384`, `model_typable` at `:437`) takes letters, digits, `.-_:/` only, so
  `opus[1m]` (its 1M-token context) is refused: brackets need quoting in each shell. An agent asking for it is
  refused. Fix: quote the model per shell (`'…'` for POSIX shells and pwsh) and allow `[` `]`.
- [ ] **`close_block` closes a block the person is typing in, or took over.** `close_block`
  (`crates/templum/src/workspace/spawn.rs:193`) asks no one; Ctrl+Z brings the block back with a new shell, without
  what the person had typed and not sent. `opened_by` also stays after the spawned agent quits: a person who then runs
  something of their own there (a dev server, an editor, a `git rebase` half done) sees the opener's routine clean-up
  kill it, and Ctrl+Z bring back only a shell in its folder, not that program: once a snapshot (about every 5 s) has
  seen the agent gone, `seen.agent` is None and `Space::item_of` (`crates/templum/src/workspace/mod.rs`) saves no
  agent; only in those first seconds does Ctrl+Z start the agent again. Fix: refuse while the person typed into it in
  the last few seconds (`Pane::keyed`, batch 7's), and while a program other than the agent is in
  front (`processes().foreground` neither the shell nor `seen.pid`), or clear `opened_by` once the person types there
  after its agent quit.
- [ ] **A wrapper set as Claude Code's or Codex's command gets their model flags.** `AgentEntry::launch_args`
  (`crates/templum/src/agents.rs:80`) gives a model and an effort to Templum's agents whatever their command, where
  `start_command` (`:50`) adds a session id only when the command runs the agent's own program. A person whose
  `[agents] claude = { command = "my-wrapper" }` doesn't pass flags on sees it fail when an agent spawns it with a
  model. Fix: refuse them when `program()` isn't the agent's, as `start_command` does.
- [ ] **Codex's `ultra` is typed for any model.** Codex 0.154 lists `ultra` only for gpt-6-astra, gpt-5.6-sol and
  gpt-5.6-terra (`codex debug models`), and `launch_args` (`crates/templum-terminal/src/agent.rs:384`) checks against
  Codex's whole list. A person sees Codex refuse or lower it for gpt-5.6-luna (manual check 1 says which). Fix: check
  the effort against the model's `supported_reasoning_levels` in `~/.codex/models_cache.json`, which `codex_models`
  (`crates/templum/src/agents.rs:185`) already reads.

Left by the review of batch 6; line numbers are those of `development` at dd11cad.

- [ ] **An interrupt between a task's paste and its Enter still sends the task.** `send_task`'s loop
  (`crates/templum/src/workspace/delegate.rs:352`) never checks that the task is still handed over. A person whose agent
  interrupts another one right after delegating sees Esc, then the Enter, and the agent start the task no one awaits.
  Fix: not "stop once `pane.handed` is None": the `Stopped` arm (`crates/templum/src/workspace/bus.rs:243`) ends the
  turn through `end_turn`, which clears `handed`, and `delegate` hands a task to an agent at work, so its previous
  turn's end landing in the wait would leave the task pasted with no Enter. Instead `interrupt` bumps a counter on the
  pane, `send_task` notes it when it pastes, and its loop skips the Enter, when `task` is true, only once the counter
  has changed.
- [ ] **Two `send_to_terminal … enter: true` in a row join into one command.** The `SendToTerminal` arm
  (`crates/templum/src/workspace/bus.rs:373`) types the text at once and its Enter `ENTER_AFTER` (150 ms) later; a
  second call to the same terminal that doesn't wait for the first's answer (an MCP client running tool calls in
  parallel, or two agents wired to the same shell) types its text in that wait. A person sees `cd /tmp` and `ls` run
  as `cd /tmpls`, then an empty line. Claude Code runs them one at a time, as no tool says `readOnlyHint`, and it was
  so before the Enter was answered. Fix: keep the pending send's `Task` on the `Pane` and type the next text once it
  has finished (a queue per terminal).
- [ ] **A third agent waiting on the interrupted one hears "Interrupted by you."** `interrupt` ends the turn through
  `end_turn` (`crates/templum/src/workspace/delegate.rs:615`), which answers every `Waiting::Turn` on that terminal. A
  person sees a third agent's `wait_for_idle` say it interrupted, when another did. Fix: give "Interrupted by you."
  to the waits from the caller, and "Interrupted by {signature}." to the others.
- [ ] **A spawn's prompt can't be interrupted before its agent shows.** `interrupt`
  (`crates/templum/src/workspace/delegate.rs:243`) refuses "runs no agent" (`:261`) while the spawned agent isn't in
  front of its shell yet, and once it shows its trust question the Esc answers that (Claude Code exits on it). A
  person sees the caller unable to call back a task it handed with its spawn. Fix: when it is the caller's task and
  `prompt` is still Some, end the task and drop the prompt without pressing Esc, before the "runs no agent" check.
- [ ] **Keys go on after the block is unwired mid-call.** `press_keys`
  (`crates/templum/src/workspace/delegate.rs:293`) checks only that the terminal is still there before each key. A
  person who unwires the block, or moves it to another workspace, during a call (up to about 750 ms) sees its last
  keys still arrive. Fix: run `caller` and `target` again before each key.
- [ ] **`close_block` offers to connect when connecting wouldn't help.** `close_block`
  (`crates/templum/src/workspace/spawn.rs:194`) checks the wire before who opened the block, so a block the caller
  neither opened nor is wired to gets "isn't connected to you. Ask the person with connect(…)". An agent that asks
  and gets the wire is then told only the person closes it. Fix: check `opened_by` first; one it opened but is no
  longer wired to still gets the connect hint (the spec's rule).
- [ ] **The "answered by" chip has no width limit.** The chip (`crates/templum/src/block.rs:183`) shows the whole
  signature; from a plain shell it is the terminal's title (up to 60 characters). A person sees it run over the
  block's header for 10 seconds. Fix: cap its width and truncate it as the title is.
- [ ] **An uppercase key under the kitty keyboard's every-key mode goes without Shift.** `named_key`
  (`crates/templum-terminal/src/input.rs:115`) types a character as its own key: with `REPORT_ALL_KEYS_AS_ESC`, `Y` is
  `ESC[89u` where a keyboard sends `ESC[121;2u`. Neither Claude Code nor Codex turns that mode on; a person sees a
  program that does take `Y` for another key. Fix: map an ASCII uppercase letter to its lowercase key with Shift,
  keeping the character.
- [ ] **A spawned agent closed before a snapshot found its session resumes the folder's latest.** `Space::item_of`
  (`crates/templum/src/workspace/mod.rs:596-611`) takes the session from `saved`, which a snapshot fills about every
  5 s; Claude Code and Codex are started without a session id (`new_id` is None for both,
  `crates/templum-terminal/src/agent.rs:39`, `:50`), so `start_agent` knows none. A person whose agent spawns another
  in its own folder, of its own kind, and closes it within those seconds (or one whose files give no session) sees
  Ctrl+Z type `claude --continue` or `codex resume --last` and bring back the caller's own conversation, two agents in
  one. Fix: start Claude Code with `--session-id {id}` (its `new_id`), keep on the pane the id `start_agent` gave, and
  use it in `item_of` when `saved` has none; for Codex, which takes none, start it fresh rather than `resume --last`
  when the block has `opened_by` and no session was found.
- [ ] **A few refusals and the hidden workspace aren't tested.** No test calls `interrupt` on a block that runs no
  agent or isn't a terminal, `send_keys` on one that isn't a terminal, nor `close_block` from a caller in a workspace
  not shown (`crates/templum/src/workspace/tests/delegate.rs`, `tests/spawn.rs`). A person sees nothing. Fix: add
  them.

Left by batch 7 and its review; line numbers are those of `development` at eb39454.

- [ ] **A task typed into a working Codex whose Enter joined the paste waits for its timeout.** `confirm`
  (`crates/templum/src/workspace/delegate.rs`) takes an agent at work as having queued the task, since Codex says
  it took it only when a turn does; if its Enter was lost, the text sits in its composer and the agent that handed
  it waits 600 s. Fix: when the turn it was on ends (`Stopped` with the task not taken, `bus.rs`), give it its
  3 seconds then.
- [ ] **The first task into a Codex that took no prompt yet isn't confirmed.** `Pane::speaks` is set by a hook, and a
  hooked Codex's first one comes with its first prompt (its `SessionStart` too), so `send_task` gives that task no
  confirmation and no second Enter. If its Enter joined the paste, the text sits in Codex's input box, its screen
  still, and the agent that handed it waits for the task's timeout (600 s by default). Fix: set `speaks` when
  `typed()` gave Codex its hooks and its person trusted them (none says so before a prompt: keep as is unless it
  bites).
- [ ] **A Codex whose person trusted only some of its hooks fails its tasks.** Codex's review trusts hook by hook;
  without `UserPromptSubmit`, `Pane::submitted` stays 0. Once any other hook but `Stop` and `Interrupt` spoke
  (`SessionStart`, `PostToolUse`, `PermissionRequest` set `Pane::speaks`), `confirm`
  (`crates/templum/src/workspace/delegate.rs`) finds no prompt taken 3 s after a task's Enter and a screen that
  changed ("Working (3s)"), and ends the task at once as "didn't take the task" while Codex does it. Before any spoke
  (its first task, or only `Stop` and `Interrupt` trusted) no confirmation comes: its `Stop` (`bus.rs`) finds the
  task not taken, answers nothing and rests it before its screen's looks can, so that task waits for its timeout
  (with `Stop` untrusted too, the looks end it after 5 still seconds, with its screen). Fix: once a `Working` or
  `Stopped` comes from a Codex whose `submitted` is still 0 (a turn ran, and no `UserPromptSubmit` said so), take it
  as one without that hook: `confirm` leaves its tasks alone, and its `Stopped` answers the task handed over.
- [ ] **Gemini CLI's `BeforeAgent` for a prompt typed while it works is unchecked.** `hooks::gemini`
  (`crates/templum-mcp/src/hooks.rs`) takes it as Codex's (`at_enter: false`); if Gemini CLI fires it at the Enter
  and queues the prompt, a turn under way would answer the task. Fix: check by hand once logged in; `at_enter:
  true` if so.
- [ ] **On Windows a spawned Claude Code or Codex may get its prompt in its trust question.** `look_ready`
  (`crates/templum/src/workspace/spawn.rs`) skips the input check there (ConPTY may not pass bracketed paste and the
  cursor on), so a still trust question takes the prompt. Without bracketed paste `type_in` sends it as keys,
  control characters as spaces (`typed_bytes`, `crates/templum-terminal/src/input.rs`): Codex's question ("1. Yes,
  continue  2. No, quit") takes a `1` or `2` at once, and the caller's 5-character id in `[from … (xxxxx)]` has one
  about 1 time in 4 (the task's own text may too). A `2` quits Codex; a `1` trusts the folder for the person, and the
  digits after it may answer its hooks question. Claude Code's question (highlighted "No, exit") may take keys the
  same way. A question that shows no typed text gets no Enter, and the task ends untaken. Before batch 7 a spawned
  Claude Code waited for its `SessionStart`, which comes after its trust question, on every platform: on Windows this
  is a regression for it (Codex was looked at the same way before). Fix: check by hand on Windows whether
  `Modes::typing` follows; if so, drop the `cfg!(windows)`. Meanwhile, one condition in `look_ready` restores the old
  wait there: on Windows a gated Claude Code also waits for `Pane::speaks` (Codex's `SessionStart` comes only with a
  prompt).
- [ ] **The person's own prompt queued in Claude Code shows idle for a few seconds, and a task typed then gets the
  wrong answer.** After the turn it was queued behind ends (`Stopped`, `crates/templum/src/workspace/bus.rs`), no hook
  says the next turn starts; the dot is idle until the screen's looks see it work (about 3 s). A task delegated in
  those seconds is queued by Claude Code behind the person's turn, but `Submitted` finds the agent Idle and doesn't
  mark it `behind`: the person's turn's end answers it with their answer. Fix: keep `Working` on a `Stopped` that
  comes while a prompt Claude Code queued is pending (a pane-level `behind`).
- [ ] **Esc while Claude Code writes loses the answer of a task queued behind its turn, or handed over just after.**
  No hook ends the interrupted turn, so the task's `behind` (`Handed`, `crates/templum/src/workspace/mod.rs`) stays
  set; the queued task then runs as a new turn (no `UserPromptSubmit`) and its `Stop` (`bus.rs`) only clears
  `behind`. The agent that handed it gets "Its turn ended without an answer" from `idle_prompt` a minute later
  instead of the answer. A task delegated in the seconds after such an Esc, while Templum still has it `Working`
  (about 6 s, until its screen's looks rest it), is `queued` too: Claude Code takes it at once, its
  `UserPromptSubmit` comes at its Enter while it is still `Working`, so the `Submitted` arm of `agent_event`
  (`bus.rs`) marks it `behind`; answered without a tool, its `Stop` only clears `behind`, and the same `idle_prompt`
  ends it. A `Working` only the looks set does the same. Fix: clear `behind` when the person's keys
  (`Pane::person_typed`) come while the agent works (no hook tells today); and in `agent_event` read
  `pane.looks.answered || pane.typed.get()` before `heard()`, marking `behind` only when neither is set (`turn &&
  at_enter && !that`).
- [ ] **The person's prompt entered just before a task can answer it.** The person presses Enter on their own prompt
  in Claude Code and, in the moment before its `UserPromptSubmit` comes (about 0.3 s), an agent delegates to it: it
  is still Idle, so the task isn't `queued`. The person's `Submitted` (`agent_event`,
  `crates/templum/src/workspace/bus.rs`) marks the task taken and not `behind`, the task's own `Submitted` is
  ignored, and the person's turn's `Stop` answers the task with the person's answer. Fix: carry the hook's `prompt`
  in `Submitted` (`hooks::claude`, `crates/templum-mcp/src/hooks.rs`) and count one toward the task only if it comes
  after the task's Enter and starts with `[from `. Claude Code's `UserPromptSubmit` also fires for a scheduled task and
  a `/loop` iteration (a background subagent's or command's report no longer counts: card 127): one of those in the second or two before the task's
  Enter is taken marks the task taken (and satisfies `confirm`, `delegate.rs`, even if its Enter was lost), and that
  turn's `Stop` answers it with, say, the subagent's summary, while the task runs later for no one. The same fix
  covers it.
- [ ] **A hooked agent whose screen moves with no hook seems stuck half a minute later.** `Looks::state`
  (`crates/templum/src/workspace/activity.rs`) turns an Idle agent `Working` when its screen changes in 3 of 4 looks,
  hooks or not; one whose hooks speak, then still for 30 looks, seems stuck: its dot turns yellow and `delegate`
  refuses it as waiting for its person (before batch 7 it rested after 5 still seconds). The person's own slower
  `Stop` or `SessionStart` hooks drawing after Templum's, or a background task's footer, can do it. Claude Code
  comes back at its `idle_prompt` only when its screen moved within a minute of its last `Stop`: that comes once per
  idle stretch, 60 s after the `Stop` (S2). Otherwise it stays yellow, as Codex does, until a hook comes or its screen
  moves again. Fix:
  a bit in `Looks` set when `state()` turns `Working` from None or Idle, cleared by `heard()`: only an agent a hook
  set `Working` seems stuck; one the looks set rests after `STILL` looks.
- [ ] **An agent that seems stuck never rests from its screen.** In `Looks::state` (`activity.rs`) only a changing
  screen leaves `STUCK_TEXT`. A Claude Code dialog no hook tells of, open mid-turn, seems stuck after 30 s; the
  person declines it or presses Esc, and the agent is back at its prompt with no hook and a still screen: the dot
  stays yellow, `delegate` refuses it as waiting for its person, and its task waits (Claude Code until its
  `idle_prompt` ends the task without an answer, a minute after it went back to its prompt; one that seemed stuck
  more than a minute into a rest, as in the entry above, gets none, and its task waits, as Codex's does, for the
  task's timeout). Fix:
  `Some(NeedsYou(Some(m))) if m == STUCK_TEXT && self.answered && still >= STILL => Some(Idle)`, which ends the
  task with its screen through the `still` list of `look_at_agents`.
- [ ] **`interrupt` on a task its agent queued and hasn't taken yet starts it, its answer going to no one.**
  `interrupt` (`crates/templum/src/workspace/delegate.rs`) ends the caller's task, presses Esc, and says "Pressed Esc
  … and ended the task you handed it", whatever the task's state. Claude Code delegates "say B" with `wait` to a Codex
  at work on its person's `sleep 60`: Codex holds it as a steer and says nothing until the turn takes it
  (`Handed::taken` stays false, S3). An `interrupt` 10 s later aborts the person's turn, and Codex's Esc "interrupt[s]
  and send[s] immediately": it starts "say B" at once. A Claude Code at work queues the task (`behind`), and after an
  Esc it runs a queued message as its next turn (S2). Either way the agent does the task while the caller was told it
  ended, and its turn's `Stopped` (`bus.rs`) finds no task handed: the answer goes to no one. Fix: when the caller's
  task is still `!taken || behind`, say so in the result ("it had queued your task and may still run it"); check by
  hand whether Ctrl+C (Codex) or Up and clearing the input (Claude Code) drops the queued task instead, and press that.
- [ ] **A spawned Gemini CLI may get its prompt in its folder-trust question.** `spawn_agent`
  (`crates/templum/src/workspace/spawn.rs`) gates only Claude Code's and Codex's prompt on their input box
  (`Prompt::gated`); Gemini CLI asks to trust a folder the first time it opens it, and after a still second
  `look_ready` pastes the prompt into that question. If the screen doesn't change, no Enter goes and the caller hears
  "didn't take the task", Gemini still asking, the pasted text maybe left behind; if the dialog reacts to the paste,
  it may answer the question for the person. Also, `look_ready` doesn't check `takes_typing` (`delegate.rs`), so an
  ungated agent at a dialog a hook told of gets the paste too. Fix: check by hand (once logged in) whether Gemini CLI
  hides its cursor in that question; if so, gate `Agent::GEMINI` too; and have `look_ready` wait while `takes_typing`
  fails.
- [ ] **A dialog's 300 ms wait may answer a `wait_for(needs_you)` that came after the agent stopped asking.**
  `needs_you` (`crates/templum/src/workspace/waits.rs`) answers, `DRAWN` after the hook, every `needs_you` wait on the
  terminal then, also one registered meanwhile after the dialog closed (a hook that answered it at once). The agent
  waiting is told its peer waits for its person when it no longer does. Fix: collect the wait keys when the hook
  comes, answer only those, and skip when the pane is gone.

Left by batch 8 and its review; line numbers are those of `development` at c51be7e.

- [ ] **A resumed agent handed a task within 5 s of opening counts its earlier turns as the task's.** `spent`
  (`crates/templum/src/workspace/delegate.rs`) takes nothing read at the hand-over as nothing spent, so that a
  session's first task counts it whole; usage is read every `session::EVERY` (5 s), so a resumed session (after a
  restart, or opened by hand) handed a task before that first read comes back with `done in 12 s, 840211 tokens`.
  Fix: read the agent's usage at the hand-over (as `tell_end` does at the end) when `pane.usage` is none.
- [ ] **What Claude Code spent comes back only with its status line, and Gemini CLI's never.** `spent`
  (`crates/templum/src/workspace/delegate.rs`) takes the usage chip's figures: a Claude Code without the status line
  (`usage.claude_limits` off) gives no cost, and its transcript-read session no tokens; Gemini CLI's session isn't
  read. A person sees `[task t7 …: done in 31 s, transcript …]` without a figure. Fix: sum the `usage` of the
  assistant lines its transcript (`transcript_path`) gained since the hand-over.
- [ ] **A task's end doesn't say how many tools its agent ran.** `tell_end` (`delegate.rs`) gives time, spending and
  transcript; Claude Code's subagents also say how many tools they ran. A person sees the asker read `[task t7 …: done
  in 31 s, 60231 tokens, transcript …]` and can't tell, nor can the asker, a task that ran 40 commands from one
  answered at once without opening its transcript. Fix: count `Working` events (each tool's end) on `Handed` from the
  hand-over, and say them (`, 40 tools`).
- [ ] **A task past its timeout that another agent takes over never comes back.** Its asker waits for its end, but
  `delegate` (`delegate.rs`) lets another agent hand that agent a task once the deadline passed, replacing `handed`;
  the first asker never hears. Fix: when replacing a timed-out task of another asker, tell that asker it
  failed ("{uid} took another agent's task").
- [ ] **A notice reaches an asker inside another tool call only once that call returns.** Claude Code and Codex queue
  typed text while a tool runs (batch 7's S2, S3): an asker waiting on `delegate` with `wait` to another agent, or
  on `ask_user`, reads a notice of another task late. A person sees the asker answer it minutes after it was typed.
  Fix: none needed unless it bites; the notice's time says when the task ended.
- [ ] **Two tasks that end within a second may come back out of order.** `tell_end` (`delegate.rs`) queues each
  end's notice only once its own read of what the task spent is done, on the background executor; a slower read (a
  long Codex rollout) queues later, so the asker may get t8's end before t7's. Its header says each one's time. Fix:
  queue the notice at the end and fill in what it spent once read, delivering a notice only once filled.
- [ ] **A change the agent doesn't take fails its whole task, and its real end is lost.** `delegate`
  (`crates/templum/src/workspace/delegate.rs`) types a change with `send_task(…, true)`: when its Enter doesn't go (a
  dialog opened as it was typed, or its screen never showed it), `not_taken` ends the task. A person sees the asker
  told `[task t7 …: failed …] … didn't take the task` while the agent's block still works on t7, and its answer never
  comes back. Fix: give `send_task` a mode for a change that, untaken, tells the asker only that the change wasn't
  taken (a notice, or the `wait` call's answer) and sets `handed.taken` back, keeping the task.
- [ ] **A task handed with `wait` never tells its asker its id.** `delegate` (`delegate.rs`) answers a `wait` call with
  the task's end only, and its timeout (`expire`, `crates/templum/src/workspace/waits.rs`) says "No answer from k9x1d
  within 600 seconds" without it. Claude Code stops waiting on a call past 2 minutes, and the end then comes back typed
  as `[task t3 …]`: a person sees the asker get a task id it never saw (only the block id ties it to its call). Fix:
  name the task in `expire`'s `Turn` text when the caller handed it, and say in `delegate`'s description that a `wait`
  call its client stops waiting on comes back typed as `[task …]` (the timeout text alone doesn't cover that cancel,
  which comes before a 600 s deadline).
- [ ] **A notice from an agent that quit goes into the next agent started in its terminal.** `Workspace::notices` and
  `Handed::by` name a terminal (`tell_end`, `deliver_notices`, `delegate.rs`): when the asker's agent quits and the
  person starts another there before the task ends, the new session is typed `[task t7 …]`, a task it never handed
  over, and may act on it. Fix: in `agent_left` (also run on a session's end), drop the notices to that terminal and
  mark the tasks it handed (`Handed::by` that terminal) so their end isn't told.
- [ ] **On Windows, a notice or a schedule right after an agent quit can go into Git Bash.** `takes_notice`
  (`crates/templum/src/workspace/delegate.rs`) drops a notice when the shell is in front, and a schedule's `deliver`
  (`schedules.rs`) refuses an agent whose shell is in front, but `shell_in_front` is always false on Windows, and
  `agent_left` leaves `seen.agent` and `seen.pid` set until the next snapshot (up to 5 s). A person sees, after
  Codex's `/quit` or Claude Code's `/exit`, the worker's answer, a schedule's `[scheduled s3]` to the caller, or a
  schedule's task handed through `delegate` to another agent typed with Enter into the shell, and bash runs its lines
  as commands (`reply` has the same gap, but notices and schedules come on their own). Fix: in `agent_left`, set
  `seen.pid = None`, so `takes_notice` waits until a snapshot finds an agent again or clears `seen.agent`, which drops
  the notice, and `takes_handing` refuses the schedule's task ("still starting"), told to the caller and tried again
  at its next time.
- [ ] **A task whose agent quit without a hook is told under the shell's title.** `see`
  (`crates/templum/src/workspace/persist.rs`) writes the new `seen` (no agent) before it calls `agent_left`, so
  `signature_of` (`delegate.rs`) falls back to the terminal's title. A person sees the asker get `[task t1 from sh
  (kf3jc): failed in 0 s] …` for a Codex that quit mid-task (Codex sends no hook when it quits). Fix: call
  `agent_left` before `see` overwrites `seen`, or keep the worker's signature on `Handed` at hand-over; assert the
  title in `a_task_whose_agent_left_or_block_closed_comes_back_as_failed`.
- [ ] **No test pins that a timeout isn't typed while a call of its asker waits.** `timed_out` (`delegate.rs`) skips
  the notice when `waits_on` says a call of the asker waits for the task; removing that check fails no test. If it
  broke, a person would see an asker inside `wait_for_idle` also typed `failed … within 600 seconds`. Fix: delegate
  without `wait` with `timeout: 2`, keep a `wait_for_idle` (timeout 10) pending past the deadline, and assert no
  notice.
- [ ] **What the next task spent can start from a stale figure after a task ended through a call.** `tell_end`
  (`delegate.rs`) writes its fresh read of what the task spent back into `pane.usage`, but a task that ended into a
  waiting call (`delegate` with `wait`, `wait_for_idle`) or through `reply` doesn't read again, so `pane.usage` stays
  the last snapshot's, up to 5 s old (spike S8-2: a Codex call or two short). A person sees the next task handed right
  after it come back with the earlier task's last calls counted in its tokens or cost. Fix: do the `SPENT_AFTER` read
  and write-back whenever `end_turn` or `reply_to` takes a `handed`, telling only when no call waits.
- [ ] **A person's half-typed message goes out with a notice.** `takes_notice` (`delegate.rs`) waits only `TYPING`
  (3 s) after the last key: a person who pauses longer mid-message in their orchestrator gets the notice pasted after
  their draft, and its Enter sends both. Fix: a longer quiet time for notices than for a task, or wait while the
  agent's input line isn't empty, if the screen can tell.
- [ ] **A notice whose Enter never goes is lost.** `deliver_notices` (`delegate.rs`) takes a notice off the queue
  before `send_task(…, false)` types it, and with `task` false a `GiveUp` does nothing. A person sees an asker whose
  screen ignored the paste (a Codex whose hooks haven't spoken yet) never hear its task's end. Fix: when its Enter
  gives up, put the notice back at the front of `Workspace::notices`.
- [ ] **A task or a reply typed right after a notice can share its Enter.** Only notices wait `NOTICE_GAP` after
  `Pane::sent` (`takes_notice`, `delegate.rs`); `delegate` and `reply` (`takes_handing`) don't. A person sees, at
  worst, a task typed within about 2 s of a notice ended as untaken ("didn't take the task") while its agent works on
  it (batch 7 had this between a task and a reply; notices make it likelier, as they come on their own). Fix: make
  `takes_handing` refuse, with "try again in a few seconds", while `sent` is within `NOTICE_GAP`.
- [ ] **That a task's end comes back after Claude Code stops waiting rests on `notifications/cancelled`, never seen
  live.** Review Focus 2 is tested only through `bus_note`'s cancel. If Claude Code moves a call past 2 minutes to
  the background without cancelling it, the wait stays to its 600 s deadline: `waits_on` (`delegate.rs`) is true at
  the task's end, the answer goes to a call nobody reads, and a person sees nothing typed into the asker. Fix: check
  it by hand (`delegate` with `wait` on a `sleep 150` task, the `[task …]` comes back typed); if the cancel never
  comes, drop a wait whose client went away (its MCP connection's request id answered or abandoned).

Left by the ask_user dialog's redesign (kanban card 81, `docs/specs/ask-user.md`).

- [ ] **A plan's frame ignores the stopped banner and a wrapped foot.** `detail` (`crates/templum/src/inbox.rs`) gives
  `plan` the popup's height less its border and `FOOT` (51), which the banner of a plan the agent stopped waiting for
  (about 50) and a foot that wraps (38 more, in a column narrower than 260) make too much: the frame can be taller than
  the body and the wheel over it never scrolls the body to Comment. Fix: subtract the banner and the foot's real
  heights.

- [ ] **A lit tab on an upper line of wrapped tabs is underlined over the next line.** `tabs`
  (`crates/templum/src/balloon.rs`, kanban card 115) draws one hairline under the whole row and each tab's 2-unit
  underline at its own bottom: where many questions or a narrow block wrap the row, the lit tab of the first line
  has its accent line between the two lines, not on the hairline. A person still sees which is lit, the line just
  floats. Fix: a hairline under each line (a row per line, splitting the tabs by measured width), or tabs as
  filled pills (`kanban::segment`'s look) when the row wraps.


Left by ask_user v2 (kanban card 34, `docs/specs/ask-user.md`).

- [ ] **A question Claude Code moves to the background isn't checked by hand.** Claude Code moves a tool call past 2
  minutes to the background; its documentation says it keeps waiting there, which no one has watched with
  `ask_user`. If it cancels the call instead, the dialog says the agent stopped waiting and the answer is typed back
  (`send_question`, `crates/templum/src/workspace/questions.rs`); if it drops it without a cancel, a person's answer
  goes to a call no one reads and nothing is typed. Fix: ask from Claude Code, leave it 3 minutes, answer, and write
  what happened in `ask-user.md`'s client table and connections.md's Risks row; on a silent drop, treat the call's
  backgrounding hook (if one comes) as a cancel.
- [ ] **The question's sound is unheard on macOS and Windows.** `chime::players` (`crates/templum/src/chime.rs`) runs
  `afplay /System/Library/Sounds/Glass.aiff` on macOS and PowerShell's `[System.Media.SystemSounds]::Asterisk` on
  Windows; neither was played on those systems, so a person there may hear nothing (or a console flash on Windows).
  On Linux both `canberra-gtk-play` and `pw-play` exit 0, but no one listened. Fix: play it there and adjust
  `chime::players`.
- [ ] **The inbox and ask_user's dialog aren't tried by hand.** The inbox (`crates/templum/src/inbox.rs`, card 144) and
  the dialog it holds (`crates/templum/src/balloon.rs`) were built and tested headless only: the popup's look, its
  list's rows, the notice's chip on a terminal, the sidebar button's badge and pulse, the options' panel and marks,
  the preview beside or under the options, the scrollbar, the highlight, and Claude Code, Codex and Gemini CLI each
  asking with `questions`, then Esc in Claude Code and an answer typed back, weren't seen. A person may meet
  misaligned marks, a pane that crowds the options or a chip that covers the terminal's last line. Card 115's
  tabs (`tabs` in the same file: their rings, the lit tab's underline, a row that wraps) weren't seen either. Fix: run
  `cargo run -p templum -- --dev`, ask with the spec's example from each agent and from `templum ask`, and correct
  what looks wrong.
- [ ] **A withdrawn request leaves its alert.** `tell_asking` (`crates/templum/src/workspace/waits.rs`) gives the
  block an "Asks you: …" alert that only answering clears (`seen_asking`); after the agent quits or the deadline
  passes, the workspace's tile stays lit and Ctrl+Shift+A leads to a block with no dialog. Fix: clear that alert
  when a Question or Connect wait is removed in `expire`, `closed` and `drop_orphans`.
- [ ] **A preview keeps a stray character of some escape sequences.** `clean_preview`
  (`crates/templum-mcp/src/ask.rs`) drops one character after ESC, so `\x1b(B` (tput `sgr0`, ncurses output) shows a
  "B". Fix: skip the intermediate bytes 0x20–0x2F, then the final byte.
- [ ] **The preview is cleaned on every frame.** `balloon::preview` (`crates/templum/src/balloon.rs`) calls
  `clean_preview` each time the dialog draws; a large preview costs that much per frame. Fix: clean it once in
  `ask::choice` when parsing, which also lets the headless tests cover it.
- [ ] **Clicking Other's radio only puts the keyboard in its field.** After picking an option, a click on Other's
  mark (`crates/templum/src/balloon.rs`, `q<q>-other-mark`) leaves the option picked though Other's text is still in
  the field, so Send answers the option. Fix: on that click, also `form.typed(ask, q, <the field's text>)`.
- [ ] **Esc and the arrows do nothing in a question's first half second.** `question_key`
  (`crates/templum/src/workspace/questions.rs:204`) returns on `too_soon` before matching the key, so it blocks every
  dialog key, though the spec only blocks those that answer (options, Send, Dismiss). A person who presses Esc as the
  dialog appears keeps it and has to press again. Fix: check `too_soon` only for Space and Enter.

- [ ] **An agent typed in a terminal isn't an agent to `delegate` for its first seconds.** Left by
  `docs/plans/2026-10-18-agents-in-any-terminal.md`: a `claude` typed by hand has its hooks at once, but
  `pane.seen.agent` (`crates/templum/src/workspace/delegate.rs:216`, at 3924a70) comes from the 5-second snapshot, so
  a `delegate` to it in that time is refused as not an agent, and `list_blocks` shows no agent. Fix: on the first hook
  event of a terminal with no agent, take the agent from the hook (`templum hook claude` knows it) into
  `pane.seen.agent`.
- [ ] **PowerShell's `claude` function changes `--` and `-p:x`.** Left by
  `docs/plans/2026-10-18-agents-in-any-terminal.md` (`crates/templum-terminal/src/integration/templum.ps1`, the
  `claude` and `codex` functions): a plain function's `$args` drops a `--`, and `-p:foo` reaches the program as `foo`
  (seen with pwsh 7.6.7 and a fake `claude` that prints its arguments), so `claude -- …` or `claude -p:x` typed in
  PowerShell reaches Claude Code differently than without the function. Fix: when
  the words hold `--` or a `-x:y`, run the program with the line's own text after the name (`$MyInvocation.Line`),
  else as today.
- [ ] **In PowerShell, `&&`, `||` and `$?` see a typed agent succeed whatever its exit code.** Left by
  `docs/plans/2026-10-18-agents-in-any-terminal.md` (`crates/templum-terminal/src/integration/templum.ps1`,
  `__TemplumAgent`): a plain function can't make its caller's `$?` false, so `claude -p x && git push` pushes after
  Claude Code exited 3, and `claude; $?` prints True; only the prompt's mark is told. And `claude -p x; /bin/true`
  marks the line failed (D;1) where bash marks it 0: the prompt turns `$?` false for the agent's code though a later
  program succeeded. Fix: none for `$?` without `[CmdletBinding()]`, which would take `-p` and `-d` as PowerShell's
  own; for the mark, turn `$?` false only when `$LASTEXITCODE` is still the agent's code.
- [ ] **A terminal opened before the switch changed keeps words that no longer match the shared files.** Left by
  `docs/plans/2026-10-18-agents-in-any-terminal.md` (`Workspace::agent_env`, `crates/templum/src/workspace/usage.rs`;
  `settings_file`, `crates/templum-usage/src/claude.rs`): every new terminal rewrites the one
  `claude-statusline.json` for the current config, while older terminals keep their `TEMPLUM_CLAUDE_ARGS`. Turn
  `[connections]` off and open a terminal: a `claude` typed in an older one still gets `--mcp-config=…mcp.json` but
  a settings file without hooks, Templum's server with a dot that never moves. Fix: a file per content (the hooks in
  their own settings file, named in the words), or drop `--mcp-config` from the words when the file has no hooks.
- [ ] **An alias of `claude` to a path skips Templum's words without a word.** Left by
  `docs/plans/2026-10-18-agents-in-any-terminal.md` (`templum.bash`, `zsh/.zshenv`): `alias claude=~/bin/claude` in
  a `.bashrc` or `.zshrc` expands to a path, which runs the program alone (as connections.md says of a path), so the
  agent has no server, hooks or status line and nothing tells the person why. Fix: name an alias to a path in
  connections.md's "Where it doesn't reach", or have bash and zsh run the alias's program with Templum's words
  when its first word ends in `/claude` or `/codex`.
- [ ] **In zsh, a typed agent suspended and brought back loses its exit code.** Left by the review of card 42
  (`__templum_agent`, `crates/templum-terminal/src/integration/zsh/.zshenv`): Ctrl+Z on a `claude` or `codex` typed in
  a zsh terminal, then `fg`, and when it ends `$?` is 0 and the prompt's mark shows success whatever its code: zsh
  moves the rest of a suspended function into a subshell, which doesn't keep it. connections.md says so. Fix: a
  program named `claude` on PATH instead of a function keeps it, at the cost connections.md's "Considered and left
  out" gives; nothing within a function was found.
- [ ] **Bytes piped into an agent typed in PowerShell 7.4+ reach it as text.** Left by the fix for piped input
  (`claude`, `codex` and `__TemplumAgent`, `crates/templum-terminal/src/integration/templum.ps1`): PowerShell passes
  a program's bytes straight to the next only when no function stands between them, so `cat a.png | claude -p
  describe` gives the agent the bytes decoded as text (a byte that isn't UTF-8 becomes `U+FFFD`) and a newline at
  the end; text such as `git diff` arrives whole. Fix: a program named `claude` on PATH instead of a function, at the
  cost connections.md's "Considered and left out" gives.
- [ ] **In zsh, a typed agent that isn't installed skips the person's not-found handler.** Left by the second review
  of card 42 (`crates/templum-terminal/src/integration/zsh/.zshenv:41`): a `claude` or `codex` missing from PATH prints
  zsh's plain message, so install hints from pkgfile or Homebrew's `command_not_found_handler` don't show; bash's
  handler does run. Fix: when `(( $+functions[command_not_found_handler] ))`, call
  `command_not_found_handler "$name" "$@"` and return its status.
- [ ] **In fish before 3.2, a typed agent that isn't installed emits no not-found event.** Left by the second review
  of card 42 (`crates/templum-terminal/src/integration/templum.fish:28,49`): the fallback prints fish's message but
  never emits `fish_command_not_found`, so Ubuntu 20.04's command-not-found hints don't show for `claude` or
  `codex`. Fix: `emit fish_command_not_found claude` (and `codex`) after the echo.
- [ ] **PowerShell's integration doesn't install under `Set-StrictMode`.** Left by the second review of card 42
  (`crates/templum-terminal/src/integration/templum.ps1:8,15`; older than the card at line 8): with
  `Set-StrictMode -Version 1` or `Latest` in the profile, line 8 reads an unset variable, throws, and the
  `try {} catch {}` swallows it, so a person sees no prompt marks and no typed-agent functions; line 15 reads
  `$Global:__TemplumAgentExit` before `Set-StrictMode -Off` without setting it. Fix: test with
  `Test-Path Variable:Global:__TemplumPrompt`, set `$Global:__TemplumAgentExit = $null` beside `__TemplumRunning`,
  and move line 15 below `Set-StrictMode -Off`.
- [ ] **Codex's and Gemini CLI's own questions aren't checked against the dot.** Left by the parity audit of card 44
  (gap 13): `hooks::claude` (`crates/templum-mcp/src/hooks.rs:64`) turns Claude Code's `AskUserQuestion` yellow;
  whether Codex's `request_user_input` or Gemini CLI's `ask_user` fires a hook Templum hears was never tried, so a
  person may see one of them asking with no yellow dot. Fix: try each by hand; map the hook that tells, as
  `AskUserQuestion` is mapped.
- [ ] **A `qwen -p` that a Qwen Code in Templum runs speaks for its block.** Left by the plan of card 44
  (`docs/plans/2026-10-18-agent-parity.md`, task 11): Qwen Code's hooks come from `QWEN_CODE_SYSTEM_SETTINGS_PATH`,
  which every program it starts inherits, so a `qwen -p` its shell tool runs sends its own `Stop`, and a task handed
  to the block ends with that run's answer. OpenCode's and Pi's plugins avoid it with `TEMPLUM_HOOKED`; a hook command
  can't. Fix: the pane follows the session id of the first hook that speaks for it and drops another session's
  events, a new session counting only from a `SessionStart` that isn't `compact`.
- [ ] **OpenCode's dialogs and its TUI weren't seen with Templum's plugin.** Left by the plan of card 44 (task 9,
  `crates/templum-usage/src/opencode.js`, `hooks::opencode`): only `opencode run` was checked by hand, so whether
  `permission.asked` and `question.asked` turn the dot yellow (their `permission` and `questions` keys), whether
  `session.error` comes with a `session.idle` after it, and what Esc fires are from OpenCode's types, not seen. A
  person may see a green dot while OpenCode asks. Fix: in the TUI, run a `bash` command that asks, answer once and
  refuse once, ask its question tool, press Esc during a turn, and compare the lines the plugin hands the hook.
- [ ] **Pi with Templum's extension wasn't seen answering.** Left by the plan of card 44 (task 12,
  `crates/templum-usage/src/pi.ts`): Pi 0.85.1 had no model it could use here, so only the order of its events was
  seen; the answer read from `agent_end`'s messages, a `ui_prompt_start` dialog, a `pi -p` its shell tool runs staying
  quiet, and the server `registerMcpServer` gives (Pi 0.99 and later) are from Pi's docs. A person may get a task's
  end without its answer, or a dot that stays green at a dialog. Fix: with a model, delegate a task to a Pi block and
  read its answer; open an extension's `ctx.ui.confirm`; on Pi 0.99 or later, run `/mcp` and call `whoami`.
- [ ] **Qwen Code and Kilo Code weren't run with Templum's server and hooks.** Left by the plan of card 44 (tasks 9 and
  11; `crates/templum-usage/src/qwen.rs`, `opencode.rs`): neither is installed here, so their words, files and
  variables are from their docs and sources (Qwen Code 0.25.2, Kilo Code 7.8), checked only with fakes. A person may
  see Qwen Code refuse `--mcp-config` or ignore the system settings file, or Kilo Code ignore `KILO_CONFIG_CONTENT`.
  Fix: install each, open it from the menu and typed in bash: `/mcp` lists `templum`, a delegated task comes back
  with its answer, a permission turns the dot yellow; for Qwen Code also with `/etc/qwen-code/settings.json` present
  (the server only).
- [ ] **Gemini CLI's notice wasn't clicked by hand.** Left by the plan of card 44 (task 3, `tell_gemini`,
  `crates/templum/src/workspace/usage.rs`): the headless test counts the notice, but the click asking Install's
  question and installing wasn't tried in a window. A person may click it and see nothing. Fix: without the extension,
  open Gemini CLI from the menu, click the notice, answer Install: Settings ▸ Connections shows it installed.
- [ ] **`opencode`, `kilo` and `qwen` typed in PowerShell under `Set-StrictMode` lose Templum's variable.** Left by
  the final review of card 44 (`crates/templum-terminal/src/integration/templum.ps1:73`): `(Get-Item "Env:$var"
  -ErrorAction Ignore).Value` throws when the variable is unset, so a person with `Set-StrictMode -Version Latest`
  sees a red error and the agent starts without Templum's server (today the whole integration already doesn't
  install under strict mode: the entry above). Fix: `-not [Environment]::GetEnvironmentVariable($var)`.
- [ ] **Qwen Code's hooks on Windows name no shell.** Left by the final review of card 44
  (`crates/templum-usage/src/qwen.rs:52`): each hook is `& '<bin>' hook qwen`, PowerShell's syntax, with no `"shell"`;
  Qwen Code's hooks take `"shell": "bash" | "powershell"`, and if its default on Windows isn't PowerShell a person sees
  a Qwen Code whose dot never moves. Fix: write `"shell": "powershell"` on Windows (`"bash"` elsewhere) once checked
  that Qwen Code 0.25 accepts the key.
- [ ] **A Templum started from an OpenCode, Kilo Code or Qwen Code block keeps the first one's variable.** Left by the
  final review of card 44 (`in_env`, `crates/templum/src/workspace/usage.rs:50`, and the shells' "already set"
  check): `OPENCODE_CONFIG_CONTENT`, `KILO_CONFIG_CONTENT` or `QWEN_CODE_SYSTEM_SETTINGS_PATH` inherited from the first
  Templum's block counts as the person's own, so the second Templum's agents run the first one's plugin and program
  (still reporting to the second's bus). A developer testing a plugin change runs the old one. Fix: when Templum's
  environment has `TEMPLUM_BUS`, don't count a value naming `/opencode/templum.js` or `/qwen/settings.json` as the
  person's.
- [ ] **Copilot CLI's `agentStop` hook may wait 3 seconds for nothing.** Left by the final review of card 44
  (`copilot_answer`, `crates/templum-mcp/src/cli.rs:473`): the transcript is polled before the bus is tried, so with
  Templum gone, connections off, or a turn that ends on an empty message, the hook takes its full 3 seconds, which
  Copilot may wait for before its next prompt. Fix: open the bus first and return at once when it fails; stop polling
  once an `assistant.turn_end` follows the last prompt.
- [ ] **Copilot CLI's answer could be the last turn's on a very fast turn.** Left by the final review of card 44
  (`hooks::copilot_answer`): the answer is the last `assistant.message` after the last `user.message`; if Copilot
  hasn't written this turn's `user.message` when its `agentStop` hook runs, the previous turn's answer comes back. Not
  seen (the hand check's turns were written in time). Fix: match the turn by the `sessionId` and timestamp of the
  hook's payload, or wait for an `assistant.turn_end` written after the hook started.
- [ ] **OpenCode's plugin reads the whole session at each turn's end.** Left by the final review of card 44
  (`crates/templum-usage/src/opencode.js:42`): `client.session.messages` without a limit, so a long session's end
  of turn moves megabytes of JSON before the dot turns idle. Fix: pass a small `query.limit` once checked that OpenCode
  returns the newest messages with it.
- [ ] **OpenCode's plugin keeps message ids in its set of subagent sessions.** Left by the final review of card 44
  (`crates/templum-usage/src/opencode.js:60`): an assistant message carries a `parentID` too, so the set grows by one
  id per message for the whole run. Harmless to the dot. Fix: add to it only on `session.created` and
  `session.updated`.
- [ ] **Gemini CLI's notice about its extension comes back at every run.** Left by the plan of card 44 (task 3,
  `tell_gemini`, `crates/templum/src/workspace/usage.rs`): a person who doesn't want Templum's extension sees
  "Gemini CLI has no Templum tools or status dot" the first time they open a Gemini CLI after each start of
  Templum. Fix: a "Don't ask again" on it, kept in `config.toml` (`[connections] gemini_notice = false`).
- [ ] **A bare `copilot` still reaches AWS's Copilot CLI with Templum's words.** Left by the final review of card 44
  (the `copilot` functions of `crates/templum-terminal/src/integration/templum.bash`, `zsh/.zshenv`, `templum.fish`
  and `templum.ps1`): a person with AWS's Copilot CLI who types `copilot` alone in a Templum terminal sees
  `✘ unknown flag: --allow-all-mcp-server-instructions` where AWS's CLI would print its help (its subcommands, help and
  version already run as typed). Fix: tell the two programs apart by the path of the `copilot` found, or tell the
  person to type `command copilot` for AWS's.
- [ ] **Qwen Code won't start when the person turns on `tools.executionSandbox`.** Left by the final review of card 44
  (`args`, `crates/templum-usage/src/qwen.rs`, and the `qwen` functions of the shell integration): Qwen Code 0.25.2
  refuses `--mcp-config` with that setting ("tools.executionSandbox does not yet support ACP, worktree management,
  LSP, MCP, extensions or provisional workspaces."; read in its bundled source, not run), so a Qwen Code opened from
  the canvas or typed in a terminal exits with that error. Fix: leave the word out when Qwen's merged settings set
  `tools.executionSandbox`, or document it in Settings ▸ Connections.
- [ ] **fish 3.1 shows `[1]` at its first prompt in a Templum terminal.** Seen by the final review of card 44, and
  already so at 5579a05 (`crates/templum-terminal/src/integration/templum.fish`): under fish 3.1.2 the `shells::fish`
  test fails ("waited for cd to end") and the first prompt shows a last status of 1; CI doesn't install fish, so
  nothing catches it. Fix: run the integration under fish 3.1 and find the command that returns 1 at startup.
- [ ] **`qwen hook …` and `qwen -d mcp list` typed in a terminal still get `--mcp-config`.** Left by the second review
  of card 44 (the Qwen command lists in `crates/templum-terminal/src/integration/templum.bash:120`,
  `zsh/.zshenv:106`, `templum.fish:99`, `templum.ps1:142`): `hook` is Qwen 0.25.2's one top-level alias (of
  `hooks`), and a command after an option isn't looked for, so Qwen prints `Unknown arguments: mcp-config, mcpConfig`
  and exits 1. Fix: add `hook` to the four lists; the option-first case can stay.
- [ ] **In PowerShell, `qwen MCP list` or `pi LIST` loses Templum's words.** Left by the second review of card 44
  (`crates/templum-terminal/src/integration/templum.ps1:142` and Pi's list beside it): `-in` ignores case, though
  Qwen and Pi read those words as a prompt, so the agent starts without Templum's server and hooks. Fix: `-cin`.
- [ ] **On Windows a `wait_for` pattern reads ConPTY's drawing, not the program's lines.** Left by card 35
  (`crates/templum-terminal/src/tap.rs`, `Scanner::keep`): ConPTY sends Templum its redraw of the screen, so after a
  resize or a full redraw a line may come again (a second match for a new watch) or two lines joined. Fix: ConPTY's
  passthrough mode, once alacritty_terminal offers it.
- [ ] **`remove_worktree` and `merge_worktree` read the worktree on the UI thread.** Left by card 35
  (`Workspace::worktree_state`, `crates/templum/src/workspace/ops.rs`): a few git commands (`worktree list`,
  `status`, `rev-list`, `diff`) run before the call answers or the inbox shows it; in a big repository the window may
  stall for a moment. Fix: read it on the background executor, the call an `Outcome::Later` that opens the request
  once read.
- [ ] **A repeating schedule drifts later.** Left by the final review of card 35
  (`look_schedules`, `crates/templum/src/workspace/schedules.rs`): its next time is `now + every`, so each run comes
  up to 15 s (a look) plus a timer's lateness after the last; over 7 days an `every: 1440` at 09:00 lands a minute or
  two later. Fix: `next + every`, falling back to `now + every` only when that time was missed (asleep).
- [ ] **`next_at` has no test.** Left by the final review of card 35 (`schedules.rs`): `at`'s today-or-tomorrow and
  its daylight-saving gap and repeated hour are untested. Fix: pass `now` and the time zone in, and test with
  `chrono::FixedOffset` and a zone with a DST change.
- [ ] **"stop_agent or close_block it first" can't be followed with stop_agent.** Left by the final review of card 35
  (`worktree_for`, `crates/templum/src/workspace/worktrees.rs`): a terminal whose shell stays in the worktree after
  `stop_agent` still works in it (`works_in` reads its folder), so the agent that follows the advice is refused again.
  Fix: say "close_block it first", here and in agent-tools.md.
- [ ] **`stop_agent` reads the process table on the UI thread.** Left by the final review of card 35 (`stop_in`,
  `crates/templum/src/workspace/ops.rs`): up to three full refreshes, one reading every process's environment, before
  it answers; on a busy machine the window may hitch. Fix: one refresh, on the background executor.
- [ ] **A schedule's text isn't flattened where the person reads it.** Left by the final review of card 35
  (`schedule_lines`, `schedules.rs`): the clock's tooltip and `whoami` show an agent's text as written, invisible or
  direction characters included. Fix: `bus::flat` it there.
- [ ] **A role over another agent types the role's model and words for it.** Left by the final review of card 35
  (`Role::spawn`, `crates/templum/src/config.rs`): `spawn_agent(role: "reviewer", agent: "codex")` with a Claude Code
  role still gets the role's `--model` and `args`, which Codex may refuse; `fork` with a role over another agent is
  refused as "leave agent out". Fix: apply a role's model, effort and args only to its own agent.
- [ ] **`stop_agent`'s count includes processes it couldn't end.** Left by the final review of card 35 (`stop_in`):
  a root child under `sudo` or a zombie counts as "ended". Fix: count what `terminate` signalled.
- [ ] **A repeating schedule to a busy agent says it didn't go every time.** Left by the review of card 35's fixes
  (`look_schedules`, `crates/templum/src/workspace/schedules.rs`): an `every: 1` to an agent on a long task types
  `[schedule s3 to k9x1d didn't go: k9x1d is on a task (t7): …]` into the caller every minute until the task ends.
  Fix: keep the last refusal told on the `Schedule`; tell one only when it differs from that one, and forget it once
  the schedule goes.
- [ ] **`remove_worktree` says it couldn't remove a worktree it removed.** Left by the review of card 35's fixes
  (`Repo::remove_worktree`, `crates/templum-git/src/repo.rs`, and `worktree_remove` in
  `crates/templum/src/workspace/ops.rs`): on a branch merged into the person's but behind its upstream, `git worktree
  remove` goes, then `git branch -d` refuses ("not fully merged"), and the agent hears "Couldn't remove x: …" while
  the worktree is gone and only its branch stays. Fix: when the branch alone stays, answer "Removed the worktree x;
  its branch stays: <git's line>".
- [ ] **Pi and Grok Build can't be forked.** Left by card 35 (`fork_id` in `crates/templum-terminal/src/agent.rs`):
  `spawn_agent` with `fork` from one of them says it can't, though `pi --fork <id>` and `grok --resume <id>
  --fork-session` exist, since Templum couldn't find the fork's own session to resume it after a restart. Fix: give a
  fork a `--session-id` Templum picks, once each agent takes one with its fork flag.
- [ ] **An agent in WSL gets none of Templum's server, hooks or status line.** `Workspace::gives`
  (`crates/templum/src/workspace/usage.rs`) gives nothing when the block's shell is `wsl.exe`: Templum's files are
  Windows paths, the bus is Windows' loopback, and the block's variables (`TEMPLUM_BUS`, `TEMPLUM_TERMINAL`, the
  `TEMPLUM_*_ARGS`) don't cross into WSL. A Claude Code set to WSL has no Templum tools, no plan limits and no hooks.
  Fix: in a WSL block, list those variables in `WSLENV` (`/p` for paths), type Claude Code's and Codex's flags with
  their files as `/mnt/c/…` paths, and run the MCP bridge as `templum.exe` through WSL's interop, which reaches
  Windows' loopback.
- [ ] **Gemini CLI and Qwen Code ask before `reply`.** Left by the plan of card 69
  (`docs/specs/agent-protocol.md`, 9): every task now asks for `reply`, and these two show "Allow execution of MCP
  tool reply?" until the person allows it always; a subagent there waits on its own dialog. They let a whole server
  run unasked (`trust: true`), never one tool, and Templum pre-allows only `reply`. Fix: a per-tool rule if either
  gains one (Gemini CLI's policy files), written into the extension and Qwen Code's `mcp.json`.
- [ ] **A subagent with Templum's tools that never replies and never goes still is never told.** Left by the plan of
  card 69 (`went_quiet`, `crates/templum/src/workspace/delegate.rs`): a screen that redraws every second (a clock, a
  counter) never stays still for 5 looks, so neither quiet nor an end comes; a waiting master hears its call's
  timeout, one not waiting hears nothing. Fix: once the task is past its timeout, tell its master once, as quiet does.
- [ ] **A nested agent's server counts as the block's.** Left by the plan of card 69 (`Workspace::has_tools`,
  `crates/templum/src/workspace/bus.rs`): a `claude -p` an agent's shell tool runs inherits `TEMPLUM_TERMINAL`, so
  while it runs its `templum mcp` makes `list_blocks` and `delegate` say the block has Templum's tools. Fix: count a
  connection only while the snapshot's agent process is its ancestor.

- [ ] **A subagent whose own task never ends can't end its task by a turn's end.** Left by the plan of card 69
  (`Workspace::hands_out`, `crates/templum/src/workspace/delegate.rs`): while a task the subagent handed out stays in
  its pane (also past its timeout, and a spawn's prompt included), the end of its turns ends nothing of its own task,
  and a still screen tells nothing. Its master hears its call's timeout, or nothing if it didn't wait, until the
  subagent replies, its agent leaves or its block closes. A sub-task whose agent went quiet (Templum's tools, no
  hooks) doesn't count: `hands_out` skips it once its master was told. Fix for the rest: ignore a handed task past
  its timeout in `hands_out`, and tell the master once, as quiet does.
- [ ] **A second `reply` naming the former asker's block is typed to it.** Left by the review of card 69
  (`Workspace::reply_to`, `crates/templum/src/workspace/delegate.rs`): after a task ended, `reply` with the block of
  the agent that handed it matches no task, so it is a message and is typed into that agent as `[reply from …]`; only
  the form without a block says "You are on no task". A master sees one message after the end. Fix: remember the last
  asker of the pane's task for a few seconds and refuse a `reply` to it.
- [ ] **`templum reply <text>` with no block isn't accepted.** (`call_of`, `crates/templum-mcp/src/cli.rs`) The usage
  line reads `reply [<block>] <text>|- [--json]`, but a block-less reply is only `reply -`; `templum reply all done`
  takes `all` for a block. A person sees a refusal from Templum ("all isn't a block"). Fix: say `reply [<block>]
  <text>|-` as `reply (-|<block> <text>|<block> -)`, or take a block-less text when the first word is no block id.
- [ ] **`delegate` with `wait` doesn't tell that the agent has no Templum tools.** (`hand_task`,
  `crates/templum/src/workspace/delegate.rs`) The sentence is added to the immediate answer only; a master that waits
  hears the task's end. Fix: put it first in the end's text when the agent had no tools at hand-over.
- [ ] **`has_tools` ignores the connections switch.** (`Workspace::has_tools`, `crates/templum/src/workspace/bus.rs`)
  An `initialize` while connections are off isn't counted, and entries from before stay when they are turned off: after
  turning them on again an agent whose server started meanwhile reads "no Templum tools" until it restarts, and one
  that had tools keeps them while its MCP `reply` is refused. Fix: always record `initialize`, and make `has_tools`
  false while connections are off.
- [ ] **A server that goes away after its task went quiet leaves the task open.** (`look_at_agents`,
  `crates/templum/src/workspace/activity.rs`) The still-screen check runs once, when the state goes from Working to
  Idle; the task then waits for the screen to move and go still again. A master sees nothing until its timeout. Fix:
  in `closed`, end the task of a terminal that lost its last connection and whose state is Idle, as a still screen does.
- [ ] **A subagent's turn can end before it sees the notice of its sub-task's end or quiet, and that ends its task.**
  Left by the second review of card 69 (`hands_out`, `crates/templum/src/workspace/delegate.rs`; `agent_event`,
  `crates/templum/src/workspace/bus.rs`): B hands C a task; C answers it, or goes quiet, while B is still in its turn,
  and B's turn ends before it reads the `[task t2 …]` notice. The sub-task no longer counts, so that turn's end ends
  B's own task, and its master gets B's last words ("…I handed it to C; waiting.") as the answer. Rare with Claude
  Code. Fix: in `hands_out`, keep counting a sub-task whose end or quiet notice to B is queued, or was typed during
  B's current turn, until B's next `Submitted` or `Working` event after it.
- [ ] **A `reply` that a quiet subagent's waiting call takes doesn't start its wait over.** Left by the fourth review
  of card 69 (`reply_to`, `crates/templum/src/workspace/delegate.rs:891-908`): when the quiet C (no hooks) is itself in
  `wait_for_idle(B)` on its master, B's `reply` to C answers that call; nothing is typed into C, so `asker_wrote` isn't
  called and `quiet` stays true. B's turn ending with "waiting for C" then ends B's own task, and its master gets B's
  interim words as the answer. Rare: C has no hooks, waits on its own master, and is still during that call. Fix: call
  `asker_wrote(&asker, from)` in the waits branch too, and drop the spec's "not one that a waiting call takes".
- [ ] **A dialog over an agent's input isn't dismissed before its task.** Left by card 118 (`EnterWait`,
  `crates/templum/src/workspace/delegate.rs`): Cline's "Introducing Cline Desktop – Press Enter to open, any other key
  to close" takes the paste's first key, or ignores the paste. The master now hears that the agent didn't take the task
  (its screen never showed it, or two Enters sent nothing), and Esc then delegating again works. A master sees one
  failed `delegate` before it. Fix: a per-agent pattern of such dialogs in the agent table (`templum_terminal::Agent`),
  and Esc pressed once before the paste when the screen shows one.
- [ ] **A `reply` typed into an agent says "Typed into" before its Enter went.** Left by card 118 (`reply_to`,
  `crates/templum/src/workspace/delegate.rs`): with no call of the agent waiting, the reply is typed and the call
  answers at once; it gets its second Enter like a task, but one that still isn't sent is lost silently. A master sees
  "Typed into k9x1d." for a reply sitting in that agent's input. Fix: answer `reply` with `Outcome::Later` on what
  `send_task` says, as `hand_task` does.
- [ ] **On Windows a second Enter can still answer a dialog of an agent without hooks.** Left by card 118's review
  (`EnterCheck::look`, `crates/templum/src/workspace/delegate.rs`): there every paste goes as keys (the console hands
  keys on, bracketed or not), so a screen the first Enter changed and that stands still for 3 s gets a second Enter.
  Aider asks at once "Add file to the chat? (Y)es/(N)o … [Yes]" for a task naming a file of its repository or a URL:
  the person sees the file added to the chat without saying yes. Fix: the second Enter into a changed screen only for
  the agents known to take a burst of keys for a paste (Codex, Copilot CLI: a flag in the agent table,
  `templum_terminal::Agent`), not for every agent on Windows.
- [ ] **Two Enters taken for new lines count as a task taken by an agent without hooks.** Left by card 118's review
  (`EnterCheck::look`, the `(true, _, false) if self.moved` arm, `crates/templum/src/workspace/delegate.rs`): a paste
  that went as keys (Codex or Copilot CLI on Windows, before their hooks spoke) can take both Enters for new lines;
  each changed the screen, so the task counts as taken while it sits in the input. The master hears "Handed to k9x1d
  as task t7." and waits for an answer that never comes, until the still screen or the timeout ends the task. Fix:
  after a second Enter into a paste that went as keys, take a screen whose only change is one more row in the input
  for "typed but not submitted".
- [ ] **A cancelled `delegate` loses "didn't take the task".** Left by card 118's review (`Outcome::Later` in
  `serve`, `crates/templum/src/workspace/bus.rs`; `cancel`, `waits.rs`; `not_taken`, `delegate.rs`): a `delegate`
  without `wait` now answers only once the agent took the task; when its client gives up on the call meanwhile (Esc
  in Claude Code), `cancel` only drops waits, so the job still answers the cancelled request, and `not_taken`, seeing
  a call still to tell, types no notice. The master sees nothing about a task its agent never took. Fix: keep the
  `Later` job's request id with it, and drop the job on `notifications/cancelled`: `not_taken` then finds `told`
  cancelled and types the notice.
- [ ] **A notice or a reply its agent's person typed over before its Enter stays unsent.** Left by card 118's review
  (`Step::Typing` in `send_task`, `crates/templum/src/workspace/delegate.rs`): the Enter no longer goes once the
  person typed into the agent after the paste, so their text isn't sent with it; a task then ends untaken and its
  asker hears so, but a notice or a reply (no task) just stays in the input with the person's text, and nothing types
  it again. Rare: the person must type in the 150–300 ms between the paste and its Enter (a notice already waits until
  they stopped typing for 3 s). Fix: put a notice back in `notices` (its text cleared from the input first is the
  hard part), and answer a reply with what `send_task` says, as the reply minor above asks.

Left by kanban card 127 (Claude Code's subagents, `docs/specs/claude-subagents.md`) and its review; line numbers are
those of `development` at f8c3ec9.

- [ ] **A task whose turn resumes a subagent started before it is answered too early.** `Handed::subagents`
  (`crates/templum/src/workspace/mod.rs`) keeps the subagents there when the task was taken, and a turn's end with
  only those left answers it (`turn_ended`, `crates/templum/src/workspace/bus.rs`); one of them resumed for the task
  (`SendMessage`, a `SubagentStart` with its id) still counts as not the task's. A master gets the interim message
  ("I asked the reviewer again…") and the real answer comes to no one. Fix: on `SubagentStarted` for an id among a
  taken task's `subagents`, take it off that list.
- [ ] **Two subagents' dialogs at once: answering the later one turns the parent back to Working.** `asked_by`
  (`Pane`) holds only the subagent that asked last; once its tool ends, the parent's dot leaves yellow although the
  first one's dialog is still open in Claude Code's screen. A person sees Working and doesn't look. Fix: keep the ids
  of every subagent asking (a set), and leave the wait when it is empty.
- [ ] **A nested subagent's block opens beside the main Claude Code, not beside its own parent.** `open_subagent`
  (`crates/templum/src/workspace/subagents.rs`) places every subagent by the terminal its hook came from; the hooks
  don't say which subagent started a nested one. A person sees a flat column instead of a tree. Fix: read
  `spawnDepth` and `toolUseId` from the subagent's `meta.json` (observed, not documented) and place it beside the
  block of the subagent whose transcript holds that `tool_use`.
- [ ] **In grid mode every subagent takes half of its parent's tile.** `open_subagent` focuses the parent before
  opening, as `spawn_agent` does, so the grid splits the parent's tile each time; ten subagents leave slivers. Fix:
  in grid mode, split the last subagent's tile instead, or open them off the grid.
- [ ] **A Claude Code moved to another workspace leaves its subagents' blocks behind.** Moving a block
  (`take_block`, `crates/templum/src/workspace/blocks.rs`) doesn't take the blocks of its subagents along; they stay,
  unwired, until they finish and close. Fix: move them with it, wired again.
- [ ] **Esc during a foreground subagent may leave its parent Working.** With fork mode off (or Claude Code before
  2.1.232) a subagent runs in the foreground, inside its parent's `Agent` call; an Esc then ends the parent's turn
  (`PostToolUseFailure` with `is_interrupt`, a `Stopped` without an answer), and whether that subagent's `SubagentStop`
  still comes wasn't probed. If it doesn't, the subagent stays at work in `Pane::subagents` and `turn_ended`
  (`crates/templum/src/workspace/bus.rs:414`) only awaits it: the person sees "Working: 1 subagent" until their next
  prompt's `Stop`, and a task handed to it stays open. Fix: probe it (fork mode off, Esc while an `Explore` runs); if no
  `SubagentStop` comes, take off, on the parent's interrupt, the subagents no `Stop`'s `background_tasks` ever listed
  (foreground ones).
- [ ] **A fork's transcript was never probed.** A fork (`subagent_type: "fork"`, `/subtask`) inherits its parent's whole
  conversation; its transcript may start with that history, which `Follower::line`
  (`crates/templum-agent/src/follow.rs:104`) would print as the fork's "Task" (the parent's first prompt) and its words,
  and title the block with that first prompt. A person would see a block that starts with someone else's conversation.
  Fix: probe a fork's `agent-<id>.jsonl`; if it holds the history, print from the `user` line that follows the
  `tool_use` whose id is its `meta.json`'s `toolUseId`, or from the fork's own prompt.
- [ ] **`/clear` greys and closes background subagents' blocks that may still run.** A new session (`Started`,
  `crates/templum/src/workspace/bus.rs:306`) forgets the parent's subagents and marks every block of theirs done
  (`subagents_done`), which closes a minute later; whether Claude Code's background subagents outlive `/clear` or
  `/resume` wasn't probed. If they do, the person sees a working subagent's block turn grey and close while it still
  writes its transcript. Fix: probe it; if they survive, keep those blocks open (still following) on `Started`, and mark
  them done only on their `SubagentStop` or the session's `SessionEnd`.
- [ ] **No bench was run with subagent blocks on the canvas.** The study asks for `--bench-mixed` with a dozen of them;
  card 127's build ran none (its blocks are terminals that run nothing, `Terminal::viewer`, and the bench opens a
  window). Fix: add a dozen viewers fed a synthetic transcript to `--bench-mixed`'s cockpit and paste its line in the
  pull request.
- [ ] **Only the cursor's row of a suggestion is left out of a screen's text.** `suggestion`
  (`crates/templum-terminal/src/terminal.rs:424`) is the faint run from the shown cursor on, on its row; a suggestion
  long enough to wrap onto the rows under it keeps those in what `read_terminal` and the other answers show, where an
  agent may still take them for its person's text. Claude Code's suggested next prompt, shown after a turn, wasn't
  probed (it takes a paid turn): only its placeholder and Codex's were. Fix: probe a long suggestion; if it wraps,
  leave out the faint rows right under it too.
- [ ] **A request's age in the inbox stays as it was drawn.** `inbox::list` (`crates/templum/src/inbox.rs`) tells "2m"
  from the clock when it draws, and nothing redraws a window at rest, so a row says "now" until something else
  redraws it (a click, an agent's output). Fix: while the inbox is open, a timer that notifies once a minute.
- [ ] **A text selected in the inbox loses the keyboard when any wait ends.** `inbox_keeps_keys`
  (`crates/templum/src/workspace/inbox.rs`) gives the keyboard back to the inbox when something inside the popup that
  is neither the inbox nor a live request's field has it; a plan's Markdown (a selectable gpui-kit `TextView`) is such
  a thing, so a line selected to copy loses the keyboard when another agent's `wait_for` ends, and Ctrl+C copies
  nothing. Fix: refocus only on the focus handles of the fields that left (`answer`/`closed`/`cancel` hold the wait:
  make `inbox_refocus` a list of its fields' handles), with a test.
- [ ] **A notice and the person's first key can share an Enter.** `send_task`
  (`crates/templum/src/workspace/delegate.rs`) types a notice, then waits for its paste to show before its Enter
  (`Step::Typing`); a person who starts typing in that moment gets no Enter, and the paste stays in the input with their
  keys (docs/specs/notice-batching.md, Non-goals). Fix to consider: queue the notice again, accepting a duplicate if the
  person sends both.
- [ ] **A draft that begins with a new line reads as an empty input.** `input_empty`
  (`crates/templum/src/workspace/delegate.rs`) looks at the last row that opens with a prompt marker; a message the
  person began with Shift+Enter has a bare marker row and its text under it, so once their last key is TYPING old and
  the keys can't tell, the notice goes in over it. Fix: read the rows under the marker up to the input's rule or box
  edge too.
- [ ] **A draft emptied under a tall Claude Code footer never ends from the screen.** `INPUT_ROWS` = 6
  (`crates/templum/src/workspace/delegate.rs`, `input_empty`) reads the bottom six rows; with a status line of 4 or more
  lines the input row sits above them, so a draft emptied with Backspace or Esc (or one an Up, Down, Tab or Ctrl+Y left
  over an empty input) is never seen empty, and the notices wait for the person's next message (with hooks, its
  `Submitted`). Fix to consider: look for the input between the last two rule or box rows instead of a fixed count.
- [ ] **Pi, OpenCode and Kilo Code get no instructions.** Only the hooks of Claude Code, Codex, Gemini CLI, Copilot CLI
  and Qwen Code take context from `templum hook` at a session's start (`session_start` and `start_output`,
  `crates/templum-mcp/src/hooks.rs`); Pi's extension returns nothing from `session_start`, and OpenCode's and Kilo
  Code's plugin has no session-start hook. A person sees those agents ignore Settings ▸ Instructions. Fix: Pi's
  extension prepends them on `before_agent_start`; OpenCode's and Kilo Code's plugin through its system-prompt hook,
  both asking `templum/instructions` as the hook does.
- [ ] **Qwen Code's and Copilot CLI's instructions are unchecked.** Their `hookSpecificOutput` and `additionalContext`
  shapes (`start_output`, `crates/templum-mcp/src/hooks.rs`) come from their docs only: Qwen Code isn't installed here
  and Copilot CLI wasn't run by hand (no sign-in). A person would see an agent that ignores its instructions. Fix: run
  each with an instruction written and ask it what it was told, then change the shape if it differs.
- [ ] **Claude Code's subagents get no instructions.** `session_start` is the session's: a subagent's hooks name an
  `agent_id`, and `SubagentStart` takes `additionalContext` too. A person sees a subagent unaware of the instructions
  the main session follows. Fix: answer `templum/instructions` at `SubagentStart` and print its `additionalContext`.
- [ ] **A resume with no instruction applying leaves an earlier set in the conversation.** The hook prints nothing when
  none apply (`hook`, `crates/templum-mcp/src/cli.rs`), so a conversation resumed after the person turned every
  instruction off keeps the ones its first start gave. A person sees an agent still following an instruction they
  deleted. Fix: the hook knows the `source`; on a resume answer a short text saying none apply any more.

## Placing new blocks

Left by kanban card 53 (blocks Templum places line up, 48 apart); line numbers are those of `development` after it.

- [ ] **A new editor moves the view even when its folder dialog is cancelled.** `new_editor`
  (`crates/templum/src/workspace/files.rs:23`) finds the editor's place, and glides the view there, before the folder
  dialog opens; cancelling it leaves the view moved for an editor that never opens. Fix: find the place, and glide,
  in the dialog's answer (`ask_editor_folder`'s callback) instead.
- [ ] **A column of subagents stops being one once the person nudges a block in it.** `free_spot`
  (`crates/templum-canvas/src/canvas.rs`) counts a wired block as on a side's line only when its edge is within half a
  unit of the line; a subagent dragged a few units off it, the next one goes the way of the connections instead (still
  48 from everything, but maybe not under the others). Fix: count a block whose edge is within the snap distance
  (`SNAP_PX`, in world units) of the line, and line the new one up with it.

## Keeping the canvas

Left by the investigation of canvases that came back empty on Windows and macOS (kanban card 41); line numbers are
those of `development` at de530a6.

- [ ] **A shell the system kills takes its block, and the next save, with it.** The frame closes every terminal whose
  shell ended (`crates/templum/src/workspace/render.rs:303`) without looking at how it ended: one killed while
  Templum runs (out of memory, a session ending in an order Templum didn't set) closes like `exit`, the 5-second
  snapshot writes the canvas without it, and its agent isn't resumed at the next start. Fix: keep the block when
  alacritty's `ChildExit` says the shell was killed (Unix: SIGHUP, SIGTERM or SIGKILL; Windows: `0xC000013A`,
  `STATUS_CONTROL_C_EXIT`), and save it from what it last knew (`pane.seen`), not from its dead pids.
- [ ] **Cmd+W pressed again and again closes every terminal.** `close_terminal`
  (`crates/templum/src/workspace/blocks.rs:824`) hands the keyboard to the block on top, as a terminal app's next tab,
  so the macOS habit of Cmd+W to close the window (Ctrl+Shift+W, Windows Terminal's for a tab) closes the terminals
  one by one, a busy one after "Close the terminal?", and the next start shows an empty canvas. Ctrl+Z brings them
  back, but few know. Fix, a product choice: ask before closing the last terminal, or leave nothing focused after a
  close by shortcut.

Left by the review of its fixes; line numbers are those of `development` at 94edde0.

- [ ] **A second Templum still shares `config.toml`.** It keeps nothing of the data folder, but its config folder is
  the first one's (`crates/templum/src/main.rs:92`), as with `--bench`: a setting changed in either window is written
  by both, and the last to save wins, so a change made in the first can be undone by the second closing. Fix: the
  second (and `--bench`) reads `config.toml` but never writes it, with a notice when a setting is changed there.
- [ ] **Moving a "The shell ended" block to another workspace stops that one saving too.** `keep_dead_starts`
  (`crates/templum/src/workspace/persist.rs:144`) freezes every workspace that holds a restored terminal whose shell
  ended as it started, wherever it is now: moved there, it freezes a workspace whose own terminals all started, and
  the notice "Some terminals couldn't start…" shows again. Fix: clear `Pane::restored` when a block moves to another
  workspace (`move_blocks`, `spaces.rs:298`), or remember the workspace it was restored in.
- [ ] **An agent quit right after its resume comes back at the next start.** `resuming`
  (`crates/templum/src/workspace/mod.rs:692`) is let go only by a snapshot that finds an agent in the block: one
  quit within the 5 seconds before the next snapshot is never seen, so for a minute (`STARTS_WITHIN`) the snapshots
  still save it, and quitting Templum then resumes it again at the next start. Fix: let go of it also when the
  shell's integration reports a command that finished (`Status::finished` grew) since the line was typed.
- [ ] **The test of a shell that ends as it starts runs only on Unix.**
  `a_restored_shell_that_ends_at_once_leaves_the_saved_canvas_alone`
  (`crates/templum/src/workspace/tests/persistence.rs:321`) and its siblings use `sh -c "exit 1"`, so Windows, where
  ConPTY reports the exit its own way, isn't checked. Fix: a `cmd /c exit 1` variant under `#[cfg(windows)]`.
- [ ] **The second-Templum wiring has no test.** `main` (`crates/templum/src/main.rs:94-106`) turns a held lock into
  a Templum without a data folder and its notice; only `session::lock` is tested. A change there that, say, keeps
  the data folder on `WouldBlock` would pass CI. Fix: move the choice into a function of `session` (the lock's result
  in, the data folder and whether to warn out) and test it.

Left by the third review of its fixes; line numbers are those of `development` at eafb5ed.

- [ ] **Typing ahead turns the protection off.** A key pressed in a restored terminal before its shell ends as it
  starts sets `had_input` (`crates/templum/src/workspace/persist.rs:550`): Enter pressed while the window comes up,
  or a `templum send` from an agent, makes the shell look ended by the person, so its block closes and the next
  snapshot saves the canvas without it. Fix: don't count input before the shell first draws a prompt (with shell
  integration) or within its first ~500 ms.
- [ ] **A real second Templum takes 3 s to show its window.** `session::lock(dir, QUITTING)`
  (`crates/templum/src/main.rs:94-97`) waits for the lock before the window opens, so a second click on the shortcut
  while Templum runs gives no feedback for 3 seconds. Fix: the quitting Templum writes "quitting" into
  `templum.lock` before its quit save; the starter waits only when it reads that, and fails at once otherwise.
- [ ] **A Templum that takes over 3 s to quit still makes the reopened one a second instance.** With a slow quit
  save (Windows), the reopened Templum gives up on the lock after `QUITTING` (`crates/templum/src/session.rs:41`)
  and starts without a data folder, with the notice. Fix: the same "quitting" marker, with no fixed limit while the
  holder says it is quitting.

## Agent accounts

The known limits of [specs/accounts.md](specs/accounts.md#known-limits), and what kanban card 37's run couldn't check
by hand (a second real login, macOS, Windows). Line numbers are those of `development` at 173017d.

- [ ] **An agent that rewrites a shared file by a rename stops sharing it until the next start.** Templum notices a
  link the agent replaced only when the account starts (`apply`, `When::Starting`,
  `crates/templum-usage/src/accounts/folder.rs:246`): until then the two accounts' settings drift apart. Fix: also
  check the account's links at each snapshot of a block of it.
- [ ] **A card for a deleted account keeps its id.** `delete_account` (`crates/templum/src/workspace/preferences.rs:152`)
  leaves `agent:claude2` on its cards: no block takes them until the person changes them. Fix: offer to give them
  to its agent ("Any Claude Code") in the delete question.
- [ ] **Claude Code's synced skills and plugins are shared too.** Two accounts sharing `skills` or `plugins` share
  `skills/synced/` and `plugins/synced/` (the claude.ai account's), and each sign-in syncs its own over them
  (`KINDS`, `crates/templum-usage/src/accounts.rs:63`). Fix: link the items' entries one by one, leaving `synced/`
  each account's own.
- [ ] **Some logins are every folder's.** Claude Code's Console sign-ins without a key (`~/.config/anthropic`) and
  Codex's MCP servers' OAuth logins (the keyring, by server) are one for every account; nothing in the dialog says
  so. Fix: say it in their agents' notes (`KINDS`, `crates/templum-usage/src/accounts.rs`).
- [ ] **Gemini CLI reads `advanced.autoConfigureMemory` from the account's folder, not its `.gemini/`.** Its issue
  27421: the setting saved in an account's `.gemini/settings.json` is ignored. Fix: none on Templum's side until
  Gemini CLI fixes it.
- [ ] **Crush's own folder is Linux's default.** `own: ".local/share/crush"` (`crates/templum-usage/src/accounts.rs:284`)
  misses `XDG_DATA_HOME` and Windows' `%LOCALAPPDATA%\crush`, so a Crush account chosen on that folder isn't
  refused as Crush's own. Fix: `own_folder` follows them for Crush.
- [ ] **Not checked by hand: two real Claude Code logins.** No second login was made in card 37's run: each account's
  own limits in the meter, `/status` naming the other account in a claude2 block, and claude2 resuming one of
  claude's sessions with `/resume` are covered by tests only (`crates/templum/src/workspace/tests/settings_panel.rs`,
  `persistence.rs`). Fix: check them by hand with two accounts, and on macOS that a claude2 login lands in the
  Keychain under its folder's own name.
- [ ] **Not checked by hand: Windows.** Junctions for folders and the Developer Mode refusal for files
  (`files_share`, `crates/templum-usage/src/accounts/folder.rs:291`) are tested by CI's `#[cfg(windows)]` test only.
  Fix: share `skills` from a Windows account by hand and check the agent reads it.
- [ ] **Not checked by hand: Codex with its own login.** codex2 logged in with its own account (`codex login` in its
  block) wasn't run. Fix: check by hand that its limits row is its own.

Left by the final review of kanban card 37; line numbers are those of `development` at 65e2868.

- [ ] **A source folder Templum makes for a share isn't private.** `apply_path`
  (`crates/templum-usage/src/accounts/folder.rs:243`) makes a missing item folder of the source (another account's,
  or the agent's own) with `create_dir_all`: 0755 on Unix, where the account's own folder is 0700. Fix: `make_dir`
  there too.
- [ ] **The menu switch of an account rewrites its entry.** The row's switch
  (`crates/templum/src/settings/accounts.rs:103`) saves through `save_account`: it applies the folder's choices as
  Save does (keeping aside a copy the agent wrote) and writes its command again when its text changed. Fix: edit only
  `agents.<id>.show`.
- [ ] **`Report::theirs` is filled and never read.** `crates/templum-usage/src/accounts/folder.rs:37`: the dialog reads a
  folder with `read` instead. Fix: delete it and its assertions.
- [ ] **Windows' test pins deleting only.** `a_folder_is_a_junction_and_a_file_needs_the_privilege`
  (`crates/templum-usage/src/accounts/folder.rs:627`) never makes an item own again nor re-points a junction, so
  whether `at` takes a junction's target (`\\?\`-prefixed) for Templum's is untested there. Fix: `apply(&[])` after,
  then assert the junction is gone and its target's file stays.
- [ ] **A deleted account's name can be typed again.** `save_account`
  (`crates/templum/src/workspace/preferences.rs:78`) accepts the name of an account deleted before: a stopped
  workspace's block of the old one then comes back as the new account's, typing `--resume <its old session>` there,
  where the spec wants it plain. Fix: remember the ids deleted (a list in config.toml) and skip them.
- [ ] **Gemini CLI's own folder is two answers.** `Dirs::of` (`crates/templum-usage/src/lib.rs:176`) ignores
  `GEMINI_CLI_HOME`, `Kind::own_folder` follows it, so with it set the extension notice and the accounts disagree on
  where Gemini CLI's own folder is. Fix: `Dirs::of` takes Claude Code's, Codex's and Gemini CLI's from
  `accounts::kind(..).own_folder`.

Left by the data-safety fixes after that review; line numbers are those of `development` at e54d463.

- [ ] **Generate and the `/usage` probe run an account's agent without its choices.** Generate's environment
  (`CommitAgent::env`, `crates/templum/src/agents.rs:767`) and the probe (`claude_poll_jobs`,
  `crates/templum/src/workspace/usage.rs:399`) name the account's folder without `AgentEntry::apply`: the agent makes
  a never-opened account's folder itself, at 0755 and without its links, and a probe still running when
  `delete_account` (`crates/templum/src/workspace/preferences.rs:154`) deletes a folder can make it again. Fix: apply
  the account's choices before either runs, and wait for (or stop) its probe in `delete_account`.
- [ ] **A Templum started in an account's block refuses that account's chosen folder.** `check_account`
  (its `own_folder` call, `crates/templum/src/config.rs:631`) takes the inherited `CLAUDE_CONFIG_DIR` for Claude
  Code's own folder, so the account whose folder it names is "claude's own folder" and config.toml is refused at that
  start, while from claude2's block a new account on `~/.claude` is accepted: the same login as `claude` (no files
  move). Its plain blocks inherit the variable too, so `claude` typed there runs on the account. Fix: drop an agent's
  variable from Templum's environment at start when it names one of its accounts' folders, as `own_state_in`
  ignores it.
- [ ] **A loop across items applies one account before the other.** `upstream` (`crates/templum/src/agents.rs:248`)
  orders whole accounts: with claude3's `skills` from claude4 and claude4's `settings` from claude3, one of them is
  applied before the other's item is linked, which then starts as a real empty folder. Fix: order by item, or
  refuse a loop across items in `check_account`.
- [ ] **Deleting an account on a linked or chosen folder leaves Templum's links in it.** `delete`
  (`crates/templum-usage/src/accounts/folder.rs:340`) removes only the folder's link when it, or Gemini CLI's
  `.gemini` in it, is a link, and `delete_account` keeps a chosen folder whole, so the links Templum made there for
  missing shared items stay: with claude5's folder a link to `~/.claude` sharing `agents` from claude3, `claude`
  keeps claude3's agents after claude5 is deleted, and a link to nothing once claude3 is. Fix: pass `delete` the
  account's sources and remove there only the links `at` says are Templum's.
- [ ] **A `..` after a link that follows a folder not made yet is still read as text.** `resolved`
  (`crates/templum/src/config.rs:668-679`) follows only the part of the path that exists, so with `~/w/l` a link to
  `<data>/accounts/claude3`, the folder `~/w/new/../l/../claude9` resolves to `~/w/claude9` and is accepted; once
  `new` is made, the system puts it in `<data>/accounts/claude9`, beside another account. Fix: resolve one part at a
  time, canonicalizing each new part once it exists and dropping the last on `..`.
- [ ] **Deleting an account whose Templum folder is a link is refused for an account inside the link's target.**
  `delete_account` (`crates/templum/src/workspace/preferences.rs:163-170`) compares resolved paths, so with claude2's
  folder a link to `~/.claude` and claude5 on `~/.claude/work`, deleting claude2 says "Claude Code 5's folder is in
  Claude Code (claude2)'s: change it first", though deleting would only remove the link. Found by reading. Fix: skip
  the check when the account's folder is itself a link.

Left by kanban card 65 (accounts by name, each a command in every shell); line numbers are those of `development` at
db2baa3.

- [ ] **A `.cmd` passes the person's words through cmd.** On Windows, `claudinho.cmd` ends in `claude %*`
  (`files`, `crates/templum-usage/src/accounts/command.rs:177`): a word with `"`, `%` or `^` may reach the agent
  changed, as with npm's commands. Fix: have the `.cmd` call a PowerShell script that passes its arguments through.
- [ ] **A folder outside home with non-ASCII letters, or under home in another case or with AppData redirected, is garbled in the `.cmd`.** `in_cmd`
  (`crates/templum-usage/src/accounts/command.rs:162`) writes only the home prefix as `%USERPROFILE%`; the rest of the
  `.cmd` is UTF-8, which cmd reads in the console's code page (850, 437), so a folder chosen as `D:\Contas\João` (and
  an absolute program path with such letters, on the `.cmd`'s second-to-last line) reaches `CLAUDE_CONFIG_DIR` as `JoÃ£o`: a
  different folder, with no login. What a person sees: the account asks to log in again when run from cmd or
  PowerShell. Fix: write `chcp 65001 >nul` before the `set` (and the program's line) and the code page back after.
- [ ] **A program installed later with the account's name is told only at the next start.** `make_command`
  (`crates/templum/src/agents.rs:515`) looks for the id as a program at each start and each change of the agents
  (`sync_commands`), and then the row says "No command: claudinho is already a command: …"; until then nothing says
  so. Templum's own file stays in the commands folder and the account stays in `TEMPLUM_ACCOUNTS` (`accounts_env`
  looks for no programs), so its function still runs in Templum's terminals; in any other, the order of the PATH
  decides. Fix: look again when the PATH changes, and remove the file or say on the row that it is shadowed.
- [ ] **An account taken out of config.toml by hand keeps its command.** `sync_commands`
  (`crates/templum/src/workspace/preferences.rs:149`) only writes: the file, which still opens the account's folder,
  stays in `~/.local/bin` until the person deletes it. Fix: at start, remove the commands that carry this data
  folder's marker and whose id is no account.
- [ ] **A block where `claudinho` ran stays the account's after the agent quits.** `Space::see`
  (`crates/templum/src/workspace/mod.rs:877`) changes `Pane::account` only when it finds an agent with its variable:
  once it quits, the block keeps the title and the Delete wait until another agent is found there. Fix: forget the
  account when the agent that gave it is gone and the block's shell has no variable naming one.
- [ ] **A command made while `~/.local/bin` isn't on the PATH works in other terminals only once the person adds it.**
  `path_note` (`crates/templum/src/settings/accounts.rs:95`) says so under Accounts and in the dialog, with the line
  to add; nothing checks that the person did. Fix: offer to add it to the shell's startup file, in a marked block.
- [ ] **A dialog opened while the shell's PATH is still being read has no PATH note.** `path_note` waits for
  `shell_path_pending` to be over and the dialog only redraws when something changes in it, so one opened in the
  first seconds shows the note after the next key. Fix: redraw the dialog when `read_shell_path` finishes.
- [ ] **The Accounts heading looks at the PATH at every frame.** `path_note` canonicalizes each folder of the PATH (on
  Windows it reads the registry too) each time Settings ▸ Agents is drawn. Fix: ask once when the panel opens, and
  again when `read_shell_path` finishes.
- [ ] **An account of an agent whose command is a wrapper runs only its first word.** An agent set to `npx claude-code`
  in Settings ▸ Agents makes a `claudinho` that runs `npx` with the person's words (`AgentEntry::program`). Fix: write
  the whole command, quoted for each shell, in the command and in `TEMPLUM_ACCOUNTS`.
- [ ] **Enter in the Edit dialog closes it without saving.** Only Create answers Enter (in the name field): in Edit
  the dialog's own Enter closes it as Esc does. Fix: answer `Confirm` there with Save.
- [ ] **`SHELL_WORDS` misses builtins whose function would break a shell.** `crates/templum-usage/src/accounts/command.rs:9`:
  a function named `compopt` replaces the builtin bash-completion calls at each Tab; fish's `path`, `math`, `random`,
  `abbr`, `argparse`, `commandline`, `set_color` and zsh's `zparseopts`, `zformat`, `vared`, `float`, `nocorrect` do
  the same, or fail to define and print an error at each shell start. On Windows, PowerShell's aliases (`start`, `h`,
  `r`, `gc`, …) win over the function, and cmd's builtins (`start`, `dir`, `path`, `title`, `color`, `copy`, …) over
  the `.cmd`, so `claudinho` named so runs something else. Fix: add them to the list (the Windows ones only on
  Windows: `copy` or `r` are fine names elsewhere).
- [ ] **A Windows device name is a valid account name.** `aux`, `con`, `nul`, `prn`, `com1`–`com9`, `lpt1`–`lpt9` pass
  every rule (`name_problem`, `crates/templum/src/agents.rs:476`), but Windows can't make `accounts\aux` or `aux.cmd`.
  Fix: refuse them with their own words, on every OS.
- [ ] **A function is defined for an account whose row says "No command".** `accounts_env`
  (`crates/templum/src/agents.rs:631`) leaves out ids that break rules 1, 2 and 4 only: an account written by hand as
  `cc` has the row "No command: cc is already a command: /usr/bin/cc", yet in every Templum terminal `cc` runs its
  agent. Fix: build `TEMPLUM_ACCOUNTS` from the accounts whose `Workspace::commands` entry is `Ok`.
- [ ] **A program written with `~` is never found.** An agent whose command starts with `~/bin/claude` makes a
  command that runs `'~/bin/claude'`, quoted, and a function that runs `"~/bin/claude"`: the shell doesn't expand it
  (`word`, `crates/templum-usage/src/accounts/command.rs:153`). Fix: expand the home folder when writing both.
- [ ] **Edit ▸ Save writes the folder as `~/…`.** `AccountDraft::of` (`crates/templum/src/agents.rs:819`) gives the chosen
  folder through `tilde()`, so `/home/me/x` is saved as `~/x` (the same folder; the spec says "as config.toml has
  it"). Fix: keep the string config.toml had in the draft.
- [ ] **Delete is refused when the command can't be removed.** `delete_account`
  (`crates/templum/src/workspace/preferences.rs:206`) removes the command before the folder: a `~/.local/bin` made
  read-only since Create refuses the deletion, and a folder deletion that then fails leaves the account without a
  command until the next start. Fix: remove the command last and warn instead of refusing.
- [ ] **The dialog re-checks the name three or four times per frame.** `Form::problem`
  (`crates/templum/src/settings/accounts.rs`) looks for the name as a program (on Windows the registry's Path too)
  from the note, Create and its label, also while the dialog slides in. Fix: compute it once per change of the name.
- [ ] **Not checked by hand: a real click-through and typing `claudinho` in a Templum terminal.** Card 65's run could
  not type or click into windows, so the dialog was looked at in screenshots of a dev build (light and dark, with a
  temporary home) and the command was run from plain bash and fish against a fake `claude`; the typed path in a
  Templum terminal is covered by the shell tests (bash, zsh, fish and PowerShell in a PTY). Not seen: the block
  titled "Claude Code (claudinho)" with its status dot moving, with a real Claude Code. Fix: do the spec's "Manual
  (card 65)" once.
- [ ] **Not checked: bash 3.2, Windows' cmd and Git Bash, and the agent's environment on macOS and Windows.** zsh,
  fish and PowerShell were run in the PTY tests, and `cargo clippy -p templum -p templum-usage -p templum-terminal
  --all-targets --target x86_64-pc-windows-msvc -- -D warnings` passes (with a stand-in C compiler: the C dependencies
  need MSVC's `lib.exe`, missing here). Not run: the functions in bash 3.2 (macOS's), the `.cmd` in cmd and the
  command in Git Bash, and the read of an agent's folder variable through sysinfo on macOS and Windows:
  `a_found_agents_folder_variable_is_read` (`crates/templum-terminal/src/terminal.rs:1653`) starts a `#!/bin/sh`
  script, so it runs on Linux only and CI never covers the other two. Fix: run `claudinho` in each by hand, and make
  the test start a copy of `/bin/sleep` named `claude` on macOS and a copy of a Windows program on Windows.
- [ ] **An account opened with Default while Settings ▸ Shell is WSL runs on WSL's own login.** An account's own
  Shell leaves WSL out (`Workspace::agent_shell`, `crates/templum/src/workspace/blocks.rs`), since its folder reaches
  its agent through a variable (`CLAUDE_CONFIG_DIR`…) that doesn't cross into WSL; but with no shell of its own it
  follows Settings ▸ Shell, so when that is WSL the account's block runs WSL's agent on WSL's login, not the account.
  Fix: pass the variable through `WSLENV` with `/p` (and check from Linux that the account folder's links resolve), or
  open an account in the system's default shell when Settings ▸ Shell is WSL.
- [ ] **An account set to WSL says "no shell" on stderr though WSL is installed.** `Workspace::shell_for`
  (`crates/templum/src/workspace/blocks.rs`) prints `templum: no shell "WSL" for <account>: the default one opens` for a
  hand-written `shell = "WSL"` on an account, whose real reason is that an account never runs in WSL
  (`agent_shell` filters it out). Only a person reading stderr sees it; the account opens in the default shell as
  intended. Fix: in `shell_for`, when the label is among `self.shells` and the entry is an account, print that
  reason instead.

## Templum Agent

Left by batch 1 of card 43 (docs/specs/templum-agent.md); line numbers are those of `templum-agent` at its Task 18.

- [ ] **The pill and the chat don't animate their size.** The pill jumps from 300 to 640 px wide
  (`crates/templum/src/chat/pill.rs`, `render`), where the spec grows it over 0.18 s; the chat fades and rises but its
  height jumps with its rows. Fix: a `with_animation` on the width keyed on the wide state.
- [ ] **No less-motion mode.** The spec stills the working line, the glow and the chat's rise when the system asks
  for less motion; Templum reads no such setting anywhere (`crates/templum/src/motion.rs`). Fix: read the system's
  reduced-motion preference once in `motion.rs` and skip those animations.
- [ ] **A reads row doesn't say what it read.** `Row::Reads` (`crates/templum-agent/src/conversation.rs`) keeps a
  count only, so its tooltip ("Read `src/main.rs`") can't be shown. Fix: keep each read's tool and path in the row.
- [ ] **The pill's working words are generic.** `pill::doing` says the tool's title ("Opening an agent…"), where the
  spec says "Opening Codex in api…". Fix: write the sentence from the running action's call.
- [ ] **`overview` doesn't list what waits for the person.** `Workspace::overview`
  (`crates/templum/src/workspace/control.rs`) lists each agent's state, not the approvals waiting. Fix: a last line
  per approval waiting.
- [ ] **A model typed in Settings ▸ Templum Agent that Templum wouldn't type is dropped silently.** `set_model`
  (`crates/templum/src/settings/templum_agent.rs`) leaves the field at the last good model. Fix: show why under the
  field, as the shell's arguments do.
- [ ] **Not checked by hand.** The spec's Manual list on Hyprland (the pill, the chat rising and closing, the
  approval's keys, the glow, light and dark themes, a real Claude Code conversation building criterion 2's
  workspace, Stop during a turn, a restart in the middle) wasn't done: the headless tests and a real Claude Code
  run outside Templum (the command line, `--resume`, the stream, the interrupt) are. Windows' `.cmd` launch of
  `claude` (`crates/templum-agent/src/engine.rs`, `command`) is untested.
- [ ] **`touched` and `denied` take the oldest matching action.** `Conversation::touched` and `denied`
  (`crates/templum-agent/src/conversation.rs`) can give an earlier, finished row of the same call a new call's
  target or Denied. Fix: search newest first, among Running rows only.
- [ ] **A killed engine's secret stays good until the next start.** `kill_engine` (`crates/templum/src/workspace/agent.rs`)
  leaves `TemplumAgent::secret` as it was, so a leftover process of the killed engine still says hello as it. Fix:
  a new secret in `kill_engine`. On Windows the leftover is likely: `claude` runs as `cmd /C claude.cmd`
  (`crates/templum-agent/src/engine.rs`, `command`) and `Child::kill` ends only `cmd.exe`, so after Stop or New chat
  Claude Code's `node` can go on running, its control connection still good, and the chat shows it stopped. Fix
  there too: start `node` with Claude Code's script directly, or put the process in a Job Object that closes with it.
- [ ] **The engine is written to on the UI thread.** `Engine::send` (`crates/templum-agent/src/engine.rs`) writes a
  message (a notice may be 256 KB) straight into the pipe: an engine that stopped reading with a full pipe would
  freeze Templum. Fix: a writer thread fed by a channel.
- [ ] **Stop's fallback kills at once.** `Engine::stop` uses `Child::kill` (SIGKILL), where the spec says SIGTERM,
  then a kill 2 s later. Fix: SIGTERM first on Unix (through `templum-terminal`'s signal helper), then kill.
- [ ] **The engine chip never shows the model Claude Code reports.** `TemplumAgent::model`
  (`crates/templum/src/chat/mod.rs`) is never set from `ChatEvent::Started`: with no model in Settings the chip
  says "Claude Code". Fix: set it in `engine_line` on `Started`.
- [ ] **`notify`'s notification opens nothing.** `control_call`'s `Notify` (`crates/templum/src/workspace/control.rs`)
  shows a notification without a click handler, where its description says a click opens the chat. Fix: an
  `on_click` that calls `review_agent`.
- [ ] **Esc after clicking the approval goes to the canvas.** A click on the approval (`chat/approval.rs`) while a
  terminal has the keyboard doesn't record `back`, so Esc afterwards gives the keyboard to the canvas, not that
  terminal. Fix: record `back` in `focus_approval` when the pill didn't have the keyboard.
- [ ] **After Stop, a task's end still wakes the engine.** `agent_notice` (`crates/templum/src/workspace/agent.rs`)
  sends every notice, so a run the person stopped goes on when an agent finishes. The spec doesn't say; ask the
  maintainer whether Stop should hold the notices as rows until the person writes.
- [ ] **The conversation grows without end and is saved on the UI thread.** `Conversation::rows`
  (`crates/templum-agent/src/conversation.rs`) keeps every row until New chat (each Action's whole `input`, a
  notice's first line), and `save_conversation` (`crates/templum/src/workspace/agent.rs`) writes all of it as pretty
  JSON at each turn's end on the UI thread: a long-running conversation makes each turn's end hitch. Fix: drop the
  `input` of rows older than the last few hundred (cap the rows the file keeps), and write compact JSON on a
  background thread.
- [ ] **The recorded fixtures aren't the command line Templum runs.** `crates/templum-agent/tests/fixtures/*.jsonl`
  were recorded without `--strict-mcp-config` and `--mcp-config`, so no `init` line shows `templum-control` in
  `mcp_servers` and nothing checks how Claude Code names it. Fix: re-record one turn with `claude::args`' exact words
  (a scratch HOME), and assert `templum-control` is `connected` in its `init`.
- [ ] **A chat row out of view that changes height is measured again only once shown.** The chat's `list`
  (`crates/templum/src/chat/panel.rs`) learns only of new rows; an action scrolled out of view that fails (it gains a
  line) or a person's row that stops being Queued keeps its old height until it scrolls into view, where the scroll
  can jump a few pixels. Fix: `ListState::remeasure_items` over the rows a turn changed (from the turn's first row).
- [ ] **The run meter never shows tokens.** `panel::meter` shows dollars only; the spec says tokens when there is no
  cost. Fix: keep the turns' and the notices' tokens and show them when the cost is 0.
- [ ] **Stopping or interrupting what would refuse anyway asks first.** `stop_agent` on the person's idle shell and
  `interrupt` on the person's idle agent (the gate in `crates/templum/src/workspace/approvals.rs`, `gate`) show the
  approval, and only after Allow answer "Nothing runs" or "is idle: there's nothing to interrupt.": the person is
  asked for nothing. So do `send_keys` into an agent on another agent's task ("on a task another agent handed it")
  and `delegate` into an agent that waits for its person or is on another agent's task: asked, then refused after
  Allow. Fix: before asking, run the target's own refusals (`stop_in`'s nothing-runs check, `interrupt_in`'s idle
  and no-agent checks, `send_keys`' and `delegate`'s busy checks) and answer at once when they refuse.
- [ ] **Parallel `open_agent` with a `worktree` can pass Agents at once.** `control_call`'s `OpenAgent`
  (`crates/templum/src/workspace/control.rs`) checks `too_many_tasks` before git makes the worktree, but the task is
  counted only once the agent opens (`open_agent_in`), so several calls in one turn all pass while their worktrees
  are made, and more agents than the cap start. Fix: count the worktrees being made for a prompted `open_agent` in
  `agent_tasks` (a counter raised before `in_worktree` and lowered when it opens or fails).

- [ ] **A dialog the agent cancels itself can give way to another in the same words unseen.** A terminal's `facts`
  (`crates/templum/src/workspace/approvals.rs`) count its agent's dialogs by their hooks and the input that reached its
  program (`Terminal::inputs`), but when the agent itself drops the dialog the approval showed (a subagent aborted) and
  a queued one in the same words takes its place, no hook fires and nothing reached it: Allow presses its keys on a
  command the person never saw. Fix: put a hash of the screen's dialog region in `facts`, leaving out its animated
  lines (a spinner, a timer).
- [ ] **The person's pointer moving over a TUI isn't counted.** `Terminal::mouse`
  (`crates/templum-terminal/src/terminal.rs`) counts in `Terminal::inputs` a press, a drag, a release and the wheel,
  not a move with no button down, so a program that asked for every move (and highlights the option under the pointer)
  can have its choice moved before an Allow of `send_keys ["enter"]`. Counting the moves would ask again each time the
  pointer crosses it on the way to the approval. Fix: the screen hash above covers it.
- [ ] **Closing an open tool row doesn't shrink the card.** The body's height for its rows (`TemplumAgent::rows_h`,
  `crates/templum/src/chat/panel.rs`) only goes up until New chat, so after a long row is opened and closed the card
  keeps the room. Fix: let it go down too, once the rows are measured under it, without the frame-to-frame jitter
  that made it go up only.
- [ ] **Answers' headings aren't semibold.** `markdown::chat_style` (`crates/templum/src/markdown.rs`) sets their sizes
  only: gpui-kit's `TextViewStyle` has no heading weight. Fix: when gpui-kit offers one.
- [ ] **Not seen by eye: the approval card and the pill that asks or works.** The Templum Agent's approvals come from
  a running engine's control connection, which no fixture or test seam can start without a live Claude Code, so the
  screenshots of card 143 lack them (and the queued label under a bubble, which loading a conversation clears).
  Fix: a dev-only fixture flag that fills `TemplumAgent::approvals` and a queued row, or a look at them in the next
  real session.

## Agents view

- [ ] **A row's age is stale in a window nothing redraws.** `inbox::age(now - since)` is read when the popup is
  drawn (`crates/templum/src/agents_view.rs`, `agent_row`), and nothing animates, so an open popup in a quiet window
  says "2m" until the next frame. Fix: while the popup is open, a 30-second timer that calls `cx.notify()`.
- [ ] **The details have no scrollbar.** The right column scrolls with the wheel but, unlike the inbox's, draws no
  `scrollbar_over` (`crates/templum/src/agents_view.rs`, `detail`): a tall details column (many subagents, a long
  task) gives no hint that it goes on. Fix: keep a `ScrollHandle` for it in the Workspace and draw `scrollbar_over`.

- [ ] **A group's header stands 28 px under the row above it.** The list is a `uniform_list`, whose items share one
  height, so a header is a row's 48 px with its name at the bottom (`crates/templum/src/agents_view.rs`, `list`):
  twice the 14 px Automations leaves over a group's name. Fix: gpui's variable-height `list` (`ListState`), or a
  header drawn in the first row of its group.

## Popups

- [ ] **Git stays "open" across a workspace switch.** The sidebar is out of the popups' backdrop
  (`crates/templum/src/popup.rs`, `slot`), so a workspace picked there while the Git popup is open keeps `git_open`:
  a workspace with no Git shows no popup, and coming back shows Git again without a click. Requests, Agents and
  Automations just show the new workspace's. Fix: `close_git` when the workspace shown changes, or open the new
  workspace's Git as the button would.

## Automations

- [ ] **A command typed into a terminal at its prompt joins what the person half-typed and left there.** Templum can't
  read the input line (`command_busy` in `crates/templum/src/workspace/automations.rs` only knows a command runs, an
  agent runs, or the person pressed a key in the last 3 seconds). What a person sees: `make test` appended to their
  half-written `git comm`. Fix: read the line between the prompt's mark (OSC 133 A) and the cursor, and hold the command
  while it isn't empty. A command in a new terminal, the default, has no such risk.
- [ ] **Run now on a held run does nothing.** `run_automation_now`
  (`crates/templum/src/workspace/automations.rs`) returns while the rule is already held or its answer is on its way,
  so pressing Run now on "Waiting for abc12" gives no feedback. Fix: flash the row, or let the press turn the held run
  into a counted one.
- [ ] **The editor's date is a plain `YYYY-MM-DD` field.** `crates/templum/src/automations_view/editor.rs` has no date
  picker, so a person must type the date of a Once. Fix: gpui-kit's date picker for the Once's date.
- [ ] **A cron's preview is computed every frame the popup is open.** `cron_preview`
  (`crates/templum/src/automations_view/editor.rs`) parses the expression and walks three next times on each draw.
  Cheap for one rule, but it runs while the person types. Fix: keep the preview with the draft and recompute it only
  when the expression changes.
- [ ] **A model and an effort are free text.** The Start an agent form takes both as typed words
  (`crates/templum/src/automations_view/editor.rs`) and only Save says if the agent refuses them. Fix: dropdowns of the
  agent's own models and efforts (`Agent::launch_args`' lists).
- [ ] **A rule turned on or saved can hear an event from the second before.** `automation_heard` is filled while any
  event rule is on and emptied at each look (`crates/templum/src/workspace/automations.rs`); a rule turned on or saved
  between an agent's turn end and the next look also fires for it. What a person sees: a rule they just turned on runs
  once for a turn that ended a moment earlier. Fix: stamp each queued event and fire a rule only for events after its
  `since`.
- [ ] **A save in the second of the run's start counts the old task for the new rule.** The answer of a task being
  typed counts only if `since` still equals the one it started with, and `since` is in whole seconds
  (`crates/templum/src/workspace/automations.rs`, `attempt`). What a person sees: a rule edited within a second of
  firing counts that run once. Fix: keep a counter that every save and switch-on bumps, and compare it.

## API

- [ ] **An approval has no time in the state.** `RequestView.since` is when its popup first showed it (0 before), since
  `chat::Approval` has no creation time. Fix: give `Approval` a `since: Instant` when it is queued.
- [ ] **A certificate or key edited on disk is read at the next start of the server only.** Settings ▸ API ▸ My
  certificate reads the files when the server starts; replacing them (a renewal) needs a toggle of the switch. Fix: a
  *Reload* button, or a look at their modified times.
- [ ] **Under nonstop output a terminal stream may skip many looks in a row.** The reader (`terminals.rs`, `look`) reads
  with `try_lock_unfair` and skips a terminal whose lock is held, which the PTY thread does while it parses; a mirror of
  a terminal that prints nonstop sees fewer frames. Fix: after a second of skips, wait for the lock unfairly once, on a
  blocking thread.
- [ ] **Settings ▸ API says "listening to events" for a connection that streams a terminal.** (`settings/api.rs`, the
  client row's `streaming`.) Fix: "streaming" or the stream's kind.

## Templum to Templum

- [ ] **A tab's × waiting for its text fires `close_note` after home already closed the note, and home's refusal
  shows.** (`workspace/remote_notes.rs`: `fire_closings`, and `remote_note_act`'s `Close`.) After "… isn't open on
  winx-desktop any more: your version is …", or a send that lands, a second toast says "b7k2m shows no note 9.".
  Fix: `fire_closings` drops a `Closing::Tab` whose note `block_showing` no longer shows.
- [ ] **A gone note whose copy was refused `TRIES` times is never tried again.** (`workspace/remote_notes.rs`: step 1
  waits while `copy_refused >= TRIES`, and only a key in the shown tab resets it.) Example: a share downgraded and
  given back; the note has no tab, so no key reaches it and the text waits until Templum quits (reopening the note
  from All notes… shows it again). Fix: reset `copy_refused` on `Update::Status(Live)`/`Update::Me` for an edit whose
  note no block shows, or say in the stuck sentence that reopening the note shows the text.
- [ ] **`edit_note` takes a text for a note the state emptied under its 8 MiB budget.** (`workspace/note_tabs.rs`
  refuses only past 256 KiB; `workspace/api_state.rs` empties a note past the budget, with `cut` true and its `rev`.)
  The spec's table says "refused for a note whose text the state cuts": an API client other than the mirror can write
  over a text it never saw (it needs 8 MiB of notes in its state). Fix: refuse a text for a note the caller's last
  state emptied, or narrow the spec and docs/api.md to "longer than 256 KiB".
- [ ] **× on a tab, or on a whole note block, past 256 KiB does nothing visible.** (`workspace/remote_notes.rs`:
  `close_when_saved`, for `Closing::Tab` and `Closing::Block`; the `NOTE_LONG` sentence is told once per note.) The tab
  (or the block) stays open, the tab with its " •", until the person shortens the text, and nothing is said on the ×.
  Fix: remove the note's `NOTE_LONG` tag from `told` in `close_when_saved`, so the next step says it again.
- [ ] **A copy past 256 KiB whose block is still there lands on top of that block.** (`workspace/remote_notes.rs`,
  `keep_note`: `add_block` with `x`, `y` = `edit.rect`'s origin; home's `Place::At` is exact, `ops.rs` `spot`.) Home
  closes the tab of a note the person made longer than 256 KiB and the block stays: the new note block (`note::SIZE`)
  covers the old block's top-left corner, on the mirror and at home. Fix: when the block is alive, send
  `near: <its id>` instead of `x`/`y` (`room_near` finds free room).
- [ ] **The banner while not live and the Forget… dialog were checked headlessly only.** (`mirror.rs`,
  `connect_dialog.rs`.) The banner live and slow and the remote tile were looked at with `--bench-mirror`; the
  banner's buttons while reconnecting, cut or stopped (their center line is tested), the Forget… dialog, the veil's
  strength and the Connect dialog's added-authority choice were not looked at on a display. Fix: look at them with two
  `--dev` instances (stop, revoke and delete at home).
- [ ] **The banner's words carry no bold.** (`workspace/remote.rs`, `banner_words`.) The spec bolds "Try again",
  "Paste a new code…" and the names; the text is plain and the buttons carry the actions. Fix: spans in `Banner`.
- [ ] **Connect anyway keeps the choice in the key only after the client saved it.** (`client/remote.rs`,
  `Life::trust_added`.) A keyring that refuses the second save leaves the key in memory for this run while the
  index says "Keyring"; the next start asks again. Fix: a `Update::Kept` after the save.
- [ ] **`revoke_share` lists as "full, or the share itself" while its registry scope is read.**
  (`crates/templum-api/src/registry.rs`, `scope`, `nominal`; `workspace/api.rs`, `Own::RevokeShare`.) A Read or
  Control share token passes the server's scope check and the glue refuses it when it names another share. Likewise
  `GET /v1/tools` lists `share_workspace` as "full" while a Full token limited to some workspaces is refused (the glue
  asks for all workspaces). Fix: a scope that depends on the caller, in `registry::scope`.
- [ ] **A shifted symbol with Ctrl or Alt reaches the kitty protocol with the symbol as its key.**
  (`crates/templum-terminal/src/input.rs`, `named_key`.) `ctrl+shift+;` is key `;` with Shift, not the unshifted key
  under it (the spec's known limit). Fix: names carry the unshifted key too.
- [ ] **The cast mark and the news dot share a corner of a sidebar tile.** (`sidebar.rs`, `tile`.) A mirrored workspace
  with news shows the 7 px dot over the 12 px mark. Fix: the dot moves to the top-left when both show.
- [ ] **The Share… dialog was checked headlessly only.** (`share_dialog.rs`.) Its layout (field spacing, the code's
  wrapping by characters in a 520 px dialog, a long Directly address) was not looked at on a display. Fix: look at it
  with `--dev` once.
- [ ] **A pinned share's address is not tried against the machine.** (`workspace/api.rs`, `share_workspace`.) A typo in
  the host or port makes a code that never connects. Fix: the dialog connects to it once before it shows the code.
- [ ] **The system's proxy settings and PAC files aren't read, nor NTLM or Kerberos.**
  (`crates/templum-api/src/client/dial.rs`.) The spec's call: only a typed or environment `HTTP CONNECT` proxy with
  Basic credentials. A work proxy that needs another login refuses with 407 ("The proxy … refused the connection").
  Fix: a later card.
- [ ] **A frame is fed on the client's thread under the viewer's fair lock,** which may wait for a paint's copy.
  (`crates/templum-api/src/client/frames.rs`, `Painter::paint`; `templum-terminal`'s `Feeder::feed`.) Fix: if
  `--bench-mirror` shows it, feed with `try_lock` and keep one merged pending frame per terminal.
- [ ] **Notices folded past the cap are told only at the next wake.** (`workspace/remote.rs`, `fold_notices`.) It runs
  at a drain, so a quiet home's "N more notices from …" waits for its next update, which can be a while after the
  minute. Fix: a one-shot timer at `notice_budget.folded_at + NOTICE_SPAN`.
- [ ] **A share whose `GET /v1/me` lists no workspace hears no notice.** (`workspace/remote.rs`, `remote_notice`.) A
  notice is shown only when its workspace is the remote's `entry.home`, which `Update::Me` sets from the first
  workspace the share lists; a Me with `workspaces: None` leaves it unset. Shares are always limited, so nothing
  reaches this today. Fix: none unless a share can be unlimited.
- [ ] **An event stream waiting for a fresh `Last-Event-ID` is advisory.** (`crates/templum-api/src/client/remote.rs`.)
  Home answers every resume with a snapshot, so the id only tells it the mirror is the same client. Nothing to fix
  unless home learns to replay.
- [ ] **The Keychain's and the Credential Manager's stores were not run.**
  (`crates/templum-api/src/client/keys.rs`, `system_store`.) Their arms were checked by reading (and the Secret
  Service's by building) on this Linux machine; the tests use the mock store. Fix: CI's macOS and Windows builds,
  then one save and load by hand on each.

- [ ] **A remote workspace's pan is kept only when the index is next written.**
  (`workspace/persist.rs`, `save_canvas_now`; `workspace/spaces.rs`, `index`.) The camera lives in the index entry,
  which is written on a switch, an edit and when Templum quits, not by the 5 s save: a crash loses a pan made since.
  Fix: the snapshot writes the index when a remote's camera moved.
- [ ] **A remote whose key can't be read is disconnected only once its start task runs.**
  (`workspace/mod.rs`, `Workspace::with`.) `connect_remotes` runs in a task spawned at the end of `with` (so the tests
  can swap `keys` first); until it runs, a remote with no key shows "Connecting…" for a frame. Fix: set
  `Disconnected` in `load_spaces` for an entry with no key.
- [ ] **Part of the Connect dialog was checked headlessly only.** (`connect_dialog.rs`.) Card 280 looked at it with
  `--dev` (empty, a code that reads, one that doesn't, Connecting…, a failure, Network unfolded, the keyring switch,
  light and dark), not at a long Cloudflare Access secret, a failure of several lines, Paste a new code… nor the
  pointer's hovers. Fix: look at those once.
- [ ] **The Connect dialog's Network section opens only with the pointer.** (`connect_dialog.rs`, `network`.) Its
  name and chevron are a press target, not a Tab stop, as the popups' hand-made buttons aren't; a person on the
  keyboard can't reach the Access token or the proxy (the old More… couldn't either: Tab stayed in the code's field).
  Fix: give the toggle a focus handle with `tab_stop(true)` and Enter/Space, with a test.
- [ ] **The Share… dialog isn't a dialog of the popups' kind, and Enter closes it.** (`share_dialog.rs`, `open`.) It
  is gpui-kit's alert dialog as the Connect dialog was before card 280: the kit's frame and title, labels over
  stacked fields, the kit's buttons, no Close, no press outside to close it; Enter in a field closes it without
  making the code. Fix: build it as the Connect dialog (`popup::dialog`, `dialog_frame`, `dialog_header`, rows,
  `foot` with Cancel and a solid Make code that Enter does), keeping its ids, with a test for Enter.
- [ ] **In a window too short for it, a dialog scrolls whole.** (`popup.rs`, `dialog`.) gpui-kit's dialog caps its
  height at the window's and scrolls what it holds, so the header and the foot scroll away with the body, under the
  kit's scrollbar. Fix: cap `dialog_frame` at the room the kit leaves and let the body scroll between the header
  and the foot, with the standard scrollbar.
- [ ] **A remote being connected shows in the sidebar as "Connecting…" while the Connect dialog waits.**
  (`workspace/remote.rs`, `connect_code`.) It is a hidden workspace from the first moment, so its tile is there (and a
  failed connect removes it again). Fix: leave it out of the sidebar until `connect_result` is Ok.
- [ ] **The Connect dialog says the keyring sentence until the probe answers.** (`connect_dialog.rs`, `open`;
  `Workspace::probe_keyring`.) On a computer with no keyring the "no keyring" line and the file switch replace it a
  moment after the dialog opens. Fix: say nothing until `keyring` is Some.
- [ ] **A copied card of a remote workspace would paste as a blank note.** (`workspace/mod.rs`, `Body::item` for
  `Body::Mirrored`.) A card has no saved shape, so it answers an empty note item. Nothing reaches it: Copy and Paste
  of blocks do nothing on a remote workspace. Fix: `Body::item` answers an `Option`.
- [ ] **`Terminal::grid_size` of a viewer is still its first size.** (`crates/templum-terminal/src/terminal.rs`.) The
  mirror reads `Pane::grid_size` (the grid's own, after a `Feeder::resize`); any other caller of `Terminal::grid_size`
  on a viewer would still read the stale one. Fix: `Terminal::grid_size` reads the grid for a viewer.
- [ ] **A remote terminal at several zooms, its selection and its Read-only footer were checked headlessly only.**
  (`block.rs`, the terminal body; `workspace/input.rs`, `cell_under`.) The cells fit home's grid at a zoom step and
  the pointer maps through the same step, which a test of `fit_zoom` and the stream proves, not how it looks. Fix:
  look at a remote terminal at 50%, 100% and 200% with `--dev` once.
- [ ] **The stream set is chosen while the canvas is painted.** (`workspace/remote.rs`, `stream_terminals`.) A pan
  that stops inside the 100 ms between two choices waits for a timer to repaint (`stream_at`); a canvas that is never
  painted again (a hidden window) keeps the last set. Fix: a timer task that chooses, if a hidden window must keep
  streaming.
- [ ] **The Reconnecting banner's "next in N s" and "Live 2 min ago" only change on a repaint.**
  (`mirror.rs`, `banner`; `workspace/remote.rs`, `banner_words`.) While a remote reconnects and nothing else repaints
  the canvas, the banner can say "next in 30 s" through the whole wait. Fix: a 1 s timer, like `stream_at`, while the
  shown remote is `Reconnecting`.
- [ ] **Install update and an account's Log in do nothing while a mirror is on screen.**
  (`workspace/update.rs`, `install_update`; `workspace/blocks.rs`, the account's login terminal.) Both open their
  terminal in the shown workspace through `spawn_block_with`, which `on_remote` refuses, and neither says why. Fix:
  open the terminal in the first local workspace (the way `Settings` does), or say `NOT_YET` in a notification.
- [ ] **Quitting while the Connect dialog waits leaves the new key in the keyring.**
  (`workspace/remote.rs`, `connect_code`; `workspace/spaces.rs`, `index`.) The pending remote is kept out of the index
  until it is live, but `KeySource::Save` already put its key in the keyring; closing the dialog deletes it
  (`give_up_connecting`), quitting does not. Fix: give up the pending remote when the window closes, and wait for the
  client's thread to finish deleting.
- [ ] **The Connect dialog's proxy field keeping only the address was not checked in the dialog.**
  (`connect_dialog.rs`, `connect`.) After Connect the field holds `proxy_of`'s address, without the login; a test
  proves the `Connecting` draft never prints it, not the field. Fix: a dialog test that types a proxy with a login
  once its inputs can be read from a test.
- [ ] **A selection of several blocks moved on the mirror is one Ctrl+Z step per block at home.**
  (`workspace/remote.rs`, `remote_moved`; `workspace/history.rs`, `end_placing`.) One `move_block` is sent per block
  (the spec's known limit). Fix: a `move_block` taking several blocks.
- [ ] **Shift+Enter in an agent without the kitty keyboard reaches home as Enter.** (`workspace/input.rs`,
  `on_key_down`; `workspace/delegate.rs`, `press_each`.) Home's newline rule is its local keyboard's, not
  `terminal_input`'s, so a mirror's Shift+Enter in Claude Code before it turned the kitty keyboard on sends Enter. Fix:
  apply the rule in `press_each` for `terminal_input`.
- [ ] **Paste a new code… closes its dialog at once.** (`connect_dialog.rs`, `connect`; `workspace/remote.rs`,
  `replace_code`.) A code home refuses, or an address that can't be reached, is told by the banner, not in the dialog.
  Fix: wait for the first status as Connect does, without forgetting the remote on failure.
- [ ] **Typing, a remote's Edit…, Paste a new code…, and the dim foot of a request were checked headlessly only.**
  (`workspace/input.rs`, `space_dialog.rs`, `connect_dialog.rs`, `balloon.rs`.) Their looks and the feel of typing
  through a real tunnel were not seen on a display. Fix: look at them with two `--dev` instances.
- [ ] **The Open dialog, Hand a task…, the dimmed menu items and the dimmed ✕ were checked headlessly only.**
  (`launcher.rs`, `ask_home_folder`; `prompt.rs`, `ask_lines`; `menu/build.rs`, `gated_item`; `block.rs`, the header's
  ✕.) Their layout, the tooltips of dimmed items (an element item has no hover of its own in a popup
  menu) and the dimmed look were not seen on a display. Fix: look at them with two `--dev` instances, one a Control
  share, one a Full share, one an older home.
- [ ] **The Open dialog's answer closes whatever dialog is on top.** (`launcher.rs`, `ask_home_folder`.) A person who
  closed it with Esc while home was answering has home's Ok close the next dialog they opened. Fix: keep the dialog's
  handle and close that one, or ignore an Ok after Cancel.
- [ ] **A shared workspace with no folder refuses the mirror's terminals and agents.** (`workspace/api.rs`,
  `confined_default`.) Home's confinement of a limited token says so in its words ("this workspace has none"), which
  the mirror shows after the click. Fix: the Share… dialog says it when the workspace has none.
- [ ] **A refused close brings its block back as a new one.** (`workspace/remote.rs`, `mirror_blocks`.) The block that
  was closed here and kept by home is drawn again with a new canvas id, so it is no longer selected or focused and
  keeps no scroll. Fix: keep the closed body for `HOLD` and put it back on a refusal.
- [ ] **Shift+PageUp and Shift+PageDown scroll a mirrored shell but not a local one.** (`workspace/input.rs`,
  `remote_page`.) Locally they reach the program as keys. Fix: page a local terminal's history the same way when no
  program took the screen or the mouse.
- [ ] **A mirrored terminal's history is read whole after any change.** (`client/remote.rs`, `Life::history`.) Up to
  256 KiB per scroll back of a busy terminal. Fix: a `read_terminal` from a line on.
- [ ] **Drags are sent to a program that asked only for clicks.** (`workspace/remote.rs`, `remote_mouse`.) The frame
  doesn't say `?1002`, so home drops them; merged moves keep it to one call per pace. Fix: a `mouse_drags` mode.
- [ ] **The history of a viewer that went full screen stays until it leaves.** (`templum-terminal`,
  `Feeder::drop_history`.) The alternate screen has no history, and the main screen's is not reached from there, so a
  stream that ends while the program is full screen leaves the history on the main screen until the viewer is fed
  again with a fill. Fix: keep the inactive grid reachable (`Term::swap_alt` resets the alternate screen, so not that).
- [ ] **A touchpad scrolls a mirrored terminal drawn smaller than its block at the block's speed.** The wheel turns
  pixels into lines at the camera's zoom (`scroll_terminal`, `crates/templum/src/workspace/input.rs`), but a viewer
  whose home grid is bigger than its block is drawn at the fit zoom (`remote::fit_zoom`), so a swipe moves fewer lines
  than it moved the text under the fingers. Fix: use the fit zoom for a remote pane's line height there.
- [ ] **Lines wheeled back before a history read that failed wait for the next one.** `RemoteSpace::back`
  (`crates/templum/src/workspace/remote.rs`) is cleared when home's history comes, when the person types and when the
  view moves through a history; a read home refused leaves it, and the next history read for that terminal scrolls it
  back by those lines too. Fix: forget it on the client's error for the read (`To::History`).
- [ ] **The mouse and the wheel over a remote terminal, Shift+PageUp and Find through home's history were checked
  headlessly only.** (`workspace/input.rs`, `workspace/remote.rs`.) The pointer over a remote terminal at other zooms,
  and a wheel step over a program that took the mouse, were not looked at on a display. Fix: look.
- [ ] **A column dragged on a mirror moves when home's state comes.** (`workspace/remote_boards.rs`,
  `remote_board_act`.) A card move shows at once and stands over home's state; a column move isn't held, so it shows a
  round trip later. Fix: a `columns_moved` hold, laid over the copy as `cards_moved` is.
- [ ] **The board's dimmed controls and its menus on a remote were checked headlessly only.** (`kanban.rs`, `gated`;
  `menu/build.rs`.) Their dimming, tooltips and the confirm dialogs of Delete card, Delete and Delete board… were not
  looked at on a display. Fix: look at them with two `--dev` instances, one a Control share, one an older home.
- [ ] **A comment deleted on a mirror has no Undo.** (`kanban.rs`, the thread.) Home's Ctrl+Z on its kanban brings it
  back; putting it back from here would change its author and time. Fix: `comment_card` of the text, or a restore at
  home.
- [ ] **`GET /v1/tools` describes `edit_comment` and `delete_comment` as the Templum Agent's.**
  (`templum-mcp/src/control.rs`, `SHARED`.) Through the API they edit the person's comments (those by `you`) and
  delete any, as docs/api.md says. Fix: the registry's own descriptions for the shared tools.
- [ ] **The card dialog on a remote was checked headlessly only.** (`kanban.rs`, `open_draft`.) The dimmed Done,
  Comment and Delete card, the attachments by name, Attach dimmed, the assignee menu of home's terminals and agents,
  and the dialog's height with them (the comment field leaves the test window) were not looked at on a display.
  Fix: look at it with two `--dev` instances.
- [ ] **A mirror's comment pencil and trash aren't dimmed, only refused.** (`kanban.rs`, `refused`.) A card whose home
  refuses `edit_comment` or `delete_comment` says the rule's words when they are clicked, once; the buttons look
  alive. Fix: read the two deeds once a frame, as Done's, and dim them.
- [ ] **A remote note's scroll may move when home's text changes, and so may a home's.** (`note.rs`, `write`.) It
  keeps the scroll with `set_scroll_offset`, which gpui-base defers to the next layout; whether that wins over the
  scroll `set_selected_range` takes to the caret was only checked headlessly (the same doubt as "`append_note` scrolls
  the note back to the person's caret"). Fix: look at it with two `--dev` instances and an agent's `append_note`; if
  the caret wins, set the offset again one frame later.
- [ ] **A remote note, a remote checklist and home's ink were checked headlessly only.** (`note.rs`, `checklist.rs`,
  `ink.rs`.) The editable note and its " •", the conflict's notification, All notes…'s Closed rows, the dimmed + and ×
  of its tabs, the dimmed check boxes and header buttons, the new item's field and home's strokes painted above a
  mirror's blocks were not looked at on a display. Fix: look at them with two `--dev` instances, one an older home.
- [ ] **A copy whose answer was lost, and that home made, stays stuck; a key makes a second one.**
  (`workspace/remote_notes.rs`, `keep_note`'s `Err` arm with `maybe_done`.) A person sees "Plan (Another computer)"
  appear as a tab of the block while their own note stays read-only with " •" and "… couldn't be kept … press a key
  to try again"; a key duplicates the note at home. Fix: at the next state, if `edit.block` shows a note named `name`
  whose text is the person's, take `keep_note`'s `Ok` path.
- [ ] **A note closed at home with a send in flight that had landed is copied anyway: its text is at home twice.**
  (`workspace/remote_notes.rs`, step 2: `sent.is_some()` goes to the copy.) The closed note holds the sent text and
  the new note beside it holds the same text; nothing is lost. Fix: in step 2, return while `sent` is Some; after the
  `Ok` the base is the sent text, so step 2 drops the edit when nothing more was typed, and after a refusal ("… shows
  no note …") it copies as now.
- [ ] **Keys typed between a copy starting and the next paint are overwritten when the copy lands.**
  (`note.rs`, `readonly` is applied at render; `workspace/remote_notes.rs`, `keep_note`'s `Ok` writes home's text
  over the editor.) The copy can start without a key (home's state, the wake), so a key can reach the editor before
  the read-only paint; that key is in neither note. Fix: in `remote_note_typed`, when the edit is `copying`, undo the
  change (`note::write` the text the copy carries), or take the copy's text for `keep_note`'s `Ok` from the editor's
  value at that moment.
- [ ] **Revisions start at 0, not from the clock, when no `notes.json` was read.** (`workspace/spaces.rs`: `stamp`
  runs only in the `Loaded::Saved` arm.) On a first run, or with the file moved aside, revisions don't start from the
  Unix time in milliseconds the spec says; no reuse is possible in practice, as the next start reads the file and
  seeds high. Fix: stamp after the `match` (`self.notes.stamp(notes::rev_seed())` for every arm).
- [ ] **A mirrored checklist's item text and name aren't dimmed, only refused.** (`checklist.rs`, `row`.) A click on an
  item's text while the remote isn't live says the rule's words once; the text looks alive (the check box, the × and
  + Add an item are dimmed). Fix: read `Deed::EditItem` once a frame in `Why`, dim the text and give it the words as
  its tooltip.
- [ ] **Notes of a mirrored workspace are cleaned and capped, then drawn whole.** (`workspace/remote_content.rs`,
  `note_cut_of`.) A hostile home's 1,000 notes of 256 KiB are 1,000 editors; the cap bounds memory but not the cost of
  opening them. Fix: make an editor for the note on screen of each block only, the rest when a tab is shown.
- [ ] **Undoing all of one's typing after a send whose answer was lost can bring the text back.**
  (`workspace/remote_notes.rs`, step 6 with `doubt`.) A send's answer is lost, the person undoes back to the base
  before home's next state arrives, step 6 drops the edit (the text equals the old base), and the state then shows the
  send had landed: the clean path writes it back into the editor. Fix: in step 6, keep the edit while `doubt` is Some.
- [ ] **Two lost answers in a row can make a spurious copy.** (`workspace/remote_notes.rs`, `send_note`'s `Err` arm.)
  A second `maybe_done` replaces `doubt` with the newer text; if the first send landed and the second was refused at
  home, home's text matches neither and the person's text becomes a note beside it. Nothing is lost. Fix: keep every
  doubted text (a short `Vec`) and resolve against any of them.
- [ ] **Home's state copies every note's text and converts all the ink on each API tick.** (`workspace/api_state.rs`,
  `note_views`, `drawing_views`.) While an event stream is open, each tick (250 ms; 100 ms while dragging) copies the
  text of every shown note (up to 256 KiB each) and converts every local workspace's ink twice (`InkItem::of`, then
  `DrawingView::from`), changed or not; a person with many large notes or much ink sees stalls of a few ms on the UI
  thread. Fix: cache an `Arc<DrawingsView>` per space keyed by an ink revision (bumped on push, remove, retain, undo
  and redo) and an `Arc<NoteView>` per note keyed by `Entry.rev` (a note's name and color change `NoteView` without
  moving it: key by the three), so an unchanged tick reuses its `Arc`s and the publisher's comparison becomes a
  pointer check.
- [ ] **Two `git` calls sent back to back on one repository may run in either order.**
  (`workspace/api_git.rs`, `api_git`, `git_queue`.) Each call finds its repository on the background executor before it
  takes its place in that repository's queue, so a `stage` and a `commit` sent without waiting for the first answer
  can swap. A client that waits for each answer (the mirror's popup does) is not affected. Fix: a ticket taken in
  `api_git`, with the queue ordering by it once the repository is known.
- [ ] **A planted `.git` file naming a network share is read by git before Templum can look at the root.**
  (`workspace/api_git.rs`, `locate`.) `Repo::discover` runs git in the folder first; on Windows git opening a share
  offers the person's credentials to it. Only something already able to write the workspace's folder can plant one (a
  share's `git` paths can't name `.git`). Fix: read the folder's `.git` file before discovering and refuse a gitdir on
  a share outside the token's folders (`refuses_share`).
- [ ] **A killed `git` call's helpers can outlive it in two cases.** (`crates/templum-git/src/repo.rs`, `until_cancelled`,
  `kill_tree`.) Once git exited and a helper still holds the pipe, the second loop kills by git's process group; if every
  member left the group (a `setsid` daemon) the group id can be freed and reused within the 10 minutes (theoretical).
  On Windows, once git has exited `taskkill /T /PID` no longer finds the root, so the helper isn't killed. What a person
  sees: a stray helper process after a call that timed out. Fix: on Unix, remember the group's members before git
  exits; on Windows, run git in a Job Object with `KILL_ON_JOB_CLOSE`.
- [ ] **A prompt can appear on home's screen during a `git` call, and commits are signed as the person.**
  (`crates/templum-git/src/repo.rs`, `quiet`; `ops/jobs.rs`, `commit`.) The call silences ssh and credential managers
  but not a graphical `GIT_ASKPASS` / `core.askPass` window, nor gpg-agent's pinentry when `commit.gpgSign` is on; with
  the passphrase cached (or `gpg.format=ssh` and an agent holding the key) a commit made by a remote token is signed
  with the person's key without a question. What a person sees: a password window nobody asked for, or commits of theirs
  signed that they didn't make (no operation can change a remote's URL, so the window cannot be pointed at an attacker's
  server). Fix: pass `-c commit.gpgSign=false` for `commit` and `sync` calls of a token below Full, and
  `GIT_ASKPASS=true` / `-c core.askPass=` for every call with a deadline.
- [ ] **`resolve` through the API can add a copy to the trash without end.** (`crates/templum-git/src/ops/jobs.rs`,
  `resolve_file`; `trash.rs`, `prune`.) `Repo::resolve` doesn't stage, so the file stays conflicted and every repeat
  copies it again (up to 50 per repository, the API's counted apart from the person's). What a person sees: the API's
  share of the trash filled with copies of one file. Fix: skip the copy when the file's bytes equal `:2`, `:3` or HEAD's
  version, which git keeps.
- [ ] **The folder menu on a mirror lists folders, not repositories.** (`workspace/git_home.rs`, `home_git_folders`.)
  Two terminals in different folders of one repository outside the shown one show as two rows. Fix: a `find` per
  folder when the menu opens, deduplicated by root.
- [ ] **A recent folder of home's whose name cleaning would change isn't listed on a mirror.**
  (`workspace/remote.rs`, `home_recent`.) A path with two blanks in a row, a tab, a leading or trailing blank or over
  300 characters is left out of the folder menu (cleaning it would name another folder). Fix: the same opaque token as
  the minor below, for every folder of home's.
- [ ] **A mirror's first Git doesn't fall back to home's newest recent folder.** (`workspace/git_home.rs`,
  `home_git_folder`.) Locally the first Git opens on the newest recent folder when the workspace has no terminal and no
  folder; on a mirror it says "No folder to open Git in on …" (the recent folders are read when the popup opens). Fix:
  read `recent_folders` at connect and take its first line last in the guess.
- [ ] **The mirror's recent folders show a round trip after a folder was opened at home.**
  (`workspace/remote.rs`, `read_home_recent`.) The menu shows the answer read when the popup opened or when the menu
  last opened, so a folder opened at home while the popup stays open shows at the menu's next opening. Fix: home's
  state carries a counter of `Space::recent`'s changes, and the mirror reads again when it moves.
- [ ] **A discard too large to keep says "Too much to keep a copy" even when home keeps no trash.**
  (`workspace/git_discard.rs`, `ask_discard`.) `measure` answers no "has a trash". Fix: `Measured.trash` at home.
- [ ] **A Sync, Push or Generate running at home can't be stopped from the mirror.** (`workspace/git_branches.rs`,
  `cancel_git`.) The button says so and home's 10-minute deadline ends it. Fix: a `cancel` operation that kills the
  call's process group at home.
- [ ] **The panel's diff on a mirror can be 10 s old.** (`workspace/git_home.rs`, `panel_due`.) A file edited again at
  home without its status changing shows its new lines within `HOME_DIFF_AGAIN`. Fix: a cheap `look` field that
  changes with the working files' stamps.
- [ ] **The mirror's Git popup was checked headlessly only.** (`git_block/view.rs`, `changes.rs`, `commit_form.rs`,
  `diff_view/paint.rs`, `menu/build.rs`.) The dimmed Discard, Abort, ⚙ and menu items, and the spinning Sync's
  tooltip were not looked at on a display. Fix: look at them with two `--dev` instances, on a Control share.
- [ ] **A folder at home whose name cleaning would change can't be opened from a mirror.** (`workspace/git_home.rs`,
  `found`.) A root with two blanks in a row, a line break, a control character, or over 300 characters is refused
  with "The repository's folder at home has a name this computer can't show." (it is every later call's `folder`, so it
  is never altered). Fix: send the folder home as an opaque token it chose, and draw a cleaned name.
- [ ] **Generate's answer is read on the UI thread.** (`workspace/git_message.rs`, `generate_at_home`.) It is the
  one call of the Git popup that doesn't go through `home_job`, so its up-to-8 MiB answer is parsed and cleaned
  in the `Then`. Fix: the same background hop as `home_send`.
- [ ] **The Automations popup's groups of home's rules and its editor on a home rule were checked headlessly only.**
  (`automations_view/mod.rs`, `home_group`; `automations_view/editor.rs`.) A test draws the group's + and rows, a home
  rule's switch, its Where, the foot sentence and Save on a Full and a Control share, and the loopback test saves and
  runs a rule from the mirror; no display looked at the group's spacing, the dim group at 50%, or Save dimmed on a
  Control share with its tooltip. Fix: look at them on a display with a mirror shown, and adjust.
- [ ] **The Agents popup's remote groups and details, and a remote chip's effort line, were checked headlessly
  only.** (`agents_view.rs`, `detail`, `agent_row`; `block.rs`, the usage chip.) A test draws the remote row; no
  display looked at the dim group, the Task and Subagents sections of a remote agent, or the tooltip's "Effort: high"
  line. Fix: look at them with a mirror shown.
- [ ] **A switched rule or a cancelled schedule of home's shows a round trip later.** (`workspace/remote_rules.rs`,
  `set_home_rule`, `cancel_home_schedule`.) The switch and a cancelled schedule's row change when home's state arrives
  (the spec's holds name blocks, cards, items and wires), so a click looks unanswered for a moment. Fix: a hold over
  the copy, as `cards_moved` does for cards.
- [ ] **A mirror can't tell which token saved a home rule.** (`automations_view/editor.rs`, the foot sentence.) Home's
  state doesn't say it, so "Saved from another computer, it turns off when this share ends…" shows under every home
  rule's editor, also a rule the person made at home. Fix: a `saved_by` field in `AutomationView`, shown only for the
  share's own.
- [ ] **A rule the mirror's copy cut can't be edited from the mirror at all.** (`workspace/remote_rules.rs`,
  `save_home_rule`.) Save of a rule whose text is over 64 Ki characters (or holds a control character, or names more
  than 100 blocks) says "This rule is longer than a mirror shows: edit it on {computer}." even if the person only
  renamed it. Fix: send home only the fields the form changed, merged over `HomeRule::raw`.
- [ ] **A failed run met first is already seen.** (`workspace/remote.rs`, `rules_seen`; `remote_rules.rs`,
  `home_unseen`.) The badge counts a home rule's failed run that happened after the popup was opened or the rule was
  first heard of; one that failed while this computer was not connected shows no badge. Fix: keep what was seen across
  runs, per remote, in the index.
- [ ] **Cancel of a home schedule can go twice.** (`workspace/remote_rules.rs`, `cancel_home_schedule`.) Two clicks
  before home answers send two calls; the second answer is "There's no schedule s3", said as a warning. Fix: key
  `home_busy` by the schedule too.
- [ ] **A mirror's Checklists dialog shows a trash that deletes nothing.** (`checklist.rs`, `open_checklists`.) Two
  clicks on a row's trash say "Not on a mirrored workspace yet"; deleting home's checklists stays at home. Fix: a
  `list_dialog::Row` that may not be deleted draws its trash dimmed with the rule's words, or none.
- [ ] **A new checklist from a mirror doesn't open its name's field.** (`workspace/checklist_blocks.rs`,
  `remote_new_checklist`.) Locally New checklist edits the name at once; on a mirror home names it "Checklist", and
  the block beside comes without the camera going to it. Fix: once home's state brings the checklist (`awaiting`),
  `edit_field(Name)` and `reveal` it.
- [ ] **Text of a mirror's note not at home yet is lost when Templum quits or the remote is forgotten.**
  (`workspace/remote_notes.rs`, `NoteEdit`.) It lives only in the editor in memory, and only for the second or so
  before it is sent, or for as long as the remote isn't live. Fix: list such notes in `may_close`'s question and in
  Forget…'s.
- [ ] **Notes closed at home before kanban card 204 can't be reopened from a mirror.** (`notes.rs`, `closed_in`.) They
  have no mark, so the state doesn't list them; they stay in home's own Notes dialog. Fix: set `closed_in` at load for
  the notes a saved canvas names (the workspace of its file), or let the person pick a workspace in home's Notes
  dialog.
- [ ] **Several carets collapse to one when home's text comes into a mirror's note.** (`note.rs`, `write`.) It keeps
  the active selection only. Fix: shift every selection.
- [ ] **A composition pending is committed early when home's change is written into a mirror's note.**
  (`note.rs`, `write`.) A dead key or an input method's text is marked, and the write commits it as plain text before
  the splice (`unmark_text`), so the accent the person was composing stays as typed or is replaced by home's text;
  nothing lands over the caret. Fix: wait for the composition to end (`marked_text_range` is None) and write then.
- [ ] **The new tab may not be shown with the keyboard when home's state arrives before `new_note`'s answer.**
  (`workspace/remote_notes.rs`, `show_awaited_tab`.) The person clicks + and the tab appears without the keyboard, or
  not shown, until a later state comes within 5 s. Fix: call `ws.show_awaited_tab(&id, window, cx)` right after
  setting `tab_awaited` in the `Ok`.
- [ ] **A note block closed with unsaved text stays open if the person switches workspace before home has the
  text.** (`workspace/remote_notes.rs`, `fire_closings`, `Closing::Block(_) => {}`.) The block they closed is still
  there when they come back. Fix: let `close_remote_now` take the space id, or keep the closing until its workspace
  is shown.
- [ ] **× on a tab whose send was refused `TRIES` times does nothing visible.** (`workspace/remote_notes.rs`,
  `close_when_saved`.) The tab stays with its " •" and nothing is said again. Fix: say the refusal on the ×, or drop
  the closing after `TRIES`.
- [ ] **A typed-in note that becomes cut, with nothing new typed, keeps its old text in the editor.**
  (`workspace/remote_notes.rs`, step 2; `remote_content.rs`, `remote_notes` skips the write while the edit exists.)
  The mirror shows the pre-cut text without "… The rest is on …" until home changes the note again. Fix: when
  step 2 drops the edit, `note::write` the copy's text if it differs.
- [ ] **A key in a note home no longer shows, or that's missing from the copy, isn't tracked.**
  (`workspace/remote_notes.rs`, `remote_note_typed`.) Text typed in that frame is overwritten by home's next write.
  Fix: `note_locked` returns a reason when `block_showing(note)` is None.
- [ ] **A copy with no home workspace to go to stays "Keeping your version…" forever.**
  (`workspace/remote_notes.rs`, `keep_note`: `return` with `copying` still true.) The note is read-only for the rest
  of the run. Fix: set `stuck` (with `copy_refused` at `TRIES`) and say why.
- [ ] **Each send of a mirror's note carries the whole text.** (`workspace/remote_notes.rs`, `send_note`.) Up to
  256 KiB every 0.5–2 s while typing, where the splice with its `rev` would be small. Fix: send the splice when home's
  `edit_note` can apply one.
