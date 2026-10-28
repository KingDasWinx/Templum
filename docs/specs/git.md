# The Git block, second version

- **Date:** 2026-10-13
- **Status:** Design approved; all six batches implemented
- **Replaces:** [cockpit.md](cockpit.md), section 8 (the Git block it describes is what this one starts from)
- **Builds on:** [context-menus.md](context-menus.md), [agents.md](agents.md), [config.md](config.md)

> **Since 2026-10-17 the Git is a popup, not a block.** Each workspace has one Git (`Space::git`), shown in a
> popup over the canvas's bottom-left corner from the Git button at the sidebar's foot (see
> [Layout](#1-layout-and-navigation)). Its content is the one below; where this spec says "the block", read "the
> workspace's Git". Git blocks, their zoom and their card at far zoom are gone.

## Goal

The Git block becomes the way to work with Git in Templum: reviewing what an agent changed, committing and
pushing, switching and comparing branches, understanding the history, and never losing work by a wrong click,
for people who know Git well and for people who don't.

**Who opens it, and why**

1. **Reviewing what an agent did:** an agent finished; the user wants to see everything that changed, file by
   file, and keep or throw away parts of it.
2. **Committing and pushing:** choosing what goes in, writing the message, commit, push; fast, by keyboard.
3. **Branches and comparison:** switching and creating branches, seeing what a branch has that `main` doesn't
   (a pull request's diff), resolving conflicts.
4. **Understanding the past:** what a commit changed, who changed a line, how a file evolved, how branches split
   and joined.
5. **Not knowing Git well:** safe, explained actions: switching branches without losing files, undoing a commit,
   getting a discarded change back.

**Success criteria**

1. Every behavior below works on Linux, macOS and Windows; CI checks it on the 3 OS and Linux (Hyprland) is also
   checked by hand.
2. No action loses work silently: whatever discards or overwrites keeps a copy the user can restore.
3. The diff reads like GitHub's: the file's language colors, old and new line numbers, changed words marked.
4. `--bench` and a new Git review scenario stay at p95 ≤ 16.7 ms (see [ADR-0002](../adr/0002-ui-framework.md)).
5. All tests in [Testing](#testing) pass in CI on the 3 OS.

## Out of scope

- Staging single lines (hunks only), interactive rebase, cherry-pick from the interface, submodules, Git LFS.
- GitHub (or GitLab) pull requests and issues; signing commits; image previews in the diff.
- A view of "only what the agent changed" separate from the user's own uncommitted changes.

## Words

The block keeps Git's own words (Stage, Commit, Push, Pull, Fetch, Stash, Amend, Rebase, Merge), the ones
tutorials, the terminal and other tools use, so the user learns them. Each one has a one-sentence tooltip wherever
it appears as a button or a heading:

| Word | Tooltip |
|---|---|
| Stage | Choose this change to go into the next commit. |
| Unstage | Take this change out of the next commit; the file keeps it. |
| Discard | Throw this change away and go back to the last commit's version. You can restore it from Recently discarded. |
| Commit | Save a version of the project with the staged changes, and a message saying what they do. |
| Amend | Replace the last commit with this one (its changes and message), instead of adding a new one. |
| Push | Send your commits to the server (the remote), so others can get them. |
| Pull | Get the server's new commits into this branch. |
| Fetch | Look at what changed on the server, without changing your files. |
| Sync | Pull, then push: your branch and the server's end up the same. |
| Stash | Put your uncommitted changes aside, to bring them back later. |
| Rebase | Put your commits on top of the others', as if you had started after them. |
| Merge | Join another branch's commits into this one. |

Risky actions say what they will do before doing it, in one plain sentence (see [Safe actions](#4-safe-actions)).

## 1. Layout and navigation

- **Where it shows:** a popup over the canvas's bottom-left corner, beside the sidebar, at the **Size**
  below, drawn at a fixed scale: it never zooms with the canvas. The Git button (GitBranch icon) at the
  sidebar's foot, apart from the workspaces under a hairline, or Ctrl+Shift+G (Cmd+Shift+G), opens and closes it;
  so do Esc, while the Git has the keyboard and no field does, and its ✕.
- **Size:** 760 × 540 until the person drags it to another. Its **right edge**, its **top edge** and the corner
  between them resize it, each showing the matching resize cursor on hover; the left edge rests against the sidebar
  and the bottom one against the window's, so the popup grows to the right and upward, its bottom-left corner
  staying where it is. It is never smaller than 600 × 420 and never bigger than the window less the 8 px it keeps
  from the window's edges and the sidebar (a window with less room than the minimum gets the room). The size is
  **kept** in `workspaces/index.json` (`git_size`, see [workspaces.md](workspaces.md)), written when the pointer is
  let go and shared by every workspace's Git, so closing and opening the popup, or restarting Templum, gives it
  back. Only the size **drawn** is clamped to the window: a smaller window shrinks the popup, and a larger one
  gives the size set back; a drag keeps what it sets inside the room it was dragged in, so the pointer going back
  shrinks the popup at once. The Split diff's sides follow the popup's width.
- **One Git per workspace:** switching workspace shows that workspace's repository. The first time the popup opens
  in a workspace it picks the focused terminal's folder, else another terminal's, else the workspace's folder, else
  the newest recent folder; with none, the folder dialog asks. Its status is read every 2 s only while the popup is
  open, and when a terminal's command ends in the repository while it is open.
- **Header:** a tile with the repository icon in the theme's accent, then the repository folder's name with ▾. A
  click on it opens the **folder menu**: the repositories this workspace's terminals work in, then the workspace's
  recent folders (each once, the one shown left out), then Choose another folder…, Refresh, Open a terminal here
  (at the view's center; the popup closes). The header's right side has the actions below and ✕.
- **Saved** in the workspace's file as an optional `git` object (`session::SavedGit`: folder, tab, summary,
  description, the left column folded, Split). An older file's Git block item becomes the workspace's Git (its
  folder, tab and commit message draft) and no block; items of kind `git` are never written again.
- **Under the name,** dimmed, one line:
  - **the current branch.** A click opens the **branch dialog**, which switches and creates: a search field over
    the local branches, then the remote ones, each row drawn as in the Branches tab (the name, bold with a check
    for the current one, and ↑↓ against its upstream; under it the last commit's subject and age; the check is the
    accent's in the tab and the row's text color in the dialog, whose selected row is filled with the accent, its
    text the accent text color of [the design system](design-system.md) (`accent_foreground`); the first 50 matches, then one row "N more: keep typing to narrow them"; the branch the text names, a
    remote's without its `origin/`, is the first of its group, and the Remote group goes first when only its first
    row is that);
    Enter or a click switches (with [the switch flow](#switching-branches)). A name no branch has gets a row
    **Create branch "name"**, which makes the branch at HEAD and switches to it (git's own message when it refuses
    the name); a name that a remote branch has, without its `origin/`, gets none (switching to that one makes the
    tracking branch). Opened from the branch name, the row comes last, so Enter right after typing still takes the
    first match. **+ New branch…** (Branches tab) and **Create a branch here** (the old-version banner) open the same
    dialog with the row **first**, for any name typed, so Enter right after typing never switches: it makes the name
    typed even when another branch holds it (`login` while `feature/login` or `origin/login` exists), and git
    refuses a local name that is taken in its own words, HEAD staying where it is (from a detached HEAD a switch
    would leave its commits behind). A remote branch's full name (`origin/login`) is refused too, saying so: a
    local branch of that name would make `origin/login` ambiguous for every comparison and merge. **New branch here…** (a commit's menu)
    opens it too; from a commit it lists no branches (there is nothing to switch to from there) and creates at that
    commit.
  - **where it stands with the server:** "↑2 ↓1", "up to date", or "not published", with a tooltip in words
    ("2 commits of yours not pushed yet, 1 on the server you don't have").
  - the terminals working in the repository, while any: a click lists them.
- **Header actions,** left to right:
  - **Compare with `main`** (an icon, named by its tooltip), while the current branch isn't the default one (see
    [Comparison](#comparison)).
  - **Sync**, the main button: pull then push (see [Sync](#sync)). It is **Publish branch** while the branch has
    no upstream (see [Sync](#sync) for the remote it goes to). Tinted with the accent while there is something to
    send or get (not published, ↑ or ↓), quiet when up to date.
  - **↻** refresh, then **⋯**: Fetch, Pull, Push, Recently discarded…, Clone a repository….
- **Tabs:** Changes (with the count of changed files), History, Branches, Stashes, each with its icon; the shown
  one underlined in the accent.
- **Banner** under the tabs during a merge, rebase, cherry-pick or revert, or on a detached HEAD: a yellow strip,
  one sentence on what is happening and its buttons (Abort, Continue; Create a branch here, Back to *default*). A
  failed read shows the same way, in red.
- **Body:** a left column, 300 world units, with the tab's list and, in Changes, the commit area under it; and the
  **diff panel** on the right, the same component in every tab. A button at the panel header's left folds the
  column (but in Branches), so the diff takes the whole width, and unfolds it in the same place; the block
  remembers it.
- **Keyboard,** while the Git has the focus. It has a focus handle of its own: opening the popup, or a click in
  it, focuses it, its key handler takes the keys below and stops them, and the workspace's shortcuts still work, as
  in an editor block. `copy` copies the diff's selection while the Git has the focus.

| Key | Does |
|---|---|
| ↑ / ↓ | previous / next file in the list (the diff scrolls to it) |
| `[` / `]` | previous / next hunk |
| `s` / `u` | stage / unstage the hunk under the keyboard cursor, or the selected file |
| `d` | discard (asks first) |
| Enter | open the file in an editor, at the hunk's line |
| `c` | focus the commit summary |
| Ctrl+Enter (Cmd+Enter) | commit |
| Ctrl+Shift+C (Cmd+C) | copy the diff's selection |

  The keyboard icon in the diff panel's header names them in its tooltip. They only act while the Git has the focus
  and no field does.
- **Empty states** say it in words with a useful button: "Nothing changed since the last commit" (History, switch
  branch), "No stashes", "No other branches" (+ New branch…, which opens the branch dialog).
- **Zoom:** none; the popup keeps its scale whatever the canvas's zoom.

## 2. The diff panel

One component shows four sources: the **working changes** (unstaged and staged), **a commit** (against its first
parent), **a stash**, and **a comparison** of two branches.

- **Panel header:** the fold button, a title ("Working changes · 4 files · +120 −30", "Commit a1b2c3d · 3 files",
  "feature vs main · 7 commits · 12 files"), and icons named by their tooltips, shown while there is a diff:
  **Unified / Split** (one column, or side by side; remembered by the block), **Ignore whitespace** (`-w`; kept
  while on, since it can hide every change), **Expand all / Collapse all**, and the keyboard's keys. An icon that
  is on is tinted with the accent.
- **Every file in one scroll.** Each starts with a **file row:** a fold arrow, the path (its folder dimmed), the
  status letter in its colored badge, `+n −m`, and its actions (Stage/Unstage, Discard, Open in editor; only the
  ones that apply to the source). A click on the left column's file scrolls the panel there.
- **Sticky title:** the panel's top shows the path of the file being read, whatever the scroll.
- **Hunks:** a hunk row with where it is (the function or heading git names after `@@`, else "Lines 12–20") and,
  for working changes, **Stage** / **Unstage** / **Discard** for that hunk (none on an untracked file, see
  Special files).
- **Lines:** two line-number columns (old, new), a `+`/`−` gutter, the text in the terminal's font with the
  **file's language colors**, a soft green or red background from the theme's ANSI colors, and **changed words**
  with a stronger background (a removed line paired with an added one in the same hunk).
- **Split mode:** removed lines on the left, added on the right, paired row by row; an addition with no removal
  leaves the left cell empty, and the other way round. A conflicted file and a blame stay unified (their parts
  aren't an old and a new side), and "No newline at end of file" isn't shown in Split. Each side is at most half
  the panel, so the added side is always in view; a longer line is cut at its side (Unified shows it whole).
- **Unchanged stretches** longer than 6 lines (3 of context on each side) fold into "⋯ 40 unchanged lines"; a click
  shows the whole file's diff (`-U` with the file's length).
- **Special files:** a rename shows "old.rs → new.rs"; a binary "Binary file · 12 KB"; a new or deleted file all
  green or all red; a mode change says so; "No newline at end of file" shows dimmed. An untracked file has no
  diff in git: the block builds one with a new file's headers (`diff --git`, `new file mode`, `--- /dev/null`),
  all green; its file row has Stage, Discard (asks first: the file is deleted) and Open in editor, and no hunk
  buttons.
- **Large and generated files** fold by default with **Show diff**: more than 2,000 changed lines, and
  `Cargo.lock`, `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `*.min.js`, `*.min.css`, `*.map`. Over
  `DIFF_LIMIT` (4 MB) a file says "Too large to show" with Open in editor. One read takes at most 200 new
  (untracked) files and 4 MB of them: past that, a new file's row says "New file · 12 KB · not read yet" with
  **Show diff** (an agent's build or an unpacked folder can be thousands of files).
- **Selecting text:** click and drag selects across lines (in Split mode, within one side); a double-click selects
  a word, a triple-click a line. A point maps to a row by the row height and to a byte in it by the shaped line
  (`ShapedLine::closest_index_for_x`, gpui-pre-0.3.8 src/text_system/line_layout.rs:78), so tabs (drawn as 4
  spaces) and wide characters select right; column arithmetic would not. Copy: Ctrl+Shift+C (Cmd+C), or the
  right-click menu.
- **Right-click on a line:** Copy (the selection), Copy line, Copy hunk, **Send to ▸** (the selection, else the
  hunk, to a terminal, as [context-menus.md](context-menus.md) describes), Open in editor at this line.
- **Colors per hunk:** each hunk's old and new sides are highlighted as two fragments, on first draw, and kept
  until the diff changes. The block keeps one `SyntaxHighlighter` per language (gpui-kit's
  `SyntaxHighlighter::new(lang)`, gpui-component-0.7.1 src/highlighter/highlighter.rs:380) and runs each
  fragment through it (`update(None, &rope, None)` then `styles`, lines 597 and 1163), caching the styles per
  hunk: a highlighter per fragment would build two tree-sitter parsers per hunk. A hunk that starts inside a
  long comment or string may color its first lines wrongly; GitHub's does the same.

## 3. Changes: review and commit

- **Left column:** **Conflicts** (only during a merge or rebase with conflicts), **Staged**, **Changes** (unstaged
  and untracked), each with its count and Stage all / Unstage all / Discard all…. Rows as today (badge, name,
  folder dimmed, hover buttons, right-click menu). Over 15 files, a **Filter files** field above the lists.
- **Diff panel:** two sections in one scroll, **Changes** then **Staged**. A partly staged file is in both, each
  with its own hunks. In Changes a hunk has **Stage** and **Discard**; in Staged, **Unstage**.
- **Per hunk,** the block builds that hunk's patch (the file's header and the hunk), writes it to a temporary
  file (`Repo::run` gives git no stdin) and runs, with the file's path:

| Action | Command |
|---|---|
| Stage hunk | `git apply --cached <patch>` |
| Unstage hunk | `git apply --cached --reverse <patch>` |
| Discard hunk | `git apply --reverse <patch>` (after a copy in the trash) |

  If the file changed since it was read, git refuses the patch: the block refreshes and says "The file changed;
  try again", with git's message under Details (it refuses for other reasons too: CRLF, whitespace).
- **Discard never loses anything:** before any discard (a hunk, a file, all) the files are copied to the trash
  ([Safe actions](#the-trash)); the notification says "Discarded src/x.rs" with **Undo**.
- **Commit area** under the lists:
  - **Summary** ("Commit message"), with a quiet counter that warns past 72 characters; **Generate ✨**
    ([section 7](#7-commit-messages-from-an-agent)); **Description** under it once there is a summary (or a
    description, or Amend).
  - **One file needs no message.** When the commit holds exactly one file (what the button counts) and the summary
    is empty, Commit works and commits as **"Update a.rs"**, **"Add a.rs"** (a new file) or **"Delete a.rs"**, by the
    file's name and what happens to it ("Rename a.rs to b.rs" for a rename, "Move a.rs to src/" when only the folder
    changed). The summary field's placeholder shows that sentence in place of "Commit message", so the person knows
    what will be used. With no files or two or more, while amending, and during a merge, rebase, cherry-pick or
    revert (which prepare their own message), a summary is required as before; a summary that is typed always wins.
    The sentence is made from what is staged when the commit runs (after Commit all changes has staged), not from
    the status the form last read: if that is no longer exactly one file (an agent wrote another in between), nothing
    is committed, a notification says the files changed, and the summary gets the keyboard if the person is still in that workspace with the Git popup open.
  - **Commit,** the whole column's width, filled with the theme's accent once committing is possible, quiet before.
    Its ▾: **Commit**, **Commit & Push** (a Publish when the branch has no upstream), and **Amend the last commit**
    (fills the fields with the last commit's message; while on, a strip says "Amending the last commit" with ✕).
    With nothing staged and something changed, the button is **Commit all changes (5 files)**: it stages
    everything, new files included (`git add --all`), then commits.
  - After a commit: "Committed a1b2c3d" with **Undo commit** ([Undoing a commit](#undoing-a-commit); only while
    that commit isn't on the upstream). The fields clear; the draft is kept across restarts as today. Undo puts
    the undone commit's message back in the fields, to fix and commit again.
- **Open in editor** on a file row and on a hunk opens the file at that line in an editor of the workspace (one
  whose folder holds it, else any, else a new one at the view's center; the camera goes there).

## 4. Safe actions

### The trash

- Before any discard, the affected files (their current content, or the fact that they didn't exist) are copied
  to `trash/` in Templum's data folder, with a manifest: the repository's root, the paths, the time, what was
  done. Untracked files are covered too.
- **Undo** on the notification restores at once. **Recently discarded…** (the top bar's ⋯) lists the entries,
  newest first, each with **Restore**; a file changed since asks before it is overwritten.
- Entries older than 7 days, or beyond the newest 50 of their repository, are deleted when Templum starts and after each discard. The copies a Templum API call makes (marked `api`) are deleted by age only, never by count: they may hold the person's work (a hand-merged file a call resolved over), and more calls must not push them out. With 50 of them younger than 7 days for a repository, a call that needs a copy is refused before it changes anything; the person restores or clears them.
- **When a copy can't be kept:** without a data folder (`session::dir()` is None, as in `--bench`), or over 500
  files or 100 MB in one discard (`status -uall` lists every untracked file: a build folder is thousands), the
  question says so ("Too much to keep a copy: 1,240 files. This can't be undone.") and the notification has no
  Undo.

### Switching branches

- **With uncommitted changes,** the block always asks: "You have 3 changed files. What should happen to them?"
  - **Leave them on `main`:** stashed as `templum: left on main` (untracked files too) and, when the user comes
    back to `main`, popped automatically (`git stash pop`: gone once applied; a conflict keeps it and says so),
    with a notification. With several stashes of that name (an earlier pop that conflicted), the newest is
    popped and the others stay in Stashes.
  - **Bring them along:** `git switch` carries them; if git refuses (they'd be overwritten), it says why and offers
    the first choice.
  - **Commit first:** cancels the switch and focuses the commit summary.
  - **Cancel.**
- **With commits not pushed** on the branch being left, it switches and a notification says "`main` has 2 commits
  not pushed yet", with **Push `main`**. With both, the question above shows that line too.
- A remote branch with no local one creates the local tracking branch.

### Undoing a commit

**Undo commit** (the notification after a commit, or the right-click on History's newest commit) runs
`git reset --soft HEAD~1`: the commit goes, its changes stay staged. Two cases need another command: the
repository's first commit has no parent, so it is `git update-ref -d HEAD` (same result); and after an **Amend**
it is `git reset --soft HEAD@{1}`, so the commit the amend replaced comes back (`HEAD~1` would lose it). "On the
upstream" is `git merge-base --is-ancestor <commit> @{u}`; once it is, the item is **Revert…** instead, which
explains: "Pushed commits can't be removed for others; this adds a new commit that undoes it".

### Sync

- **Sync** = `git pull --ff-only`, then `git push`; the notification says what happened ("Got 3 commits, sent 2"),
  counted from the status's ahead and behind before and after, not from git's output. As built: `git fetch --prune`,
  the status (its ahead and behind are then exact), `git merge --ff-only @{u}` when behind, `git push` when ahead:
  one network read, and a diverged branch is found before anything changes.
- **Diverged** (fast-forward impossible): "Your branch and the server's both have new commits." with **Put my
  commits on top (rebase)** (recommended; `git pull --rebase --autostash`, since a rebase refuses to start over
  uncommitted changes) and **Merge**.
- **No upstream:** the button is **Publish branch**: `git push -u <remote> <branch>` to the repository's only
  remote; with several (`git remote`), the button's menu lists them; with none, "No remote" with **Run in a
  terminal**. Today's code and the Branches tab's Publish fix `origin`; they take the same rule.
- Credentials as today: git never waits for a password; a failure that wanted one offers **Run in a terminal**.

### Risky actions say what they do

- **Discard all…:** "Discard 7 files? You can restore them from Recently discarded."
- **Delete branch:** one `git branch -d` refuses asks again: "Its 3 commits not merged anywhere will be lost."
  (`git rev-list --count <branch> --not --exclude=<branch> --branches --remotes`).
- **Checkout of a commit:** the banner: "You're looking at an old version. Commits made here won't belong to any
  branch." with **Create a branch here** and **Back to `main`**.

### Starting without a repository

"Not a Git repository" offers **Initialize** and **Clone a repository…** (also in the top bar's ⋯): a URL and a
destination folder (default: the chosen folder; an empty or missing folder takes the clone itself, one holding files
gets a folder named after the repository inside it, as `git clone <url>` does), cloned in the background, the button spinning meanwhile and a click on it
cancelling, as Fetch's does (git writes its progress to stderr, which the block's runner reads only when the
command ends; a progress bar would need a streaming runner, not worth it for a clone); when done, the Git opens
on it. A clone that wants credentials fails with **Run in a terminal**, as other commands do.

## 5. Branches, comparison and conflicts

### Branches tab

- **Local** first: the current one marked; each with ↑↓ against its upstream and its last commit's subject and
  age (`BRANCH_FORMAT` gains `%(committerdate:unix)`; it has no date today). **Remote** in a folded group. A
  filter field.
- **Click:** switch, with [the switch flow](#switching-branches). **+ New branch…** opens the
  [branch dialog](#1-layout-and-navigation) (and, in History, a commit's **New branch here…** opens it to create
  at that commit).
- **Right-click:** **Compare with `main`**, Merge into *current*…, Rename…, Delete…, Publish, Copy name.

### Worktrees an agent made

`spawn_agent`'s `worktree` makes `.worktrees/<name>` on a branch of that name (connections.md). An agent removes it
with `remove_worktree` (at once when it has no changes and its commits are merged; losing work only once the person
says Delete in its balloon) and merges it back with `merge_worktree` (into the branch of its own folder, once the
person says Merge; a conflict is aborted, naming the files): [agent-tools.md](agent-tools.md#remove_worktree-and-merge_worktree).
Neither runs while a terminal block works in the worktree.

### Comparison

- **Compare** (in the Branches tab: its column becomes the comparison's commits, the panel its diff, a ← goes back)
  shows what a branch has that the base doesn't: the left column lists its commits
  (`git log base..branch`), the panel its diff (`git diff base...branch`), as a pull request does. A click on a
  commit narrows the panel to it; a click on the heading shows them all again.
- **The base** is the default branch: what `git symbolic-ref refs/remotes/origin/HEAD` names (`parse_branches`
  leaves `origin/HEAD` out, so it is its own command), else `main`, else `master`. A selector changes it.

### Conflicts

- A merge, rebase or pull that stops on conflicts shows the banner: "Git couldn't join the changes in 2 files by
  itself. Choose which version stays in each part." and the **Conflicts** group in Changes.
- **The panel, on a conflicted file,** shows each conflict region (`<<<<<<<`, `|||||||` when present, `=======`,
  `>>>>>>>`) as two parts named after the markers' labels (`<<<<<<< HEAD` is **HEAD**, `>>>>>>> feature` is
  **feature**), each highlighted, with **Use this** on each, **Use both** and **Edit in editor**. Never "yours" and
  "theirs": during a rebase, HEAD is the branch being rebased onto and the other side is the user's own commit,
  so those words would be backwards. A choice rewrites that region of the file (a copy goes to the trash first).
- With no region left, **Mark resolved** stages the file; with no conflicted file left, the banner shows
  **Continue**. The whole-file choices stay in the right-click menu (`git checkout --ours` / `--theirs`), named
  the same way: "Use the version from HEAD", "Use the version from feature".

## 6. History

- **Commit list with a graph:** a column of colored lanes on the left, computed from each commit's parents
  (already in `LOG_FORMAT`'s `%P`), shows where branches split and join. The log runs with `--date-order`
  (children before parents, or lanes cross back; the default order doesn't promise it) and every page shares
  it, so **Load more** continues the lanes. The current branch by default; **All branches** is
  `--branches --tags --remotes` (not `--all`, which has the stashes). Each row: subject, ref badges (branches,
  tags, HEAD), author, age, short hash; commits not on the upstream (`git rev-list @{u}..HEAD`) have a "not
  pushed" mark. 200 at a time, **Load more**.
- **Click a commit:** the left column shows its full message, author, date and files; the panel its whole diff.
- **Right-click a commit:** Checkout…, New branch here…, Create tag…, Revert…, **Undo this commit** (the newest,
  not pushed), Copy hash, Copy message.
- **Search** runs in git, not over the loaded commits, in three modes: **Message** (`--grep -i`), **Author**
  (`--author -i`), **Code** (`-S`: commits that added or removed a text). It runs on Enter, and again when the
  field is cleared (the whole history back); the text is taken as written (`-F`). A search hides the graph: its
  commits' parents aren't in the list.
- **File history:** right-click a file (in Changes, in a commit's files, in an editor's tree) → **File history**:
  History narrowed to it (`git log --follow --name-status -- path`, so each commit knows the file's path at that
  time, before a rename), the panel showing that file's change in each commit: a commit clicked there shows that
  file's change alone, by its name then (the commit's other files are in the left column). A file's history has
  no search and no All branches (`--follow` follows one file). **Restore this
  version…** writes the file as it was in that commit (`git show rev:<path then>`, written to the path now), after
  a copy in the trash.
- **Blame:** right-click a file → **Blame**: the panel shows the whole file with the diff's rows (colors,
  selection) and, for each group of lines from one commit, its author, date and short hash
  (`git blame --porcelain`). The panel's rows are one column of text, so they are a heading row above the group
  ("a1b2c3d · Ada Lovelace · 3 d ago · Subject"; "Not committed yet" for lines not committed). A click on a
  group's heading opens that commit (none for the lines not committed).

## 7. Commit messages from an agent

### In Settings ▸ Agents

Each agent's dialog (Templum's and the user's) gets a **Commit messages** part:

- **Generate command:** how to run the agent's CLI non-interactively, a template with placeholders:
  - `{model}`: the model chosen;
  - `{prompt_file}`: a temporary file holding the request; without it, the request goes to the standard input;
  - `{output_file}`: where the CLI writes its answer; without it, the answer is the standard output.
- **Models:** the list, one per line, the first being the recommended one.
- **Templum's presets** (editable, with ↺ back to them):

| Agent | Generate command | Models |
|---|---|---|
| Claude Code | `claude -p --model {model} --tools= --setting-sources=` | `haiku` (recommended), `sonnet`, `opus` |
| Codex | `codex exec -m {model} --sandbox read-only --ephemeral --skip-git-repo-check -o {output_file} -` | read from `~/.codex/models_cache.json` (entries with `"visibility": "list"`), else the list written |

- A user's agent gets the same fields, empty; once filled, it is offered in Settings ▸ Git.
- **Running:** through the user's login shell (`$SHELL -lc` on Unix, `/bin/sh` when `$SHELL` is unset, as for
  an app opened from a desktop menu; PowerShell on Windows), as typed in a terminal, so PATH, aliases and
  version managers work; placeholders are replaced by quoted paths. Its own runner, not `Repo::run`: that one
  gives the command no stdin and git's environment.
- **No credit to the agent:** the command runs with `CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS=1`, so Claude Code
  leaves out its own commit rules, which end every message with a `Co-Authored-By: Claude` trailer. Templum's
  rules (the request) and the Instructions are the only commit rules; someone who wants the trailer asks for it
  there.
- **A one-shot answer:** Claude Code's preset runs with no tools (`--tools=`) and none of the person's settings
  (`--setting-sources=`: their hooks, plugins and MCP servers), so it answers the request instead of starting a
  session in the repository (before, with them, it asked "what do you want me to do with this diff?", reviewed
  the change in the person's language, or ran the tests and git). `=` and no value, as PowerShell 5.1 drops an
  empty `""`. A provider set only in Claude Code's settings (its `env`) isn't read then: it goes in the shell's
  environment, or the command is edited. Codex's preset already runs read-only and ephemeral.

### In Settings ▸ Git ▸ Commit messages

- **Agent:** the agents with a generate command. **Model:** its list, the recommended one marked; another can be
  typed.
- **Timeout** (default 60 s) and **Max diff size** (default 60 KB).
- **Instructions:** the user's commit rules ("Prefix the summary with the ticket id", "In Portuguese"), on top of
  Templum's own, which they win over where they disagree.
- **Prompt (advanced):** the request's text, editable, with the placeholders `{instructions}`, `{files}`,
  `{diff}`, `{recent_commits}`, `{truncated}`; **Reset to default**.
- **Try it:** runs the command on a sample diff and shows the answer, or the whole error (exit code, stderr).

### Generate ✨

- Beside the summary, shown when the agent chosen in Settings ▸ Git has a generate command and its program is
  installed.
- **The request:** Templum's rules for a good commit message first: a one-shot request, not a conversation (ask
  nothing, run no tools, explain nothing); the summary in Conventional Commits form, `type(scope): summary`, at
  most 72 characters, imperative, no period; a blank line and a short body wrapped at 72 saying what changed and
  why, when the summary doesn't say it all; plain text (no Markdown, fences or quotes); no credit to an AI or a
  tool; the repository's own types, scopes, language and casing, or its own convention if it follows another.
  Then the 10 last commit subjects, the changed files, and the staged diff (with **Commit all changes**, every
  change), cut at Max diff size with the full file list and `{truncated}` saying so. Last, after the diff, where an
  agent heeds them best (in real runs, rules before a long diff were half ignored), the instructions (the
  repository's own when set, else the global ones), which win over Templum's rules.
- **Runs in the background;** the button spins and a click on it cancels. The answer fills Summary and
  Description for the user to read; nothing is committed. A failure or a timeout: a notification with Details.
- **First use:** once, a confirmation says the diff is sent to the agent's provider and uses the user's account;
  agreeing writes `confirmed = true` under `[git.commit_message]`.
- **The ⚙ beside it:** the model for now, and **Instructions for this repository** (overrides the global ones in
  this repository only).

## On a mirrored workspace

The popup opens on a mirrored workspace (docs/specs/templum-to-templum.md, Batch 2g) and looks and acts as it does here,
but every read and act is an operation of home's `git` tool (docs/api.md): nothing runs or is read on this computer,
where home's path may be another folder. It opens on the focused terminal's folder at home, else the first terminal's,
else the workspace's; the folder ▾ lists home's folders (the workspace's, its terminals', then home's recent folders of
the workspace, read when the popup and the menu open), Choose another folder… stays on home, Refresh and Open a terminal
here (at home) work. The status is read every 2 s while the popup is shown and the remote is live; the diff when the
status changes, every 10 s, and when asked (opening, Refresh, whitespace, a whole file); the other tabs' lists as the
tab asks. Fetch, Pull, Push, Sync, Publish and Generate go on the slow lane (10 minutes) and can't be stopped from here:
the spinning button says so. Generate uses home's agent (it asks there, once, whether the diff may go to the agent's
provider); its ⚙ stays on home. Recently discarded… is home's trash, Send to lists home's terminals. What a Control
share can't do (Discard, Restore this version, deleting a branch, dropping a stash, Abort) is dimmed with the rule's
words and refused before any question; Clone, Open in editor and Run in a terminal are left out. A failed command's
Details shows git's stderr as home gave it. The folder, tab and message live in memory only.

## Configuration

```toml
[agents]
claude = { models = ["haiku", "sonnet", "opus"] }   # only what differs from Templum's preset
my-agent = { name = "My agent", command = "my-agent", generate = "my-agent ask -m {model} -f {prompt_file}", models = ["fast", "smart"] }

[git.commit_message]
agent = "claude"
model = "haiku"
timeout = 60          # seconds
max_diff_kb = 60
instructions = "Conventional Commits, English, imperative."
confirmed = true      # the first-use confirmation was accepted
# prompt = """…"""   # only when changed

[git.repositories."/home/u/proj"]   # the root as `git rev-parse --show-toplevel` prints it: `/` on every OS
commit_instructions = "Prefix the summary with the ticket id."
```

- `generate` and `models` follow the `[agents]` rules of [agents.md](agents.md): for one of Templum's, a missing
  key keeps the preset, and saving it back at the preset removes it. Codex's preset list comes from its cache and
  changes by itself, so its `models` is written only when the user edits the list and removed only by ↺.
- A `generate` that is set can't be empty; `timeout` is 5 to 600; `max_diff_kb` is 4 to 1024. Errors are reported
  like any other config error.
- `[git.repositories]` is keyed by the root with `/` on every OS (`C:/Users/u/proj` on Windows), and
  `config::Edit` takes an owned table name (it is `&'static str` today) so the settings can write that table.

## Files

The canvas's Git item gains two keys, both optional (an older file loads as before):

```json
{ "kind": "git", "rect": {…}, "folder": "/home/u/proj", "tab": "changes",
  "diff": "split", "lists": "folded" }
```

## Architecture

The rule of [ARCHITECTURE.md](../ARCHITECTURE.md): what can live without the UI is in a library crate and tested
headlessly.

**`templum-git`** (no UI):

| File | Holds |
|---|---|
| `diff.rs` | Parsing `git diff` output into `FileDiff { old_path, new_path, status, binary, mode, hunks }`, `Hunk { old_start, old_len, new_start, new_len, context, lines }`, `Line { kind, old_no, new_no, text }`; and `hunk_patch(file, hunk)`, a hunk's patch for `git apply`. |
| `words.rs` | The changed ranges of a removed line and its added line (a longest common subsequence over words and punctuation). |
| `conflict.rs` | Conflict regions of a file's text (with or without the base part), each side with its marker's label, and the text with one region resolved (one side, or both). |
| `graph.rs` | Lanes for a list of commits and their parents: each row's lane, and the lines into and out of it. |
| `trash.rs` | The trash: put (copies and a manifest, refused over the cap), list, restore (with "changed since" detection), prune. |
| `repo.rs` | New commands: diff by source (working, staged, commit, stash, range; an untracked file built with a new file's headers), apply a patch from a temporary file (cached, reverse), blame, file log with the path at each commit, log search, log with `--date-order`, default branch (`symbolic-ref`), remotes, on the upstream, undo commit (its three forms), clone, switch with a stash per branch, a date in `BRANCH_FORMAT`. |
| `message.rs` | A commit message from an agent: the request from the template and its placeholders, the command through the login shell with a timeout and cancel (stdin or a prompt file, stdout or an output file), the answer split into summary and description. No UI: tested with a script that writes a known answer. |

**`templum`** (the UI):

| File | Holds |
|---|---|
| `git_block/diff_view/` | The diff panel: its rows (file, hunk, lines, folded stretch), unified and split layouts, highlighting with the cache, the selection, the sticky title. |
| `git_block/*.rs` | The tabs reorganized around the left column and the panel; the block's focus handle and key handler; the branch dialog; the banners; the Generate button, its spinner and the first-use confirmation (what `message.rs` runs). |
| `config.rs`, `settings/` | `[agents]`'s `generate` and `models`; `[git.commit_message]`; `[git.repositories]` (an owned table name in `Edit`); the Git section and the agents dialog's new part. |
| `workspace/` | `copy` for the focused Git's selection; `git_ops.rs`: opening, closing and pointing the workspace's Git (`toggle_git`, `open_git`, `close_git`, `set_git_folder`, `git_folders`, `poll_git`); `render.rs` places the popup; `input.rs` resizes it (`begin_git_resize`, `release_git_resize`, `git_popup_size`). |
| `sidebar.rs` | The Git button at the sidebar's foot. |
| `session.rs` | `SavedGit`: the workspace's Git, with `diff` and `lists`; `Index.git_size` (`SavedSize`): the popup's size. |

**Data flow:** every git command still goes through the block's queue (`git_job`/`act_job`, one at a time, results
dropped when the block changed folder); a diff is read in the background, parsed in `templum-git`, and turned into
rows on the UI thread; highlighting runs per hunk the first time it is drawn, and is kept until the diff changes.
The 2-second read fetches the whole working diff, not one file's as today: a text equal to the shown one changes
nothing (no rows, no highlighting, the scroll and the selection stay), as `reload_git_diff` already does.

## Error handling

- Every failed command: the notification of today, with Details and Run in a terminal.
- A hunk patch git refuses: refresh, and "The file changed; try again", with git's message under Details.
- A restore over a file changed since: asks before overwriting.
- A generate command that fails, times out, or answers nothing: a notification with its exit code and stderr; the
  fields are left as they were.

## Testing

**`templum-git`, unit, on recorded output:**

- `diff.rs`: a modified file, several hunks, a rename with changes, a pure rename, a binary file, a new and a
  deleted file, a mode change, CRLF lines, "\ No newline at end of file", a file name with spaces; `hunk_patch`
  applied with `git apply --cached --check` in a temporary repository for each.
- `words.rs`: changed words in a line, an inserted word, a whole line changed, Unicode text.
- `conflict.rs`: two-way and three-way markers, their labels, several regions, each resolution.
- `graph.rs`: a straight line, a branch and its merge, an octopus merge, a root commit in the middle.
- `trash.rs`: put, list, restore, "changed since", pruning by age and count, the cap.
- `message.rs`: the request from the template (every placeholder, the cut diff), the answer split, a command
  that answers, one that fails, one that times out, one that writes to the output file.

**`templum-git`, integration (`tests/repo.rs`), on real temporary repositories:** stage, unstage and discard of
one hunk of a file with three, and of a file with CRLF line endings; diff of a commit, a stash, a range and an
untracked file; undo commit after a commit, after the first commit, and after an amend; on the upstream or not;
switch with a stash per branch and back, popped; default branch with and without `origin/HEAD`; the only remote
and several; blame; file log through a rename, with the path at each commit; log search in each mode; a log in
date order.

**`templum`, headless UI (`workspace/tests/git_*.rs`):**

- the diff panel: rows for a sample diff, Unified and Split, folding and expanding, the sticky title, selection
  and copy, Send to;
- each hunk button, an untracked file's row without them, the trash's Undo and Restore, a discard over the cap
  and one without a data folder, Commit all changes, Commit & Push, Undo commit (its message back in the fields);
- the branch dialog and the switch question's four answers; the unpushed-commits notification;
- Sync's fast-forward and diverged cases, Publish to the only remote and the menu with several, Clone;
- Compare, conflicts region by region with the markers' labels (a merge and a rebase), Mark resolved and
  Continue;
- History's graph rows, search modes, file history, Restore this version, Blame;
- Generate with a fake agent command (a script that writes a known answer), its timeout and failure, the
  first-use confirmation written once; the settings' Try it;
- the keyboard keys.

**Manual (Hyprland):** a real agent session reviewed and committed; Generate with Claude Code and Codex.

## Performance

- The panel is a virtual list: only the rows on screen are built.
- Reading and parsing a diff, and running an agent, happen off the UI thread.
- Highlighting is per hunk, on first draw, cached.
- `--bench-git` (the Git popup on a 5,000-line diff, its panel scrolling, Unified then Split) stays at
  p95 ≤ 16.7 ms.

## Delivery

One plan per batch, each usable on its own:

| Batch | Delivers |
|---|---|
| 1 | The diff panel (Unified) in Changes, with selection, folding, special files, highlighting |
| 2 | Hunk stage and discard, the trash, the new layout and size, the keyboard, Commit all changes, Commit & Push, Undo commit |
| 3 | Safe actions: the branch dialog and the switch question, Sync, Publish, Clone, tooltips, the detached-HEAD banner |
| 4 | Branches tab, Compare, conflicts region by region |
| 5 | History with the graph, search modes, file history, Restore this version, Blame, Split mode |
| 6 | Commit messages from an agent: Generate, Settings ▸ Git, the agents dialog's Commit messages part |
