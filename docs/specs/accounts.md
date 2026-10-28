# Agent accounts

- **Date:** 2026-10-18
- **Status:** Implemented (kanban card 37, "Múltiplas contas do claude"):
  [docs/plans/2026-10-18-agent-accounts.md](../plans/2026-10-18-agent-accounts.md). Accounts by name, each a command
  in every shell, and the new dialog (kanban card 65, 2026-10-20): Implemented,
  [docs/plans/2026-10-20-accounts-by-name.md](../plans/2026-10-20-accounts-by-name.md)
- **Builds on:** [agents.md](agents.md) (the agents, Settings ▸ Agents, `[agents]`), [settings.md](settings.md),
  [usage.md](usage.md) (limits are an account's), [connections.md](connections.md) (`spawn_agent`, what is typed into
  an agent, agents typed in a terminal), [persistence.md](persistence.md#agents) and
  [kanban.md](kanban.md#who-a-card-is-for)
- **Shells** (kanban card 66): an account has a shell of its own, never WSL:
  [shells.md](shells.md#a-shell-for-each-agent)

## Goal

The card, in English: "Make something for this in Settings. Let a person have several Claude accounts, like claude,
claude1 and so on, and look at other agents too: Codex, Pi, Grok… It must be possible to see what such an agent
shares: a claude2 that is the same as claude but on another account, keeping the sessions, plugins and so on; a
claude3 with everything new, sharing nothing with the other two; a claude4 that shares everything of claude3. Make it
very customizable. Two Codex accounts too: codex opens one account, codex2 another, sharing the same sessions and
plugins or not."

A person who pays for two plans (work and personal, or two Max plans to get past a limit) opens either one from the
canvas as a separate agent, `claude` or `claude2`, and decides item by item what the second one takes from the first:
its settings, its instructions, its skills, its plugins, its sessions. Every agent below keeps everything in one
folder that a variable of its environment moves, so an account is that agent run with its own folder, some of whose
items are links to another account's. Its login is never one of them.

Card 65, in English: "The Add an Account dialog is bad, a bit confusing, asking the user to pick a folder. What I
wanted on this screen: the user wants to create the alias "claudinho", an alias for claude that opens another Claude
account; then they could have "claudio", which opens another Claude account. Also check whether we have a claude2
mock; I feel we do. The alias must work in every shell of the user's system, or a selector lists the shells and they
choose where it works."

So an account is made by **its name, which is also a command**: the person types `claudinho`, Templum makes the rest
(its folder, its links, the command), and `claudinho` typed in any terminal of the computer, in any shell, opens Claude
Code on that login. `claudio` is another. Nothing asks for a folder.

**Success criteria**

1. **Settings ▸ Agents ▸ Accounts** adds an account of any agent of [the table](#which-agents), edits what it shares,
   logs it in and deletes it.
2. An account is an agent like the others: **AI agent ▸** lists it, `spawn_agent` opens it by its id (`claude2`), a
   card can be for it, its block is titled by its name, and it comes back on its own session after a restart.
3. Each item of its folder is its own or another account's, with chains (claude4 takes claude3's, which takes
   claude's); a choice changed later never deletes anything.
4. Templum never reads, links, moves, copies or deletes an agent's login (its credentials and its account state).
5. Plan limits and their notifications are per account.
6. All tests in [Testing](#testing) pass in CI on the 3 OS.
7. **Add an account** asks for a name, and nothing else unless the person wants to choose what it shares; no folder.
8. That name is a command in every shell of the person's computer, Templum's terminals included, where it also gets
   Templum's server as a typed `claude` does; deleting the account removes it.
9. An account made before card 65 keeps its folder, its name and its shares, and gets its command when its id can be
   one ([Its name](#its-name)).

## Out of scope

- **Logging in for the person** or reading a login: the agent's own login runs in a terminal.
- **Accounts of an agent that keeps its login outside its folder** (the system keychain, not told apart by folder):
  two folders would be one account. [Which agents](#which-agents) leaves them out.
- **API keys:** an agent that only takes a key from its environment (Aider) has nothing to tell accounts apart but the
  key; the person sets it in their own shell.
- **A command, resume command, model list or commit-message command of an account's own:** it runs its agent's, as
  Settings ▸ Agents has them.
- **Renaming an account:** its name is its id, its folder's name, its command, and what canvases, cards and
  `spawn_agent` call it; a login on macOS is named by its folder's path. The person deletes it and adds it again,
  and logs in again.
- **Editing the person's shell startup files** (`.bashrc`, `.zshrc`, `config.fish`, the PowerShell profile) or
  Windows' Path: one file on PATH reaches every shell; when its folder isn't on PATH, the dialog shows the line to add
  ([Its command](#its-command)).
- **The command in WSL** from a Templum on Windows: WSL is another system, with its own PATH and agents.
- **Moving an account's content** when its folder changes, and **copying** an item from another account: Templum
  links or leaves alone; a copy is the person's to make.

## Words

- **An account:** an entry of `[agents]` that is another folder of one of Templum's agents ("its agent"), with an id
  (`claudinho`; `claude2` for one made before card 65) and a title ("Claude Code (claudinho)", or the `name` set).
- **Its command:** a small script named by the account's id in the commands folder, which runs its agent on its
  folder ([Its command](#its-command)).
- **The commands folder:** `~/.local/bin` (`%USERPROFILE%\.local\bin` on Windows).
- **Its folder:** the folder its agent's variable names while it runs, `<data>/accounts/<id>` unless the person
  chose one.
- **The agent's own folder:** where the agent keeps everything without Templum's account (its variable as Templum's
  environment has it, else its default, `~/.claude`; a variable naming one of its accounts' folders is a Templum
  started in that account's block, and the default is the agent's own). It is the agent's default account, whose id is the agent's
  (`claude`).
- **An item:** one file or folder of an agent's folder that an account may share, from [the table](#which-agents)
  (Claude Code's `skills/`).
- **Own / shared with X:** the item is the account's (a real file or folder in its folder, or nothing yet), or a link
  to the same item of X, the agent's own folder or another account of the same agent.
- **Its login:** the files of the agent's folder that hold its credentials and its account's state, which
  [the table](#which-agents) names; never an item.

## Which agents

Researched on 2026-10-18 from each agent's documentation and source, and, for Claude Code 2.1.286, Codex 0.154,
Gemini CLI 0.63, Copilot CLI 1.0.98, Grok Build 1.0.50, Crush 0.98.1 and Pi 0.85.1, on this machine (links in
[Sources](#sources)). An agent has accounts when one variable moves everything it keeps, its login included, and its
login is told apart by that folder.

| Agent | Variable: the folder it names | The agent's own folder | Its login: never touched | Log in types |
|---|---|---|---|---|
| Claude Code | `CLAUDE_CONFIG_DIR`: the folder | `~/.claude` | `.credentials.json`, `.credentials.lock`, `.claude.json`, `.config.json`, `backups/`, `remote-settings.json`, `policy-limits.json`, `stats-cache.json`, `mcp-needs-auth-cache.json`, `jobs/`, `daemon/` | `claude auth login` |
| Codex | `CODEX_HOME`: the folder | `~/.codex` | `auth.json`, `.credentials.json` | `codex login` |
| Gemini CLI | `GEMINI_CLI_HOME`: the folder that holds `.gemini/` | `~/.gemini` | `oauth_creds.json`, `google_accounts.json`, `gemini-credentials.json`, `mcp-oauth-tokens.json`, `a2a-oauth-tokens.json`, `.env` | `gemini`, then `/auth` in it |
| GitHub Copilot CLI | `COPILOT_HOME`: the folder | `~/.copilot` | `config.json`, `mcp-oauth-config/`, `mcp-secrets/` | `copilot login` |
| Pi | `PI_CODING_AGENT_DIR`: the folder | `~/.pi/agent` | `auth.json` | `pi`, then `/login` in it |
| Grok Build | `GROK_HOME`: the folder | `~/.grok` | `auth.json`, `mcp_credentials.json`, `managed_config.toml`, `requirements.toml` | `grok login` |
| Kimi Code | `KIMI_CODE_HOME`: the folder | `~/.kimi-code` | `credentials/`, `device_id`, `region` | `kimi login` |
| Crush | `CRUSH_GLOBAL_DATA`: the folder | its data folder (`~/.local/share/crush` on Linux) | `crush.json` | `crush login` |

The items each one shares (key, what the dialog calls it, its paths; a path ending in `/` is a folder). An item with
several paths shares them together: they only make sense as one.

- **Claude Code:** `settings` Settings (`settings.json`); `instructions` Instructions (`CLAUDE.md`); `rules` Rules
  (`rules/`); `skills` Skills (`skills/`); `agents` Subagents (`agents/`); `commands` Commands (`commands/`);
  `output-styles` Output styles (`output-styles/`); `hooks` Hook scripts (`hooks/`); `plugins` Plugins (`plugins/`);
  `keybindings` Keybindings (`keybindings.json`); `sessions` Sessions, for /resume (`projects/`, `file-history/`,
  `plans/`); `history` Prompt history (`history.jsonl`). Claude Code writes `settings.json` and `.claude.json`
  through a link (checked with 2.1.286). MCP servers added with `claude mcp add -s user` live with the login: an
  account adds them again, or keeps them in a project's `.mcp.json`. It also asks again to trust each folder.
- **Codex:** `config` Settings, MCP servers and profiles (`config.toml`); `instructions` Instructions (`AGENTS.md`);
  `hooks` Hooks (`hooks.json`); `rules` Rules (`rules/`); `agents` Agents (`agents/`); `skills` Skills (`skills/`);
  `plugins` Plugins (`plugins/`); `sessions` Sessions (`sessions/`, `session_index.jsonl`); `history` Prompt history
  (`history.jsonl`). Codex writes `config.toml` through a link (checked with 0.154); that file also keeps which folders
  and hooks the person trusts. Its databases (`state_5.sqlite` and the others) are never shared: two processes of two
  folders on one database is what Codex's lock bugs are made of. Codex's own list of sessions to resume reads a
  database of each folder: another account's shared sessions resume by their id. Skills in ~/.agents/skills are every
  account's.
- **Gemini CLI:** `instructions` Instructions (`GEMINI.md`); `commands` Commands (`commands/`); `skills` Skills
  (`skills/`); `extensions` Extensions (`extensions/`). Not `settings.json`: Gemini CLI writes it by a rename, which
  replaces a link with a file (checked with 0.63). Not its sessions: their folders are named from a map
  (`projects.json`) that it rewrites the same way. Its login is this folder's when the person signs in with Google (the
  default); an API key goes to the system's keychain, one for every folder.
- **GitHub Copilot CLI:** `settings` Settings (`settings.json`); `instructions` Instructions
  (`copilot-instructions.md`, `instructions/`); `agents` Agents (`agents/`); `skills` Skills (`skills/`); `hooks`
  Hooks (`hooks/`); `extensions` Extensions (`extensions/`); `mcp` MCP servers (`mcp-config.json`); `lsp` Language
  servers (`lsp-config.json`). Not its sessions (`session-store.db`, a database) nor its plugins (their list is in
  `config.json`, with the login). Its login is in the system's keychain, one per GitHub account, and `config.json`
  says which one this folder uses. Copilot CLI not logged in here uses the GitHub CLI's login (gh).
- **Pi:** `settings` Settings (`settings.json`); `keybindings` Keybindings (`keybindings.json`); `system` System
  prompt (`SYSTEM.md`, `APPEND_SYSTEM.md`); `extensions` Extensions (`extensions/`); `skills` Skills (`skills/`);
  `prompts` Prompts (`prompts/`); `themes` Themes (`themes/`); `tools` Tools (`tools/`); `sessions` Sessions
  (`sessions/`). Not `models.json`: a custom provider's key may be in it.
- **Grok Build:** `config` Settings (`config.toml`); `instructions` Instructions (`AGENTS.md`); `rules` Rules
  (`rules/`); `skills` Skills (`skills/`); `agents` Agents (`agents/`); `commands` Commands (`commands/`); `plugins`
  Plugins (`plugins/`, `installed-plugins/`); `sessions` Sessions (`sessions/`). Not `hooks/`: its sandbox refuses
  links among its hooks' paths. It follows a link inside its folder (checked with 1.0.50 on a temporary home:
  `grok inspect` lists a skill of a `skills/` linked to another folder's).
- **Kimi Code:** `config` Settings (`config.toml`); `instructions` Instructions (`AGENTS.md`); `mcp` MCP servers
  (`mcp.json`); `skills` Skills (`skills/`); `plugins` Plugins (`plugins/`); `sessions` Sessions (`sessions/`).
- **Crush:** none. Its login and the settings it writes are one file, `crush.json`: an account of Crush is a login of
  its own and shares nothing (its sessions are each project's, `.crush/`, already common to every account).

**Variables that win over every account's login,** named under the items when Templum's own environment has one (the
person's shell may too): Claude Code `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`,
`CLAUDE_SECURESTORAGE_CONFIG_DIR`; Copilot CLI `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_TOKEN`: "GH_TOKEN is set
in your environment: Copilot CLI uses it whatever this account's login."

**The macOS Keychain.** Claude Code keeps a login on macOS in the Keychain, under a name made from the exact text of
`CLAUDE_CONFIG_DIR` (documented; the same code since 1.0.30): Templum gives an account the same text every time (its
folder as Templum makes it, no trailing separator), so moving Templum's data folder or choosing another folder for the
account asks to log in again. Setting the variable to `~/.claude` would name another, empty entry: the agent's own
folder is the variable left unset. Copilot CLI's login is in the system's keychain on every OS, by GitHub account.

**Left out**, and why:

| Agent | Why |
|---|---|
| OpenCode, Kilo Code | No variable of their own: only `XDG_DATA_HOME`, which every program they start would inherit |
| Cursor CLI | `CURSOR_CONFIG_DIR` moves only `cli-config.json`; its login stays where it was |
| Mistral Vibe | Its key is in the system's keyring under one name for every folder |
| Amp | No variable for its folder; its login is in the keyring |
| Auggie | Only a flag (`--augment-cache-dir`) moves its login; no variable |
| Qwen Code | Its sign-in ended (2026-04-15): an account is an API key |
| Aider | API keys only |
| Kiro CLI | `KIRO_HOME` is ignored by its current version (its issue 10417); its login's place unconfirmed |
| Droid | Its variable (`FACTORY_HOME_OVERRIDE`) is undocumented |
| Cline | Two at once need a second variable (`CLINE_HUB_PORT`) for its fixed port |
| Goose | Its keyring entry is one for every folder unless a second variable (`GOOSE_DISABLE_KEYRING`) is set |

One variable per agent is the rule; Cline and Goose come with a second variable when someone asks.

## An account in config.toml

Accounts are entries of `[agents]` ([agents.md](agents.md#changing-ours-adding-yours)) with `of`:

```toml
[agents.claude2]
of = "claude"                      # the agent it is an account of
name = "Claude Code 2"             # missing: "Claude Code (claude2)"
show = true                        # the canvas's menu lists it
# folder = "~/.claude-work"        # missing: <data>/accounts/claude2
share = { settings = "claude", instructions = "claude", skills = "claude", plugins = "claude", sessions = "claude" }

[agents.claude3]
of = "claude"                      # no share: everything its own

[agents.claude4]
of = "claude"                      # everything of claude3's ("Share everything with" writes every key)
share = { settings = "claude3", instructions = "claude3", rules = "claude3", skills = "claude3", agents = "claude3", commands = "claude3", output-styles = "claude3", hooks = "claude3", plugins = "claude3", keybindings = "claude3", sessions = "claude3", history = "claude3" }
```

- `of` is one of Templum's agents that [has accounts](#which-agents). An entry with `of` takes only `of`, `name`,
  `show`, `folder`, `share` and `shell` (the shell it opens in, its own and never its agent's;
  [shells.md](shells.md#a-shell-for-each-agent)); `command`, `resume`, `generate` and `models` are its agent's in effect (with the
  person's changes) and refused here ("agents.claude2.command: an account runs its agent's command").
- `share` maps an item's key ([the table](#which-agents)) to who it is shared with: the agent's id (its own folder)
  or another account of the same agent. A missing item is the account's own. An unknown key, a target that is
  neither, an account of another agent, or a chain that comes back to the account (claude3's `skills` from claude4,
  claude4's from claude3) is a config error naming the key, and the last valid config stays.
- `folder`: `~` is the home folder. It is a full path (`/…` or `~/…`; `C:\…` on Windows): a relative one would be each
  block's working folder. It can't be any agent's own folder (another agent's too: a Codex account on `~/.claude`),
  nor the home folder, nor in or around Templum's folder for accounts (`<data>/accounts`, whose folders go with their
  account: "that is Templum's folder for accounts: leave Folder empty"), nor another account's, in it or around it
  (claude3 on `~/w/skills` and claude2 on `~/w`). Folders are compared with their links followed, as far as they
  exist: a link to `~/.claude`, a case variant on macOS and Windows and a `\\?\` path are that folder.
- The id of an account added in Settings is **the name the person types** (`claudinho`), checked as
  [Its name](#its-name) says (card 37's dialog gave `claude2`, `claude3`: the agent's id and the first free number;
  those accounts keep their ids). It never changes; the title does (`name`). One written by hand may be any free id
  (`work`), but not another account's in other case (`Claude2` beside `claude2`: one folder on macOS and Windows).
  A new account written by Settings has no `name`: its title is "Claude Code (claudinho)", which says its agent.
- An account isn't one of Templum's agents: Settings ▸ Agents' **Reset all** leaves it, and its agent's ↺ too.

## Its folder and its items

### Making and changing it

What the account's choices say is applied when the dialog saves it, and again each time it starts (the menu,
`spawn_agent`, a restore, Log in), so a link that went missing is made again:

- **The folder** is made when missing (0700 on Unix). **Templum moves a real file or folder only in the folder it
  made itself** (`<data>/accounts/<id>`), and not when that folder, or Gemini CLI's `.gemini` in it, is a link (to
  `~/.claude`, say): then it is a folder the person chose. In one it only makes and removes its own links, so one
  that already has files (~/Documents, another agent's) is taken as it is and nothing of it is renamed. The
  dialog reads its links when it is chosen: a link to the same item of the agent's own folder or of an account
  becomes that item's choice, so a folder set up by hand keeps its sharing.
- **Shared item, nothing there:** a link to the same item of who it is shared with. That item is a folder Templum
  makes (empty) when missing, so a link never points at nothing; a file that doesn't exist there yet isn't linked, and
  is linked at a later start once the agent made it (its settings file, written at its first change).
- **A link Templum didn't make** (one pointing anywhere but the same item of the agent's own folder or of one of its
  accounts, like a dotfiles folder): left as it is, whatever the choice; the dialog shows "Linked by you to
  ~/dotfiles/claude/skills" in place of its dropdown. Templum only ever replaces or removes the links it would make.
- **Shared item, a link Templum would make there:** pointed at who it is shared with now (only the link is
  replaced).
- **Shared item, a real file or folder there**, in Templum's folder, when the dialog saves: it is kept aside as
  `<item>.own` (then `.own-2`, …) and the link made. At a start, nothing is moved: the agent replaced the link with
  its own copy (some write a settings file by a rename), and a notification says so: "Claude Code 2's settings.json
  isn't shared any more: Claude Code wrote its own. Edit the account to share it again." Saving the dialog then keeps
  that copy aside.
- **Shared item, a real file or folder there, in a folder the person chose:** left as it is, at Save and at a start
  alike, and one notification names them: "Claude Code 2 keeps its own projects: Templum never moves files in a
  folder you chose (~/Documents)."
- **Never a link to itself:** a share whose item already is the source's, through a link (Templum's folder for the
  account made a link to `~/.claude`, or the source's item a link to the account's), is left, and the problem
  notification says so: "Skills: it is /home/me/.claude's own through a link, so it isn't linked to itself".
- **Own item, a link Templum would make there:** the link is removed (never what it points at), and in Templum's
  folder `<item>.own` comes back when it is there. Otherwise the item stays missing and the agent starts it fresh.
- **Own item, a real one there:** left as it is.
- **The login** ([the table](#which-agents)) is never made, linked, moved, read or deleted; an account's folder
  starts without it, and the agent's login writes it there.
- **Chains** follow: a link points at the other account's item, not at what that one links to, so claude4's `skills`
  is claude3's whatever claude3 decides later. The accounts an account shares from get their choices first (as at
  their start), so opening claude4 before claude3 never makes claude3's `skills` an empty folder of its own.

A problem (a folder that can't be made, a link the OS refuses) is one notification naming the item and the reason, and
the account still opens with what could be done.

### The links on each OS

- **Linux and macOS:** symbolic links, for files and folders.
- **Windows:** a folder is shared by a directory junction, which needs no administrator and no Developer Mode. A file
  is shared by a symbolic link, which Windows allows only with Developer Mode on (or as administrator); without it
  the item stays its own, and the dialog says so beside it: "Windows shares a file only with Developer Mode on
  (Settings ▸ System ▸ For developers). Folders share without it." A hard link is never used: an agent that writes
  its file by a rename would silently stop sharing it.

### Deleting an account

**Delete this account** in its dialog asks first:

- Refused while a block of a running workspace is the account's, its agent running in it or only its shell ("Claude
  Code 2 has 2 blocks: close them first"): typing its agent in one would make its folder again, with a login no row
  manages. Refused too while another account shares one of its items ("Claude Code 4 shares Claude Code 2's skills:
  change it first"). Refused too while another account's folder is in its own, as a config.toml written before
  Templum refused that can have ("Claude Code 3's folder is in Claude Code 2's: change it first").
- A block of it that comes back later (Ctrl+Z, a stopped workspace started, a restore) is a plain terminal: an
  `"account"` Templum doesn't know gives no variable and no account, as in an older Templum.
- **Its folder in Templum's data folder** is deleted with it, its links removed first (what they point at stays; a
  folder that is itself a link, or whose `.gemini` is: only that link): "Its
  folder is deleted: its login and everything it doesn't share. What it shares stays where it is." For an agent whose
  login is in the system's keychain (Claude Code on macOS, Copilot CLI everywhere), the question adds "Its login stays
  in the system's keychain: run /logout in it first to remove it."
- **A folder the person chose** is kept: "Its folder ~/.claude-work stays."
- **Its command** is deleted when it is Templum's own ([Its command](#its-command)): "Its command claudinho is
  removed."
- Its entry leaves `[agents]`; a card for it keeps the id (the person changes it), as a card for a closed block does.

### Changing its folder

Since card 65 the dialog has no folder field, and its Save keeps `folder` as config.toml has it: a folder changes
only in config.toml, by hand. The account's next start applies its choices in the new folder, and its command is
written again with it. **The old folder stays**, whether Templum made it or the person chose it: it holds a login,
which Templum never deletes on a change; log in again (on macOS even for the same files moved: the Keychain names a
login by the folder's path). `save_account` still refuses another folder while deleting would be refused (its blocks
open, another account sharing from it), with the same words ending "change its folder", for any caller that passes
one.

## Its name and its command

Kanban card 65.

### Its name

The name is the account's id. The dialog checks it at each key, and `save_account` again; the first rule broken is
said under the field:

1. **A word a shell takes as a command:** a lowercase letter, then lowercase letters, digits, `-` and `_`, at most 32
   in all: "Use lowercase letters, digits, - and _, starting with a letter". Lowercase because macOS and Windows don't
   tell `Claudinho` from `claudinho`, so `Claude` would be the file `claude`.
2. **Not one of Templum's agents** (any id `Agent::from_name` knows) **nor `templum`**: "claude is one of Templum's
   agents", "templum is Templum's own command".
3. **Not an id of `[agents]`**, in any case (config.toml's, with what Settings hasn't applied yet): "claudinho is
   already an agent or an account in Templum".
4. **Not a word of a shell:** a fixed list of the builtins and keywords of bash, zsh, fish and PowerShell (`cd`,
   `exit`, `source`, `test`, `time`, `function`, `end`, `begin`, `set`, …): "cd is a word of your shell". A function
   of that name in Templum's terminals would break the shell; a file would shadow the builtin in scripts.
5. **Not a command on this computer:** a program of that name where Templum looks for agents (the PATH the person's
   shell gets, Windows' registry Path, and the folders agents install into, `locate`), other than its own command:
   "claudinho is already a command: /usr/bin/claudinho". On Windows `.exe`, `.cmd`, `.bat` and `.ps1` count, as for
   agents; the other extensions of PATHEXT (`.com`, `.vbs`, …) are not looked for.
6. **Not a file of the commands folder** other than its own command (`claudinho`, and on Windows `claudinho.*`), on
   PATH or not: the same words with its path.
7. **Not a folder Templum still keeps** (`<data>/accounts/claudinho`, left by a change of folder in config.toml): the
   account would open on that login, and deleting it would delete it: "Templum still keeps a folder for claudinho
   (~/.local/share/templum/accounts/claudinho): choose another name".

An account of config.toml (one made before card 65, or by hand) whose id breaks rule 1, 2, 4, 5 or 6 keeps working
from the menu and `spawn_agent`, and gets no command; its row says why ("No command: test is a word of your shell").

### Its command

One file, in the commands folder, reaches every shell and program of the person: bash, zsh, fish, Nushell, PowerShell,
cmd, a script, another terminal app. No list of shells to choose from, and no startup file edited.

**Linux and macOS:** `~/.local/bin/claudinho`, mode 0755:

```sh
#!/bin/sh
# templum-account claudinho /home/me/.local/share/templum
# Made by Templum: opens Claude Code on its account claudinho (Settings ▸ Agents ▸ Accounts).
# Deleted with the account; Templum writes it again when it changes.
export CLAUDE_CONFIG_DIR='/home/me/.local/share/templum/accounts/claudinho'
exec claude "$@"
```

**Windows:** in `%USERPROFILE%\.local\bin`, `claudinho.cmd` for cmd and PowerShell, and `claudinho`, the script above
with the Windows folder, for Git Bash (as npm writes its commands):

```bat
@echo off
rem templum-account claudinho C:\Users\me\AppData\Roaming\templum
rem Made by Templum: opens Claude Code on its account claudinho (Settings > Agents > Accounts).
rem Deleted with the account; Templum writes it again when it changes.
setlocal
set "CLAUDE_CONFIG_DIR=%USERPROFILE%\AppData\Roaming\templum\accounts\claudinho"
claude %*
exit /b %errorlevel%
```

- **What it runs:** its agent's program (the first word of the agent's command in effect, `AgentEntry::program`),
  found on PATH when it runs, with the person's words, and the account's variable from [the table](#which-agents)
  naming its folder (`AgentEntry::var`: Gemini CLI's names the folder that holds `.gemini/`). Not Templum's flags: it
  is `claude` typed by hand on another login. In Templum's terminals a function adds Templum's
  ([Typed in a terminal](#typed-in-a-terminal)). The folder is written in single quotes, a `'` as `'\''`; in the
  `.cmd`, a `%` as `%%` and the home folder as `%USERPROFILE%`, since cmd reads a batch file in the console's code
  page, not in UTF-8.
- **Templum's own:** a file of the commands folder is Templum's command for an account when its second line is the
  marker `templum-account <id> <data>` (after `# ` or `rem `), naming that id and this Templum's data folder: an
  installed Templum and a `--dev` one, or two data folders, keep theirs apart, and each sees the other's as a command
  that is taken. Templum writes, rewrites and deletes only its own; any other file is never touched.
- **When it is written:** at **Create** in the dialog; and each time the agents in effect are computed from
  config.toml (Templum's start, config.toml changed): an account's missing command is made when its id passes
  [Its name](#its-name) (rules 1, 2, 4, 5, 6), and one of Templum's own is written again when its text differs (the
  agent's program or the account's folder changed). A file is written to `<name>.templum-new` in the same folder,
  then renamed over the old one, so no shell ever runs half of one. The commands folder is made when missing.
- **Deleted** with its account (Delete account), when it is Templum's own; on Windows both files. Nothing else
  removes one: an account taken out of config.toml by hand keeps it ([Known limits](#known-limits)).
- **A failure** (a read-only folder): one notification, "Couldn't make the command claudinho: <the OS's reason>";
  the account is saved anyway and opens from the menu.
- **Not on PATH:** when the commands folder isn't among the folders of the PATH the person's shell gets (on Linux and
  macOS the shell's, read at start as `read_shell_path` does, else Templum's own; on Windows the registry's Path),
  the dialog and the Accounts heading say so once, with what to add, and Templum's terminals still have the command
  ([Typed in a terminal](#typed-in-a-terminal)):
  - Linux, macOS: "Add this to your shell's startup file to use claudinho outside Templum:" over the selectable line
    `export PATH="$HOME/.local/bin:$PATH"` (fish's own `export` takes it too).
  - Windows: "Add %USERPROFILE%\.local\bin to your Path to use claudinho outside Templum."

## Where an account shows up

### Opening it

- **AI agent ▸** lists it after Templum's agents and the person's own (accounts by id), by its name, installed when
  its agent is. The folder dialog is the same ("Open Claude Code 2").
- **`spawn_agent`** takes its id (`agent: "claude2"`), its agent's `model` and `effort`, and a worktree, as for its
  agent. The tool's `agent` description adds "or an account's name (Settings ▸ Agents ▸ Accounts)" (it said "like
  claude2" before card 65).
- **Opened** (the menu, `spawn_agent`, Log in, a restore), its block's shell gets the account's variable in its
  environment (`CLAUDE_CONFIG_DIR=<its folder>`), with what its agent gets ([connections.md](connections.md)), and its
  agent's command is typed as for its agent: Templum's flags (`--settings`, `--mcp-config`, Codex's `-c`…) don't
  depend on the folder. The variable is the shell's, not one command's: the agent typed again in that block, after
  it quits, is the same account.
- **The person's own variable:** a shell whose startup files set the same variable again (an `export
  CLAUDE_CONFIG_DIR=…` in `.bashrc`) runs the agent in that folder instead; the snapshot reads the folder the agent
  runs on, so the block is that folder's account, or none ([Typed in a terminal](#typed-in-a-terminal)).

### Its block

- A terminal Templum opened for an account **is that account's** (`Pane::account`), saved with the block
  (`"account": "claude2"`), kept by a restart, Ctrl+Z and a move to another workspace, even while no agent runs in it.
- **Its agent:** when the 5-second snapshot finds the account's agent running in such a block, the block's agent is
  the account (`claude2`): its name is the block's title for the person and in `list_blocks` ("Claude Code 2"), its
  usage chip is that session's, Shift+Enter and the question before closing are its agent's. Another agent typed there
  is that agent, on its own folder.
- **Restore:** the block's shell starts with the account's variable, also when its folder is gone and nothing is
  resumed; its session comes back as its agent's does (`claude --resume <id>`, read from the account's folder:
  `sessions/<pid>.json`, the rollouts of its `sessions/`).
- An older Templum ignores `account`, and doesn't know an agent `claude2`: it resumes nothing there.

### Usage and limits

Plan limits are an account's ([usage.md](usage.md)), so each account is its own row of the meter and the popover,
named by its name, and its own **in the title bar** switch in Settings ▸ Usage (`usage.hide` takes its id):

- **Claude Code:** the status-line relay, which Claude Code runs with its environment, writes the folder its
  `CLAUDE_CONFIG_DIR` names into the file it writes (`"templum_config_dir"`); an account's limits come from the
  newest relay file of its folder. The `/usage` probe runs for the agent's own folder and for each Claude Code
  account with a login (its `.claude.json`), whether it ran this run or not, with its variable, as
  [usage.md](usage.md#claude-code) says (when it asks).
- **Codex:** an account's limits come from the rollouts of its own folder's `sessions/`. When several of a Codex's
  folders share one `sessions` (codex2's linked to codex's), its newest rollouts may be either one's: then each one's
  limits come only from the rollouts its own running sessions write (each process's file, as its usage chip finds
  it), and stay as last read while none runs.
- A session's usage chip reads the account's files (Claude Code's transcript fallback, Codex's rollout).
- Notifications name the account: "Claude Code 2: 90% of the 5-hour limit used".
- **Matched by folder:** a session whose relay names an account's folder counts for that account, whatever block it
  runs in (a person's own `CLAUDE_CONFIG_DIR=… claude` in a plain terminal, when that folder is an account's); a
  folder no account has counts for the agent's own (a shell's startup files may name one of the person's).

### Cards for it

[kanban.md](kanban.md#who-a-card-is-for): **Any session of ▸** lists the accounts with the agents. A card for an
account (`agent:claude2`) is taken only by a block whose agent is that account. A card for its agent
(`agent:claude`, "Any Claude Code") is taken by any of the agent's blocks, its accounts' included: they run its program.

### Typed in a terminal

- `claudinho` typed in any terminal of the computer, Templum's or not, in any shell, runs [its command](#its-command):
  its agent on its folder.
- `claude` typed in a plain terminal is the agent's own folder, as before (cards 42 and 44 unchanged).
- `claude` typed in an account's block is that account: the shell has its variable.
- **In Templum's terminals** `claudinho` also gets what a typed `claude` gets, Templum's server and hooks
  ([connections.md](connections.md#agents-typed-in-a-terminal)), and works even with the commands folder off PATH:
  - Each new terminal gets `TEMPLUM_ACCOUNTS`: a line per account whose id passes [Its name](#its-name)'s rule 1,
    `<id>`, `<program>`, `<variable>` and `<folder>` separated by tabs, raw (not quoted for a shell). Always set,
    empty with no account, so a Templum started from another's terminal doesn't pass that one's on (as
    `TEMPLUM_CLAUDE_ARGS`). An account whose folder holds a tab or a line break is left out of it. Set whatever
    `[connections] enabled` says: the switch reaches the words `claude` adds, not the account's folder.
  - The [shell integration](shell-integration.md) (bash, zsh, fish, PowerShell) defines, when the shell starts, a
    function per line, named by its id, unless the person has a function of that name (as for `claude`): it sets the
    variable to the folder for that one command and runs the program by its name, so Templum's own `claude`
    function, when the shell has one, puts Templum's words first; the person's words follow. The exit status is the
    agent's. The variable isn't left in the shell afterwards (PowerShell sets it and puts the old value back).
  - cmd, Nushell, `shell.integration` off, a shell started from the first one: no function; the command on PATH runs,
    without Templum's server.
- **The block is the account's** whatever started it: the 5-second snapshot reads, for the agent it finds in a block
  and only for an agent with accounts, that agent's variable from its process's environment (sysinfo's `environ`, on
  Linux, macOS and Windows; one more read of that one process, never of every process). Then:
  - the variable names an account's folder (compared as `Account::folder` is written, components normalized): the
    block becomes that account's (`Pane::account`), as if the menu had opened it: its title, its usage chip, cards
    for it, `list_blocks`, its resume after a restart with the variable in its shell, and Delete waits for it;
  - it names another folder or is unset: the block is no account's (the agent's own, or a folder of the person's),
    unless the block is the account of another agent (`codex` run in a `claude2` block is on Codex's own folder, and
    the block keeps `claude2`: its shell still has the variable);
  - the environment can't be read (it comes back empty: another user's process, an elevated one on Windows): the
    block keeps what it had.

  So `CLAUDE_CONFIG_DIR=~/x claude` typed by hand is the account whose folder `~/x` is, and a `.bashrc` that sets the
  variable again in an account's block makes the block the folder's it names. A block where `claudinho` ran stays the
  account's after the agent quits, until another agent is found there: a restart gives its shell the variable, as for
  a block the menu opened.

### Templum's server and hooks

Unchanged per account: they come from what is typed, not from the folder. What does live in the folder:

- **Codex** keeps its trust of Templum's hooks in its `config.toml`: an account with its own asks once more ("Hooks
  need review"); one that shares `config` with a trusted folder is trusted.
- **Gemini CLI** keeps Templum's extension in `extensions/`: an account that shares `extensions` with a folder that
  has it has it. One with its own: the notice of
  [Gemini CLI without its extension](connections.md#gemini-cli-without-its-extension) checks that account's folder,
  and its Install links the extension there (the same `gemini extensions link`, run with the account's variable).
- **Claude Code** keeps the person's user-scope MCP servers in `.claude.json`, with the login: an account doesn't
  share them; the dialog doesn't say it, to stay short.

### Commit messages

An account is one of the agents **Generate ✨** can use ([git.md](git.md)): its agent's generate command and models,
run with the account's variable, so the message is written on that account's plan.

## Settings ▸ Agents ▸ Accounts

Under the agents' rows and **Add an agent**, a heading **Accounts** with the line "Another login of an agent, opened
by its own command." Accounts aren't among the agents' rows above it (nor in the welcome's Agents page). Under it,
once, the [not on PATH](#its-command) note when it applies and an account has a command.

- **A row** per account, by id: its title; "Claude Code · type claudinho in any terminal", or "Claude Code · no
  command: <why>" ([Its name](#its-name)); a switch (the menu lists it); **Log in**; **Edit**.
- **Add an account** opens the dialog. No row and the button alone when there is none.

### The dialog

In the language of the Git popup and the kanban card dialog ([kanban.md](kanban.md), its card dialog): a block's own
color (`ui.surface`) under hairlines, rounded 8, the theme's accent for what is lit, **480 wide**, built with gpui-kit's
dialog as the card dialog is (its header, body and footer; its OK and Cancel buttons not used). Sizes in pixels; every
color from the theme. Esc and the ✕ close it without saving.

**Add an account:**

- **Header** (20 at the sides, a hairline under it): "Add an account" (14, semibold), and under it (12, `title_dim`)
  "Another login of an agent, opened by its own command."
- **Body**, a column with a gap of 18:
  1. **Agent** (`account-agent`): the label (12, `ui.title`) and a dropdown of the agents of
     [the table](#which-agents), installed first. Claude Code is chosen when it is installed, else the first
     installed one, else Claude Code.
  2. **Name** (`account-name`): the single-line field of the card dialog's title (15 text, 38 high), placeholder
     "claudinho", focused when the dialog opens; it lowercases nothing by itself. Under it one line (12), updated at
     each key: nothing while empty; the first [rule](#its-name) broken in the theme's red (ANSI 1); else, in
     `title_dim` with the name in the accent: "Type claudinho in any terminal to open Claude Code on this account."
  3. **Shell** (`menu-account-shell`): the label and a dropdown of **Default (*name*)**, which follows Settings ▸
     Shell, and every shell found except WSL, where an account would lose its folder
     ([shells.md](shells.md#what-works-in-wsl)). A label that is none of them is last, as "*label* (not
     available)", so Save keeps it.
  4. **Setup** (an agent with items; Crush has none): the label, then one strip of three segments in the look of the
     card dialog's Priority strip (`account-setup-same`, `account-setup-fresh`, `account-setup-custom`):
     **Same as Claude Code** (chosen first), **Fresh**, **Custom**; under it one line (12, `title_dim`):
     - Same: "Everything but the login is your Claude Code's (~/.claude): settings, skills, plugins, sessions…". Every
       item is shared with the agent's own folder, except, on Windows without Developer Mode, the files
       (`files_share`): there the line adds "Files stay its own: Windows shares them only with Developer Mode."
     - Fresh: "It starts empty: nothing shared with your Claude Code." No item shared.
     - Custom: "Choose item by item." Under it, in a faint panel (`ui.card`, rounded 8, scrolling past 260), card
       37's controls unchanged: **Share everything with** (`account-share-all`) and a row per item, Own or shared,
       with "Linked by you to …", the Developer Mode words and "Shared once Claude Code writes one" as before. Those
       words go under the item's paths, in the room of the row: an item that has them has no list. Custom opens on
       the choices Same or Fresh last made.
  5. **Notes** (11, `title_dim`), only when one applies: the [not on PATH](#its-command) note; a variable that wins
     over every account's login (as before). Nothing is shown otherwise: the dialog stays short.
- **Footer** (a hairline over it, 20 at the sides, 12 up and down): at the right **Create claudinho**
  (`account-save`), the card dialog's solid accent button, its label following the name ("Create" while empty);
  while the name is empty or breaks a rule it is faint (`ui.hover`, `title_dim` text) and does nothing. Enter in the
  name field is the same as a click.

**Create** saves the entry (`[agents.claudinho]` with `of` and `share`), applies the folder's choices, writes
[its command](#its-command), closes the dialog and Settings, and opens [Log in](#log-in) for it. The account is in
**AI agent ▸** at once. A problem `save_account` finds (one the dialog couldn't see: a config error) is a notification,
the dialog left open.

**Edit claudinho** (Edit on its row):

- **Header:** the id as a faint accent badge (the card dialog's card number: 12 semibold, `ui.accent` on `ui.accent`
  at 16%, rounded 6) and "Account of Claude Code" (12, `ui.title`).
- **Body:** **Command**: "Type claudinho in any terminal (~/.local/bin/claudinho)" or "No command: <why>";
  **Folder**: its folder as text (home as `~`), Templum's or the one chosen in an earlier Templum, with no field and
  no **Choose…**; **Shell**, the same list; **Setup** opened on its shares (every item it can share shared with the agent: Same; none: Fresh;
  otherwise Custom); the notes.
- **Footer:** at the left **Delete account** (`delete-account`), the card dialog's red ghost button with the trash
  icon ([Deleting an account](#deleting-an-account)); at the right **Save** (`account-save`), the solid accent button.
  Save keeps `name` and `folder` as config.toml has them (a title that is the default one isn't written as `name`).

### Log in

**Log in** opens a terminal block at the view's center, in the home folder, that is the account's (its variable), and
types its agent's login command from [the table](#which-agents) and Enter. The person logs in there, and closes it or
keeps it. **Create** opens it for a new account; the row's **Log in** opens it again any time.

## Calls made for card 37

The person was away; each can be changed.

1. **An account is an `[agents]` entry with `of`,** not a table of its own: it is in every list of agents at once (the
   menu, `spawn_agent`, cards, Generate, usage) with no second list to keep in step.
2. **Its id is `claude2`**, the agent's id and a number, as the card writes it, fixed at creation; its name is free
   (card 65: the id is the name the person types).
3. **The folder is the account.** The variable moves everything an agent keeps, so an account is a folder and links,
   with no copy of the agent's settings in Templum.
4. **Item by item, own or shared with one of the agent's folders**, and "share everything with" filling the rows;
   chains are links to links, so claude4 follows claude3's later choices.
5. **Nothing is ever deleted by a change:** a real item is kept aside as `<item>.own` and comes back when it is own
   again; deleting an account deletes only a folder Templum made, after asking.
6. **Windows shares folders always (junctions) and files only with Developer Mode (symbolic links)**, never hard
   links, which an agent's rename breaks without anyone seeing.
7. **The block is the account's**, through its shell's variable, saved with the block: Templum knows an account's
   session because it set its folder, on every OS, without reading another process's environment.
8. **No `claude2` command in terminals** (replaced by card 65: every account is a command, [Its command](#its-command));
   `claude` in an account's block is it.
9. **"Any Claude Code" includes its accounts; a card for claude2 is claude2's only.**
10. **Limits per account**, the relay naming its folder; the `/usage` probe for every account with a login.
11. **Log in types the agent's own login command** in a block of the account; Templum never touches the login.
12. **The settings live in Settings ▸ Agents**, under the agents, rather than a section of their own: an account is an
    agent there too.
13. **Templum only touches the links it would make** (to the same item of one of the agent's folders); a person's own
    links are left and shown, and a folder set up by hand is read into the dialog's choices.
14. **Deleting an account, or changing its folder, waits for its blocks to close**, its shells included; a block of it
    that comes back afterwards is a plain terminal.
15. **A folder change keeps the old folder:** it holds a login, never deleted except by deleting the account.
16. **Limits follow the folder**, not the block, since a login is its folder's.

## Calls made for card 65

The person was away; each can be changed.

1. **The name is the id and the command:** one word for what the person types in a terminal, what `spawn_agent` and
   cards call it, and its folder's name.
2. **A file on PATH, not aliases in startup files:** one file reaches every shell and program at once; startup files
   are often links into a dotfiles repository, differ per shell, and would be four edits to undo. So no shell
   selector: there is nothing to choose.
3. **`~/.local/bin` on every OS** (under the user's profile on Windows): the user's commands folder of the XDG base
   directories and systemd's file hierarchy, where Claude Code's own installer puts `claude` (so it is usually on PATH
   already), pipx and uv theirs. A `.cmd` and a sh script on Windows, as npm does.
4. **Templum's terminals define it as a function too**, from `TEMPLUM_ACCOUNTS`: a typed `claudinho` gets Templum's
   server as a typed `claude` does, through the same function, and works with `~/.local/bin` off PATH.
5. **A block's account follows the folder its agent runs on**, read from its process's environment, so a typed
   `claudinho` is that account's block (title, resume, usage, Delete waiting for it). It also fixes the minor "A shell
   that sets the agent's variable again runs the agent in that folder".
6. **No rename:** delete and add again.
7. **No folder to choose:** a new account is always on Templum's folder; a folder chosen in an earlier Templum stays,
   shown as text, and changes only in config.toml.
8. **Three setups instead of twelve dropdowns:** Same as the agent first (card 37's "claude2 the same as claude but on
   another account"), Fresh, and Custom with card 37's controls unchanged.
9. **Create opens Log in at once:** a new account has no login, and its agent would ask anyway.
10. **The title stays "Claude Code (claudinho)"** when no `name` is set: it says the agent, and holds the name.
11. **Lowercase names only**, for macOS and Windows, whose files ignore case.
12. **The automatic `claude2` id is gone** (`new_account_id`): its "a folder still there" check is rule 7.

**The "claude2 mock" the card asks about.** Checked on 2026-10-20: Templum makes no `claude2` account and keeps none
outside tests; the person's config.toml has no account. `claude2` was the id card 37's dialog proposed
(`new_account_id`), which goes; the example in `spawn_agent`'s `agent` description ("or an account, like claude2",
`crates/templum-mcp/src/tools.rs`), which agents read and repeat, and which now names no account; and the tests'
fixture account (`CLAUDE2` in `crates/templum/src/workspace/tests/agents.rs`, and the configs of `agents.rs`,
`config.rs`, `session.rs` and `persistence.rs` tests), which stays: a config in a test's temporary data folder.

## Considered and left out

- **Aliases or functions in each shell's startup file** (`.bashrc`, `.zshrc`, `config.fish`, the PowerShell profile),
  in a marked block, with a list of shells to choose from: four syntaxes, files often managed by the person's
  dotfiles, nothing for Nushell, cmd, scripts or a program that runs `claudinho`, and four edits to undo.
- **The command in Templum's `<data>/bin`** (already first on PATH in Templum's terminals): no other terminal has it.
- **A `templum account claudinho` launcher** in the command, which would apply the account's links and add Templum's
  words in Rust: the command would break when Templum moves or is uninstalled, and `templum.exe` has no console on
  Windows.
- **Templum's words in the command itself** (it reads `TEMPLUM_CLAUDE_ARGS`): a fifth copy, in sh, of each agent's
  rules for when to add them (Copilot CLI's and Pi's subcommands); the functions reuse the shells' own.
- **A copy instead of a link** ("own, starting from claude's"): a copy of `projects/` can be gigabytes, and the
  person can copy one file by hand.
- **`share = "claude3"` for every item at once:** one more form to parse; the dialog's "Share everything with" writes
  every key, so an item a later Templum adds isn't shared without the person saying so.
- **An account's own command or model** (`claude2 = { command = "claude --model opus" }`): the agent's command is the
  account's; a field of its own when someone asks.
- **Sharing the login** (two folders, one login): that is the agent's own folder.

## Known limits

Each goes to [docs/minors.md](../minors.md) under **Agent accounts** with the code that has it.

- A `.cmd` passes the person's words through cmd: a word with `"`, `%` or `^` may change, as with npm's commands.
- A program of the same name installed later is told only at the next start of Templum (or change of the agents),
  when the account's row says "No command: claudinho is already a command: …". Until then nothing says so; after it
  Templum's own file stays, and the account stays in `TEMPLUM_ACCOUNTS`, so its function still runs in Templum's
  terminals. In any other terminal the order of PATH decides.
- An account taken out of config.toml by hand keeps its command, which still opens its folder.
- A block where `claudinho` ran stays the account's after the agent quits, until another agent is found there.
- A command made while `~/.local/bin` isn't on PATH works in other terminals only once the person adds it.
- An agent that rewrites a shared file by a rename stops sharing it; Templum notices only at the account's next start.
- A card for a deleted account keeps its id until the person changes it.
- Claude Code syncs a claude.ai account's skills and plugins into `skills/synced/` and `plugins/synced/`: two accounts
  sharing `skills` or `plugins` share those too, and each sign-in syncs its own over them.
- Claude Code's Console sign-ins without a key (`~/.config/anthropic`) and Codex's MCP servers' OAuth logins (the
  keyring, by server) are one for every folder.
- Gemini CLI's `advanced.autoConfigureMemory` is read from the account's folder itself, not its `.gemini/` (its issue
  27421).

## Architecture

| File | Change |
|---|---|
| `templum-usage/src/accounts.rs`, `accounts/folder.rs` (no UI; a module of the crate that already knows each agent's folders, not a crate of its own) | Each agent's variable, own folder, items and login; applying an account's choices to its folder, reading a folder's links back, keeping items aside, deleting a folder without following links; junctions on Windows |
| `templum/src/config.rs` | `of`, `folder`, `share` in `[agents.<id>]`, validated (cycles, targets, keys) |
| `templum/src/agents.rs` | Accounts among the agents in effect (`AgentEntry::account`), their variable, their id; left out of detection |
| `templum/src/workspace/blocks.rs`, `usage.rs` | An account's variable in its block's shell, its choices applied at each start, Log in |
| `templum/src/workspace/mod.rs`, `persist.rs`, `session.rs` | `Pane::account`; the snapshot's agent is the account; `"account"` in the canvas file; restore |
| `templum-usage` | A process's account folder (`Proc::home`), the relay's `templum_config_dir`, limits per account |
| `templum/src/workspace/content.rs` | A card for an account |
| `templum/src/settings/accounts.rs` (new) | The Accounts rows and dialog, Log in, Delete |
| `templum-git/src/message.rs` | Generate runs with an account's variable |
| `templum-usage/src/accounts/command.rs` (new, card 65) | Its command's text for each OS, whether a file is Templum's own, writing it (renamed into place, 0755) and removing it; the shells' words |
| `templum/src/agents.rs` (card 65) | The name's rules (`name_problem`), the commands folder, `TEMPLUM_ACCOUNTS`'s lines, a block's account from the folder found; `new_account_id` removed |
| `templum/src/workspace/preferences.rs` (card 65) | Create writes the command, Delete removes it; commands checked when the agents are computed |
| `templum/src/settings/accounts.rs` (card 65) | The new dialog and rows |
| `templum-terminal/src/terminal.rs`, `agent.rs` (card 65) | `Detect::var`; `Inspection` reads the found agent's variable; the PATH folders of the person's shell |
| `templum-terminal/src/integration/*` (card 65) | A function per line of `TEMPLUM_ACCOUNTS` in bash, zsh, fish and PowerShell |
| `templum/src/workspace/mod.rs` (`see`), `usage.rs` (`agent_env`) (card 65) | The block's account from what was found; `TEMPLUM_ACCOUNTS` |

## Testing

**Unit (`templum-usage`'s `accounts`):** no agent's item is or holds one of its login's names; every key is unique per agent; a
shared item becomes a link to the other folder's item, a missing source folder is made, a missing source file isn't
linked; a real item is kept aside on save (`.own`, then `.own-2`) and left at a start (reported), and never moved
in a folder the person chose, nor in Templum's folder (or its `.gemini`) that is a link elsewhere, which deleting
removes alone; a folder that is a link to its source isn't linked to itself; own again removes
only the link and brings `.own` back; a link the person made elsewhere is never replaced nor removed, and a folder
set up by hand is read back as choices; a chain links to the middle account's item; the login file is never touched
by any of these; deleting removes the folder's links before the folder and leaves their targets; Windows: a folder is a
junction, a file without the privilege is refused with the copy above.

**Unit (`templum`):** `[agents.claude2] of = "claude"` parses; `command` on an account, an unknown item, a target of
another agent, a cycle, a taken folder, one in or around another's, any agent's own folder, ids equal but for case
are errors; an account inherits its agent's command and resume, with the person's changes; ids `claude2`, `claude3`
(gone with card 65's typed name);
accounts aren't detected as agents; its variable; a card for `claude2` is taken by a claude2 block only, one for
`claude` by both.

**Unit (`templum-usage`):** the relay writes `templum_config_dir` from its environment; limits per folder; Codex limits from a
process's own rollout when sessions are shared; a process's session id read from its account's folder.

**Headless UI:** the menu opens an account with its variable in the shell's environment and its name as the cover's
title; `spawn_agent` takes `claude2`; a snapshot that finds Claude Code in an account's block names the account; the
canvas file keeps `account` and a restore gives the shell the variable; Settings adds an account (an `[agents]` entry
and a folder with its links), keeps a chosen folder's own files where they are, edits a share, refuses a cycle, logs
in (a block of the account, its login typed) and deletes one (the folder gone, the shared source left); deleting or
moving an account with a block of it open (its shell alone too) is refused, and a block of a deleted account comes
back plain.

**Card 65.** Unit (`templum-usage`'s `command`): the sh and `.cmd` texts for a folder with a space, a `'` and a `%`, and the `.cmd`'s home folder as `%USERPROFILE%`;
written 0755 in a temporary folder through a renamed file, and Templum's own; a file without the marker, or with
another data folder's, never rewritten nor removed; written again only when its text differs; removing one leaves the
others. Unit (`templum`): each rule of [Its name](#its-name) with its words (a program in a temporary PATH folder, a
file in a temporary commands folder, a folder left in a temporary data folder); `AccountDraft::of` leaves a default
title out of `name`; `TEMPLUM_ACCOUNTS`'s lines (a folder with a tab left out); the block's account from the folder
found (an account's, another, none, unread). Unit (`templum-terminal`, Unix): a fake `claude` started with
`CLAUDE_CONFIG_DIR` set is found with that folder. Shell integration (each shell when installed): `claudinho x` runs
the fake `claude` with Templum's words, then `x`, and the folder in its variable; the shell doesn't keep the variable;
the person's own `claudinho` function wins. Headless UI: Create with a name writes the entry, the folder's links and
the command in a temporary commands folder, and opens Log in; a broken rule keeps Create faint with its words; Same
shares every item, Fresh none; Edit opens on the setup; Delete removes the command; a typed `claudinho` found in a
plain block makes it the account's; an account written by an older Templum (`folder`, `name = "Claude Code 2"`) is
edited and saved with both kept. No test reads or writes the person's `~/.local/bin`, PATH or agents' folders.

**Manual (card 65, Hyprland):** create `claudinho`, log in from the block Create opens; `claudinho` in a new Templum
terminal (its block titled "Claude Code (claudinho)", its status dot moving), in another terminal app, and in fish;
delete it and `claudinho` is gone. Not checkable here: macOS, Windows' cmd, PowerShell and Git Bash.

**Manual (Hyprland, and macOS for the Keychain):** two Claude Code accounts logged in, each with its own limits;
claude2 sharing claude's sessions resumes one of claude's with `/resume`; codex2 with its own login; a Windows
account sharing `skills` by a junction.

## Risks

- **An older Templum and an account entry:** `[agents]` refuses unknown keys, so an older Templum reports
  `agents.claude2.of` as a config error and keeps its last valid config (at its start: the defaults). Downgrading
  after adding an account needs the entry removed.
- **Agents change their folders:** an item renamed by a new version of an agent is a link to nothing that the agent
  ignores; [the table](#which-agents) is checked again when an agent's version changes what it keeps.
- **Sessions of two accounts in one folder:** an agent's own resume list shows both accounts' sessions; resuming the
  other account's session works where the agent keeps them by id (Claude Code's `projects/`).

## Sources

Read on 2026-10-18. Write-through checks were made in a temporary folder named by each variable, never the person's.

- **Claude Code:** [authentication](https://code.claude.com/docs/en/authentication) (several accounts, the Keychain
  per `CLAUDE_CONFIG_DIR`), [env-vars](https://code.claude.com/docs/en/env-vars),
  [claude-directory](https://code.claude.com/docs/en/claude-directory), [skills](https://code.claude.com/docs/en/skills)
  (synced skills); issues [92252](https://github.com/anthropics/claude-code/issues/92252) (the variable set to
  `~/.claude` names another entry), [79223](https://github.com/anthropics/claude-code/issues/79223),
  [87447](https://github.com/anthropics/claude-code/issues/87447); `claude --help`, `claude auth --help` (2.1.286).
- **Codex:** [config-advanced](https://learn.chatgpt.com/docs/config-file/config-advanced),
  [config-reference](https://learn.chatgpt.com/docs/config-file/config-reference); source:
  [types.rs](https://github.com/openai/codex/blob/main/codex-rs/config/src/types.rs) (`file` is the default store),
  [storage.rs](https://github.com/openai/codex/blob/main/codex-rs/login/src/auth/storage.rs) (the keyring by home),
  [edit.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/config/edit.rs) (writes through a link),
  [oauth.rs](https://github.com/openai/codex/blob/main/codex-rs/rmcp-client/src/oauth.rs); issues
  [46269](https://github.com/openai/codex/issues/46269), [42447](https://github.com/openai/codex/issues/42447),
  [35555](https://github.com/openai/codex/issues/35555), [4432](https://github.com/openai/codex/issues/4432).
- **Gemini CLI:** [configuration.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md),
  [enterprise.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/enterprise.md),
  [paths.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/paths.ts),
  [commentJson.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/utils/commentJson.ts)
  (writes by a rename),
  [oauth-credential-storage.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/code_assist/oauth-credential-storage.ts),
  [apiKeyCredentialStorage.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/apiKeyCredentialStorage.ts);
  issues [23622](https://github.com/google-gemini/gemini-cli/issues/23622),
  [27421](https://github.com/google-gemini/gemini-cli/issues/27421).
- **GitHub Copilot CLI:** [config dir reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference),
  [authenticate](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli);
  `copilot help environment` (1.0.98).
- **Pi:** [config.ts](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/config.ts)
  (`getAgentDir`, `getAuthPath`); `pi --help` (0.85.1).
- **Grok Build:** its bundled user guide (`~/.grok/docs/user-guide/`: configuration, authentication, sandbox) and
  binary (1.0.50).
- **Kimi Code:** [data-locations.md](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/configuration/data-locations.md).
- **Crush:** [load.go](https://github.com/charmbracelet/crush/blob/main/internal/config/load.go),
  [login.go](https://github.com/charmbracelet/crush/blob/main/internal/cmd/login.go); `crush dirs` (0.98.1).
- **Left out:** OpenCode [global.ts](https://github.com/anomalyco/opencode/blob/dev/packages/util/src/global.ts),
  Kilo Code [global.ts](https://github.com/Kilo-Org/kilocode/blob/main/packages/core/src/global.ts), Cursor CLI
  [configuration](https://cursor.com/docs/cli/reference/configuration), Mistral Vibe
  [keyring.py](https://github.com/mistralai/mistral-vibe/blob/main/vibe/utils/keyring.py), Amp
  [settings](https://ampcode.com/docs/cli/settings), Auggie [reference](https://docs.augmentcode.com/cli/reference),
  Qwen Code [storage.ts](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/config/storage.ts), Kiro CLI
  [issue 10417](https://github.com/kirodotdev/Kiro/issues/10417), Cline
  [paths.ts](https://github.com/cline/cline/blob/main/sdk/packages/shared/src/storage/paths.ts), Goose
  [paths.rs](https://github.com/block/goose/blob/main/crates/goose/src/config/paths.rs).
- **Windows links:** [hard links and junctions](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions),
  [CreateSymbolicLinkW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinkw),
  [RemoveDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-removedirectoryw);
  [`junction`](https://crates.io/crates/junction) 2.1 (MIT); Rust's
  [`remove_dir_all`](https://doc.rust-lang.org/std/fs/fn.remove_dir_all.html) never follows a link
  (CVE-2022-21658's fix).
