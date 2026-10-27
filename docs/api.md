# The Templum API

Your Templum, from anywhere: a phone, a dashboard, another computer. It is an HTTP API inside Templum, off until you
turn it on, that answers only to **tokens** you make. Every tool the Templum Agent has is `POST /v1/tools/{name}`; the
things a dashboard lists are JSON resources; everything that changes comes on one live event stream. Design:
[specs/api.md](specs/api.md).

> **Read this before you make a token.** A **Control** token can make your agents act, and an agent in bypass mode runs
> what it is told. A **Full** token can type into terminals: anyone who holds one can run commands on this computer.
> Keep tokens out of agents' reach (never in a terminal's environment, a prompt or a file an agent reads), give a
> dashboard a **Read** token, and prefer short expiries.

*How these examples were checked:* every route, header, status and event below is exercised by the integration tests in
`crates/templum-api/tests/` (plain and over TLS) and the workspace tests in
`crates/templum/src/workspace/tests/api.rs`, which run the server with a real HTTP client. The `curl` and JavaScript
lines were not run against a running window of the dev build (the author's environment had no display), so try one
first.

## Quickstart

1. **Turn it on:** Settings ▸ API ▸ *Turn on the API*. It listens on `http://127.0.0.1:7411` (this computer only).
2. **Make a token:** *New token…*, a name, what it **Can** do, when it **Expires**, which **Workspaces**. The token is
   shown once; Templum keeps only a fingerprint of it.
3. **Call it:**

   ```sh
   export TOKEN=tpl_…
   curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:7411/v1/state
   ```

### From outside this computer

Templum opens a port; the way in is yours. The server refuses plain HTTP on any address but this computer's: off it,
HTTPS is on (Templum's own certificate, or yours).

- **cloudflared** (HTTPS is the tunnel's; Templum stays on `127.0.0.1`):

  ```sh
  cloudflared tunnel --url http://127.0.0.1:7411          # a quick try
  ```

  For good, a named tunnel's config:

  ```yaml
  ingress:
    - hostname: templum.example.com
      service: http://127.0.0.1:7411
    - service: http_status:404
  ```

  Put **Cloudflare Access** (or a VPN) in front of it as a second lock. Behind a tunnel every connection comes from this
  computer; Templum reads the client's address from the last `X-Forwarded-For` entry (the one the proxy added; no other
  header is believed), so failed logins from the internet are counted per client, not against you. A good token passes
  whatever the address's record. Behind a plain TCP forwarder (`ssh -R`, `socat`, `ngrok tcp`) nothing appends
  `X-Forwarded-For`, so the address Templum shows is whatever the client sends; only an HTTP proxy that appends the
  header tells clients apart.
- **Tailscale, a VPN, a reverse proxy:** the same: point it at the port.
- **The LAN:** *Who can reach it* ▸ every network or one address; HTTPS switches to Templum's certificate. Its SHA-256
  fingerprint is in Settings (*Copy*); *Save certificate…* writes a `.pem` to install on a phone. *New certificate*
  makes another (clients that pinned the old one stop connecting).

## Examples

```sh
B=http://127.0.0.1:7411; H="Authorization: Bearer $TOKEN"; J='Content-Type: application/json'

curl -H "$H" $B/v1/agents                                              # the agents and their states
curl -H "$H" -H "$J" -d '{"agent":"claude","workspace":"w3","prompt":"Take card 40"}' $B/v1/agents
curl -H "$H" -H "$J" -d '{"task":"Review the open PRs"}' $B/v1/agents/b7k2m/task
curl -H "$H" -H "$J" -d '{"answers":[["Postgres"]]}' $B/v1/requests/r12/answer
curl -H "$H" -H "$J" -d '{"allow":true}' $B/v1/requests/a3/answer         # an approval: needs a Full token
curl -H "$H" -H "$J" -d '{"column":"Done"}' $B/v1/cards/40/move
curl -H "$H" -H "$J" -X PATCH -d '{"x":120,"y":80,"w":640,"h":400}' $B/v1/blocks/b7k2m
curl -H "$H" -H "$J" -X POST -d '{}' $B/v1/automations/1/run
curl -H "$H" -H "$J" -d '{"board":"Sprint","title":"Login"}' $B/v1/tools/create_card   # any tool, by name
curl -N -H "$H" $B/v1/terminals/b7k2m/stream                            # a terminal's screen, live
curl -H "$H" -H "$J" -d '{"keys":["l","s","enter"]}' $B/v1/terminals/b7k2m/input   # needs a Full token
curl -N -H "$H" "$B/v1/events?view=true"                                # also the canvas's camera
```

A POST, PUT or PATCH with a body must say `Content-Type: application/json`; one with no body needs no type.

### Events from JavaScript

A browser's `EventSource` can't set headers, so it opens with a **ticket**: a key for one stream, 30 seconds, once.

```js
const base = 'https://templum.example.com';
const headers = { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
const state = {};                                   // collection → id → object

async function connect() {
  const { ticket } = await (await fetch(`${base}/v1/tickets`, { method: 'POST', headers, body: '{}' })).json();
  const source = new EventSource(`${base}/v1/events?ticket=${ticket}`);
  source.addEventListener('snapshot', e => {        // the whole state: replace what you had
    const s = JSON.parse(e.data);
    for (const [name, list] of Object.entries(s))
      if (Array.isArray(list)) state[name] = Object.fromEntries(list.map(o => [o.id, o]));
  });
  for (const kind of ['workspace', 'block', 'wire', 'request', 'board', 'card', 'automation', 'usage']) {
    source.addEventListener(kind, e => {
      const { op, id, value } = JSON.parse(e.data);
      const table = (state[kind + 's'] ??= {});
      op === 'put' ? (table[id] = value) : delete table[id];
    });
  }
  source.addEventListener('notice', e => console.log('notice', JSON.parse(e.data)));
  // EventSource's own retry would reuse the used ticket: close and open again with a new one.
  source.onerror = () => { source.close(); setTimeout(connect, 2000); };
}
connect();
```

Node can send the header itself, and pin Templum's certificate with the fingerprint Settings shows:

```js
import https from 'node:https';
const pin = 'AB:CD:…';                              // Settings ▸ API ▸ Certificate
https.get('https://192.168.1.5:7411/v1/events', {
  headers: { Authorization: `Bearer ${token}` },
  checkServerIdentity: (host, cert) =>
    cert.fingerprint256 === pin ? undefined : new Error('Templum\'s certificate changed'),
}, res => res.on('data', chunk => process.stdout.write(chunk)));
```

## Reference

### Authentication

`Authorization: Bearer tpl_…` is the only way in: no cookie, no basic auth, no token in a URL. A token is `tpl_` and 43
characters (256 random bits); Templum keeps its SHA-256 and compares in constant time. A wrong, expired or revoked
token, no token, a used or stale ticket: `401` with `WWW-Authenticate: Bearer` and the same body whatever the reason.
Failed authentications (a token or ticket was shown and wasn't good; asking with none is not one) are counted per client
address (an IPv6 /64 is one address): at the tenth within 10 minutes the address's failures are answered `429` for 2
seconds, then twice as long at each further failure, up to 15 minutes (`Retry-After`). A good token always passes,
and a failure's connection is closed. The table holds 4096 addresses; past that the rest share one entry. The refusal
answers failures sooner and keeps the log quiet; it does not slow guessing, because a token is checked before the
address's record is looked at. What stops guessing is the token itself: 256 random bits.

Connections: 64 in all, 16 per token; off this computer, 8 that have shown no token per address (the newest makes room
by closing the oldest of that address; the cap is the same for an address the limiter is refusing, since its users open
connections in parallel). Nobody is turned away at the door, whatever the limiter says of their address: a good token
always passes against guessing, also from an address shared with someone who guesses. That does not hold against a
flood of connections: whoever shares the address and opens connections without a token can still starve the
connections of that address that haven't authenticated yet. When the table is full the oldest connection that has
shown no token, of the address holding the most, makes room, and a 65th with all 64 authenticated gets `503`. A
token's 17th connection gets `429`.

**What a token limited to some workspaces can't do:** `overview` (read `GET /v1/state`), `list_cards` or
`create_card` without naming a `board` it sees, open anything in a folder outside its workspaces' folders (`..` and
symlinks followed; a terminal, an editor or an agent in a workspace with no folder; a `worktree` of a repository whose
main tree, as git reports it, lies outside them; on Windows, a folder on a network share or any `\\?\…` path that none of them is on; only a drive letter is the computer's own), or make a command, switch or run one: those are a Full token's whoever it is limited to.
These refusals are all `403` `scope`, a `worktree` outside included. A folder it names is stored as the real path that was checked (`"."` becomes the workspace's folder), and an automation
it saved is checked again at each run: if its folder no longer resolves inside the workspaces' folders (a link was
swapped in), the run is Skipped "Target missing". The folder limits guard what a token can name; they are not a
sandbox: a process the token starts inside a folder (a terminal, an agent) has your reach. Open event streams of a token end when its list of workspaces
changes; the client opens them again.

**Automations follow their token:** an automation saved through the API (`save_automation`, by any token) is turned
off when that token is revoked (Revoke or Revoke all) or expires, and a notification says "Automation <name>, saved by
token '<token name>', was turned off." A rule the token had turned off itself is told of too ("…stays off.") and stays off.
Until the sweep (every few seconds) turns it off, a run of a revoked or expired token's rule is Skipped "Target missing:
its token was revoked". Saving it yourself in the editor (or deleting it) makes it yours: it stays.

A **ticket** (`POST /v1/tickets` → `{"ticket":"tkt_…","expires_in":30}`) opens one `GET /v1/events?ticket=…` or one
`GET /v1/terminals/{block}/stream?ticket=…` within 30 seconds, once, with its token's scope and workspaces. It is
the only key a URL may carry, and only on those two routes. Revoking a token voids its tickets.

### Scopes

| Scope | May |
|---|---|
| **read** | Every GET, the event stream, and the tools that only read (`overview`, `list_agents`, `list_shells`, `recent_folders`, `read_terminal`, `list_cards`, `read_note`, `read_checklist`, `wait_for`) |
| **control** | Also every other tool that doesn't type for you: opening agents, handing and interrupting tasks, blocks, wires, cards, notes, checklists, showing; answering questions, plans and requests to connect; running, switching and saving automations that run no command; Git's acts (`git`: stage, commit, push, branches, stashes…) |
| **full** | Also `send_to_terminal`, `send_keys`, `terminal_input`, `share_workspace`, saving an automation that runs a command, deleting an automation, deleting a note (`delete_note`), answering a request to delete or merge a worktree, answering the Templum Agent's approvals, and Git's discards, Restore version, deleting a branch, dropping a stash and aborting (`git`) |

Too low a scope: `403` `{"error":{"code":"scope","message":"send_to_terminal needs a Full token."}}`. A call through the
API never shows the Templum Agent's approval: the token is your own hand, and its scope says how far it reaches.

**Workspace-limited tokens** see only those workspaces, their blocks and wires, the requests of their blocks, the boards
a kanban block of theirs shows (with those boards' cards), the notes and checklists a note or checklist block of
theirs shows, the drawings of those workspaces and the automations for those workspaces; no usage, no
approvals. A block, workspace, board, card, request or automation outside them is answered as one that doesn't exist.
They can't make a workspace, save automations only for their own workspaces, and get only the recent folders that lie
inside those workspaces' folders (`recent_folders`).

### Resources (read scope)

| Path | Answers |
|---|---|
| `GET /v1/state` | The whole state: `app`, `workspaces`, `blocks`, `wires`, `requests`, `boards`, `cards`, `notes`, `checklists`, `drawings`, `automations`, `schedules`, `shares`, `usage`, `view` |
| `GET /v1/workspaces`, `/{workspace}` | Workspaces in the sidebar's order |
| `GET /v1/blocks`, `/{block}` | Blocks, back to front; `?workspace=w3` filters |
| `GET /v1/agents` | The blocks running an agent, with its state |
| `GET /v1/terminals/{block}/text` | `read_terminal`: `?lines=200&last_command=true` |
| `GET /v1/terminals/{block}/stream` | A terminal's screen, live ([Terminals, live](#terminals-live)) |
| `GET /v1/notes/{block}/text`, `/v1/checklists/{block}` | `read_note`, `read_checklist` |
| `GET /v1/requests`, `/{request}` | What waits for you: questions, plans, requests to connect or delete a worktree, approvals |
| `GET /v1/boards`, `/{board}`, `/v1/cards/{card}` | Boards with their columns' card ids; a card whole |
| `GET /v1/automations`, `/{automation}` | Your automations, with their next and last runs |
| `GET /v1/me` | Who the token is: its name, scope, workspaces, expiry and share, and this computer's name ([Shares](#shares-templum-to-templum)) |
| `GET /v1/shares` | The workspaces shared with another Templum, as the token sees them |
| `GET /v1/usage` | The agents' plan limits |
| `GET /v1/tools` | The registry: every tool's name, description, input schema and scope |
| `GET /v1/openapi.json` | The OpenAPI 3.1 document, generated from the registry |
| `GET /v1/events` | The event stream |

Ids are Templum's own: block ids (`b7k2m`), workspace ids (`w3`), board, card, column, item and comment numbers,
automation numbers; a request is `r<n>` (the inbox's; `n` starts at the Unix time in milliseconds when Templum starts, so
a restarted Templum never reuses a request id a mirror may still hold) or `a<n>` (the Templum Agent's approvals). Times
are Unix seconds.

**Workspaces and the Home.** A workspace has `opened`, when it was last opened (shown) in Templum, in Unix seconds:
null for one not opened since Templum kept it. It is what the Home's cards say ("Opened 3 h ago"); the Home's other
counts are the state's too: the blocks' `agent.state` (`needs_you`, `done`), `requests` and `automations[].last`. The
Home itself is window chrome, like the popups: it isn't in the state and no tool opens it; `show` closes it.

**Agents and schedules.** An agent's block has `agent.task`, the task it works on:
`{"id":"t7","text":"…","by":"b7k2m","since":1761560000}` (`text` at most 2,000 characters; `by` the block that handed
it, `"api"` for the API, null for one the token doesn't see), and `agent.role`, the role it was opened with, or null.
`schedules` holds the deliveries agents asked for (`schedule`):
`{"id":"s3","by":"b7k2m","to":"k9x1d","text":"Check the deploy","next":1761562000,"every":600,"until":1762166800}`
(`every` in seconds, null for once); a workspace-limited token sees those whose `to` block it sees, `by` null when it
doesn't see that one.

### Tools and aliases

`POST /v1/tools/{name}` runs any tool with its arguments as the JSON body; the answer is
`{"text":"Created card 40.","workspace":"w3","block":"b7k2m"}` (the tool's words, and what it touched when known). The
tool refusing is `422` `refused` with its sentence; a malformed argument is `400` `invalid`. `GET /v1/tools` lists them,
with the sixteen only the API has:

| Tool | Arguments | Scope |
|---|---|---|
| `answer_request` | `request`, then one of `answers` (a list per question: the labels picked, and at most one other text; a plan is `[["Approve"]]` or `[["Reject"]]`), `allow` (true/false), `dismiss`; `note` | control; full for a worktree's request and an approval |
| `save_automation` | `automation` (as `automations.json` keeps one: `name`, `workspace`, `when`, `do`, `max_runs`; `on`, default true, for a new one), `id` to replace one | control; full when its `do` runs a command |
| `set_automation` | `automation` (its number), `on` | control |
| `run_automation` | `automation` | control |
| `delete_automation` | `automation` | full |
| `cancel_schedule` | `schedule` (`s3`) | Cancels a delivery an agent asked for later, as the Automations popup's Cancel does | control |
| `terminal_input` | `block`, then `text` (a paste), `keys` (1 to 256, [a key's name](#a-keys-name)) or `mouse` (1 to 64 events) | full |
| `share_workspace` | `workspace`, `name`, `scope` (`control` or `full`), `expires` (Unix seconds, or null), `way`, `address` | full, a token for all workspaces |
| `revoke_share` | `share` (left out: the calling token's own) | full, or the share itself |
| `git` | `workspace`, `folder`, `op` and its arguments ([Git](#git)) | read, control or full, by `op` |
| `recent_folders` | `workspace` | read |
| `edit_note` | `block`, `note` (a note that block shows as a tab), then any of `name`, `color` (`red`, `yellow`, `green`, `cyan`, `blue`, `magenta` or `none`), `text` with `rev` | control |
| `new_note` | `block`, then `name`, `text` | control |
| `open_note` | `note`, then `block`, or `workspace` with `x`, `y` (both or neither) | control |
| `close_note` | `block`, `note` | control |
| `delete_note` | `note` | full |

REST aliases name the common calls; path parameters and the body become the tool's arguments, and the path wins:

| Method and path | Tool |
|---|---|
| `POST /v1/workspaces`, `/v1/blocks`, `/v1/show` | `create_workspace`, `add_block`, `show` |
| `PATCH /v1/blocks/{block}`, `DELETE /v1/blocks/{block}` | `move_block` (`x`, `y`, `w`, `h`), `close_block` |
| `POST /v1/wires`, `DELETE /v1/wires/{a}/{b}` | `connect`, `disconnect` |
| `POST /v1/agents`, `/v1/agents/{block}/task`, `/interrupt`, `/stop` | `open_agent`, `delegate`, `interrupt`, `stop_agent` |
| `POST /v1/terminals/{block}/text`, `/keys` | `send_to_terminal`, `send_keys` |
| `POST /v1/terminals/{block}/input` | `terminal_input` |
| `POST /v1/requests/{request}/answer` | `answer_request` |
| `POST /v1/boards/{board}/cards`, `PATCH /v1/cards/{card}`, `POST /v1/cards/{card}/move`, `/comments` | `create_card`, `edit_card`, `move_card`, `comment_card` |
| `POST /v1/automations`, `PUT /v1/automations/{automation}` | `save_automation` (the body is the automation) |
| `PATCH /v1/automations/{automation}`, `POST …/run`, `DELETE …` | `set_automation`, `run_automation`, `delete_automation` |
| `POST /v1/workspaces/{workspace}/shares`, `DELETE /v1/shares/{share}` | `share_workspace`, `revoke_share` |

**Placing and naming.** `add_block` and `open_agent` take `x` and `y`, the block's top-left in canvas units within
±10⁷, both or neither and not with `near`: the block goes there, as one added at a click. `open_agent`'s answer
names its block when it opened at once (not in a new worktree). `connect` takes `label` (one line, at most 60
characters; blank takes it off): on two blocks already wired it renames their wire, else it wires them with that
name; either is one Ctrl+Z step. `list_agents` ends the line of an agent the person hid in Templum's menus with `  hidden`
(it still opens); a mirror's menus leave those out.

**Shells.** `list_shells` names the shells a terminal can run, a name a line, as Add ▸ Terminal ▸ lists them: the
default first, its line ending with `  default` (`zsh  default`), then each other shell found (`fish`, `PowerShell 7`).
Names only, never a program's path. `add_block`'s `shell` (a terminal's) opens one of them by that name, the default
when left out; a name the list doesn't have is refused (`There's no shell "fish": list_shells lists them.`): it is never
a program or a command of the caller's.

**Recent folders.** `recent_folders` names a workspace's recent folders (the folders it opened lately: Git, agents,
editors, terminals), newest first, a folder a line, as its Git popup's folder menu lists them; nothing when there are
none. A mirror of another computer is refused with its usual sentence. A workspace-limited token gets only those that
lie inside its workspaces' folders, links followed, each as its real path, once: never a folder it couldn't open.

### Git

`git` runs one of the Git popup's operations in a repository of a workspace, as the popup does. It is never raw git
arguments: `op` names the operation, its arguments go beside `workspace` (a local workspace; a mirror of another
computer is refused with its usual sentence) and `folder` (a folder at home, whole or relative to the workspace's; its
repository is used; left out, the workspace's own folder), and a path is relative to the repository's root.
`{"workspace":"w3","op":"stage","paths":["src/a.rs"]}` answers
`{"text":"Staged 1 file.","git":null,"workspace":"w3"}`: `text` is a sentence, `git` the operation's result (in the
table; JSON `null` when there is none). Reads take any token; **control** acts; **full** is what can lose work:
`discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop` and `abort`. `GET /v1/tools` lists the
scope by `op`.

| `op` | Arguments (`?` optional) | `git` |
|---|---|---|
| `find`, `init` | – | `{root, main_tree}` (`root` null: no repository; `init` makes one in the folder) |
| `look` | – | `{status, operation, author}` |
| `diff` | `ignore_whitespace?`, `whole?` (`[{staged, path}]`) | `{unstaged, staged}`, the files' hunks and lines |
| `history` | `all?`, `search?` (`{by, text}`), `path?`, `skip?` | `{commits, paths, unpushed}`, 200 each |
| `show` | `hash`, `ignore_whitespace?` | `{commit, files, diffs}` |
| `blame` | `path`, `hash?` | the lines, each with its commit |
| `branches`, `stashes`, `remotes`, `trash` | – | the lists (`trash`: this repository's discards) |
| `unmerged` | `branch` | the number of commits only it has |
| `default_branch`, `last_message` | – | a name or message (or null) |
| `compare` | `base`, `branch`, `hash?`, `ignore_whitespace?` | `{commits, files}` |
| `stash_diff` | `stash` (`{index, hash}`), `ignore_whitespace?` | the stash's files |
| `measure` | `paths` | `{files, bytes, keep}`: what a discard would keep a copy of |
| `set_author` | `name`, `email` | a token for all workspaces writes the global config, a share only the repository's |
| `stage`, `unstage` | `paths` | – |
| `stage_hunk`, `unstage_hunk` | `patch` | – |
| `commit` | `summary` (blank: the staged file's sentence), `description?`, `amend?`, `all?` | `{short, replaced}` |
| `undo_commit` | `hash`, `replaced?` | `{done: message}`, `"pushed"` or `"moved"` |
| `fetch`, `push`, `continue` | – | – |
| `pull` | `how?` (`fast_forward`, `merge`, `rebase`) | – |
| `push_branch`, `merge`, `checkout`, `revert` | `branch` or `hash` | – |
| `publish` | `remote`, `branch` | – |
| `sync` | `join?` (true rebases, false merges) | `{done: {got, sent}}`, `{diverged: …}`, `"gone"`, `"unpublished"` |
| `switch` | `branch`, `remote?`, `leave?` | `"nothing"`, `"popped"` or `{kept: line}` |
| `new_branch`, `tag`, `rename_branch` | `name` (and `hash?`, `hash`, `branch`) | – |
| `stash`, `stash_apply`, `stash_pop` | `message`, `untracked?` or `stash` | – |
| `resolve`, `resolve_region` | `path`, then `side` or `region`, `take`, `shown` | – |
| `restore` | `entry` (an id of `trash`; a path or any other name is refused), `force?` | `"done"` or `{changed: [paths]}` |
| `ignore` | `path` | – |
| `message` | `all?` | `{summary, description, committed}`: home's agent writes it |
| `discard` | `paths` | the copy kept for undoing it (`restore`), or null |
| `discard_hunk` | `path`, `patch` | the same |
| `restore_version` | `hash`, `then` (the path in that commit), `path` | the same |
| `delete_branch` | `branch`, `force?` | – |
| `stash_drop`, `abort` | `stash` · – | – |

A `patch` is one hunk of one file exactly as the diff shows it (the lines `hunk_patch` draws): anything else, a mode, a
rename, a binary or a symlink header included, is `400`. A call checks every string before git sees it: paths are
relative, without `..`, `.git` (however a file system reads it) or a drive; hashes are hexadecimal; names don't start
with `-` or `+`, hold none of `^ ~ : ? * [ \`, `..` or `@{`, and aren't a hash's digits; branches, remotes and
discards must be ones the repository lists; `undo_commit`'s `replaced` must be the commit an amend replaced; a tag can't take a branch's name nor a branch a tag's; a written file isn't a symbolic link and its folder stays inside the
repository. A token limited to some workspaces only reaches a repository whose root and main working tree are inside
their folders (`403` `scope` otherwise), and never has the global author changed. One call runs at a time per
repository, the others wait; a call is stopped after 10 minutes, with everything git started, and the text git writes
is read up to 64 MiB (past it the call is killed and refused: "The output of git is too large to read"). ssh and
credential helpers fail at once instead of asking, but a prompt can still appear on the computer it runs on: a graphical
`GIT_ASKPASS` or `core.askPass` window, or a signing passphrase (`commit.gpgSign`: with the key cached, or `gpg.format=ssh`
with an agent, a commit made through the API is signed as the person). A path names one file, never a pattern (`[ab].txt`
is not `a.txt`). A copy a call keeps in the trash (`discard`, `resolve`, `restore_version`, a forced `restore`) is never pushed out
by another call: it leaves only after 7 days. With 50 of them younger than that for a repository, a call that needs a
copy is refused before it changes anything ("the trash already holds 50 copies made through the API"): restore or clear
them on the computer. The person's own discards are counted apart (the newest 50 each). `message` needs a person to have used Generate at home
once (it asks whether the diff may go to the agent's provider) and a ready agent there. An answer is at most 8 MiB:
a diff that would weigh more comes with the hunks of its biggest files left out and `cut` true on each of them; any
other answer past it is refused. A `422` of `git` adds `details`: git's whole stderr, at most 64 KiB.

**Boards.** `create_card` takes `labels` (red, yellow, green, cyan, blue, magenta). `edit_card` also takes `title`,
`description` (blank clears it), `labels` (`{"red": true, "blue": false}`: turned on, turned off; the rest keep) and
`remove_items` (item ids). `move_card` takes `before`: a card of that column, which the card goes just before; left
out, the column's end. `delete_card` (`card`; control) deletes a card and its comments. `edit_board` (`board`, then
`name` or `delete: true`; control) renames a board or deletes it with its cards. `edit_column` (`board`; control) adds
a column (`column` left out: at the end, or `before` another; `title` names it), renames one (`column`, `title`),
moves one (`column`, `before`: a column of the board, or `"end"`) or deletes one with its cards (`column`, `delete:
true`, alone). `show_board` (`block`, then `board` or `new`; control) shows a board in a kanban block, or makes one
(`new`: its name, blank for "Board n"; `columns`: To do, Doing and Done when left out) and shows it. Each is one
step of the kanban's Ctrl+Z; a limited token reaches only the boards its workspaces' kanbans show. `edit_comment`
and `delete_comment` (`card`, `comment`, and `text`) act as you: edit a comment by `you`, delete any.

**Notes, checklists and ink.** `notes` holds the notes a note block shows, its `text` cut at 256 KiB (`cut` true
then), and each note no note block shows that was last open in a workspace (closed since the version that added
`edit_note`), its `text` empty and `workspace` that workspace's id; a workspace-limited token sees those of its
workspaces. All the texts together take at most 8 MiB as JSON (a control character takes up to 6 bytes), and the
notes past that come with `cut` true and an empty `text`. Every note has `rev`, its text's revision (a number that grows each time its text changes and at each
`edit_note` of its text, from the Unix time in milliseconds when Templum started, so a restarted Templum never reuses
one), and `changed` (Unix seconds). `edit_note` renames a note (`name`, one line; blank is Untitled), tints it
(`color`), or replaces its whole text (`text`, at most 256 KiB) when its `rev` is still `rev`, the one the state showed
with the text the change was made on; else it is refused ("Note 7 changed since: its newer text and rev are in the
state."), and so is a text for a note longer than 256 KiB, which the state shows cut: no change is written over a text
the caller didn't see. The text goes in as one step of the note's Ctrl+Z, your caret kept. `new_note` adds a new note
as a shown tab of a note block (`text` at most 256 KiB). `open_note` shows a note's tab where it is open, else opens it
as a tab of `block`, or in a new note block of `workspace`. `close_note` closes a tab as its × does (a blank note goes).
`delete_note` deletes a note for good, from every block, as the Notes dialog's trash does. A workspace-limited token
reaches only the notes its blocks show and those last closed in its workspaces. None of the five moves your keyboard,
selection or camera, and on a block of a mirrored workspace they are refused ("… is a mirror of …: act on it there.").
Notes closed before this version are listed to no one through the API (they stay in the Notes dialog).
A note block has `notes`: `{"tabs": [7, 9], "shown": 7}`. `checklists` holds the checklists a checklist block
shows; a checklist block that shows one has `checklist`: its id. `drawings` holds one item per workspace with ink, its
id the workspace's: its drawings in paint order, as the canvas file keeps them (`kind` `stroke` with `points`, or
`line`, `arrow`, `rect`, `ellipse` with `from` and `to`; `color` by name; `width`; `highlight`), at most 2,000
drawings and 100,000 points (a shape counts 2), the oldest left out past either. `edit_item` (`block`, `item`, `text`)
and `delete_item` (`block`, `item`; control) change or delete an item of the checklist a checklist block shows, any
block your token sees; each is one step of the checklist's Ctrl+Z. `edit_checklist` (`block`, then `name`, or `item`
with `before`: an item of the checklist it goes just before, or `"end"`, or both; control) renames that checklist
(blank reads "Checklist") or moves one of its items, one step of its Ctrl+Z. `new_checklist` (`block`, `name`;
control) shows a new checklist, named `name` ("Checklist" left out), in a checklist block that shows none; a block
that shows one keeps it and the call is refused (`add_block` opens a block with a new one).

**Who acts.** A call acts as you: no approval, comments by `you`. Tasks it hands (`delegate`, `open_agent`'s prompt) are
typed as `[from API · <token name>, task t7]`, and their end, a `wait_for` with `wait: false` and every other notice
Templum would type into an asker's terminal come as `notice` events. **Tools that wait** (`delegate` with `wait`,
`wait_for`) hold the request up to their timeout; a client that goes away drops what it waited for. **Cloudflare cuts a
response that sends nothing for 100 seconds:** through it, hand the task without `wait` and listen for its `notice`.

### Events

`GET /v1/events` is Server-Sent Events (`text/event-stream`). It opens with `event: snapshot` (the state, as the token
sees it), then one event per change, named for the collection, singular (`workspace`, `block`, `wire` with the id `a-b`
(its two block ids sorted), `request`, `board`, `card`, `note`, `checklist`, `drawing` (the workspace's id),
`automation`, `schedule` (`s3`), `share` (`s4`), `usage` with the id `usage`):

```
id: 8f3a1c20-1042
event: block
data: {"op":"put","id":"b7k2m","value":{…the block, whole…}}

id: 8f3a1c20-1043
event: request
data: {"op":"delete","id":"r12"}
```

A client mirrors the state by applying puts and deletes to the snapshot. A block that moves to another workspace is one
`put` with its new `workspace`; to a token that doesn't see the new one it is a `delete`. `event: notice` is the one
happening that isn't state: `{"text":…,"block":…,"workspace":…}`. A watch set through the API (`wait_for` with
`wait: false`) names the watched block and its workspace, so a token limited to that workspace hears it, and an
automation's Notify me sends `{"text":"Morning review: Look","workspace":"w3","automation":1}` to the tokens that see
its workspace (one for all workspaces: tokens for all workspaces). Templum
compares the state about four times a second (ten while a block is dragged or resized, or the canvas is panned, zoomed
or flown), only while a stream is open.

`GET /v1/events?view=true` also sends `event: view` each time the shown workspace's camera changes: `data` is
`{"workspace":"w3","x":120,"y":40,"w":1280,"h":720,"zoom":1.0}`, the world rectangle its canvas shows (its camera, and
the canvas's size at the last paint) and its zoom (one of the zoom steps, from 0.01 to 3), or `null` when the shown workspace isn't one the token sees. The
state has the same `view` (null before the first paint, and in grid mode, where the canvas shows tiles). `view=false` or none leaves it out; any other value is `400`.

- **Ids** are `<run>-<n>`; `run` is random at each start of the server. `Last-Event-ID` (EventSource sends it by itself)
  of this run gets a fresh `snapshot`, then the notices after it that Templum still keeps (the last 256, at most an hour
  old); of another run, the snapshot alone. State is never replayed: the snapshot is the truth.
- **Heartbeat:** a `: ping` comment every 15 seconds.
- **Backpressure:** each stream queues 256 events; a client that falls behind loses its queue and gets a new `snapshot`
  when it catches up.
- **Cut:** revoking the token, its expiry (checked at each heartbeat), *Disconnect*, turning the API off and
  quitting end its streams. Eight streams per token.
- **Replaced:** a share's token (not one Settings made) that opens `GET /v1/events` with a `Templum-Mirror` header,
  a session of at most 64 characters, other than the one its earlier connections carry, first sends those
  connections' streams `event: replaced` (`data` is `{}`) and ends them, ends its terminal streams on them and cuts
  its other idle connections: a computer that slept leaves its connections open, and they would hold the token's 16.
  A mirror that gets `replaced` while its own connections are live stops: another computer connected with its code.

### Terminals, live

`GET /v1/terminals/{block}/stream` (read scope) is Server-Sent Events of a terminal's screen: what a viewer needs to
draw it, not the bytes of its program. Each `event: frame` has the terminal's size, cursor and title, and rows:

```
id: 8f3a1c20-1050
event: frame
data: {"size":{"cols":120,"rows":40},"cursor":{"row":3,"col":10,"shape":"block","blinking":false},
       "title":"vim main.rs","whole":true,
       "rows":[{"y":0,"runs":[{"text":"main","fg":4,"bg":"#1e1e2e","bold":true},{"text":" fn"}]}, …]}
```

- **A run** carries its own text, so no client counts bytes. `fg` and `bg` are absent for the theme's default, a number
  0–255 for a palette slot (your palette draws it, as zoom draws with your fonts), or `"#rrggbb"`; a slot the program
  set itself (OSC 4, 10, 11) comes as its RGB. `bold`, `dim`, `italic`, `underline` (any kind) and `inverse` are there
  only when true. Hidden text comes as blanks; the second cell of a wide character is left out (draw the character over
  its two columns); combining marks follow their character; trailing blanks with the default background are trimmed,
  and a blank row has no runs.
- **Whole and changed rows.** The first frame has `"whole":true` and every row. After it a frame holds only the rows
  that changed (each with its `y`), and is sent when anything changed: a frame with no rows is the cursor or the title
  moving. Size, `cursor` and `title` are in every frame; `cursor` is `null` while the program hides it, `shape` is
  `block`, `underline` or `beam`, `title` is the program's own (OSC 0/2) or `null` (the block's title is in the state).
- **Modes.** Every frame has `modes`, what the program took of the keyboard and the mouse at home:
  `{"full_screen":true,"mouse":true,"mouse_moves":true,"kitty_keys":true}`, each present only when true (`{}` when
  none). `mouse_moves`: the program asked for every motion (`?1003`), not only clicks and drags: send a move with no
  button held only then. A frame whose only change is a mode has no rows. A client that sends keys yields the
  shortcuts a program would take, and a client that sends the mouse sends only what `mouse` and `mouse_moves` say; a
  home older than this field sends none.
- **The live screen.** The frame is what the program drew, never the scrollback the person scrolled to or their
  selection: `read_terminal` reads the scrollback. Where a terminal's view is in its history (its scrollbar, *N lines
  below*) is the person's, as the camera and the keyboard are: it isn't in the state, and no tool moves it.
- **At most 20 frames a second** per terminal. Each stream queues 20 frames or 8 MiB; a client that falls further behind loses
  them and gets one `whole` frame of the newest screen, never a backlog. A terminal stream doesn't resume: it always
  opens whole, and `Last-Event-ID` is ignored.
- **Tickets.** A browser's `EventSource` opens it with `?ticket=` as it opens the event stream.
- **It ends** when the block is closed or moves out of the token's workspaces (opening it again is `404`, with the words
  of a block that doesn't exist), and as the event stream does: a revoked or expired token, *Disconnect*, the API turned
  off, quitting. A block that isn't a terminal is `422` `refused` ("k9x1d isn't a terminal."). A terminal stream holds
  a connection: it counts in the 64 and in its token's 16 (a 17th is `429`), and opening one is a call (the token's 20 a
  second). There is no cap of its own: a client streams the terminals it shows.
- **A mirror draws** a `whole` frame by replacing its screen; a frame's rows by replacing those rows by `y` (each row's
  runs, left to right, from column 0); then it places the cursor and sets the title. Palette numbers take your own
  colors; `inverse` swaps foreground and background where you draw it. A resize arrives as a `whole` frame with the new
  size: a viewer scales the frame (it never resizes the terminal).

```js
const t = await (await fetch(`${base}/v1/tickets`, { method: 'POST', headers, body: '{}' })).json();
const screen = [];                                   // y → [runs]
const source = new EventSource(`${base}/v1/terminals/${id}/stream?ticket=${t.ticket}`);
source.addEventListener('frame', e => {
  const f = JSON.parse(e.data);
  if (f.whole) screen.length = 0;
  for (const row of f.rows) screen[row.y] = row.runs;
  pre.replaceChildren(...screen.map(runs => {
    const line = document.createElement('div');
    for (const r of runs ?? []) line.appendChild(document.createElement('span')).textContent = r.text;
    return line;
  }));                                                // (colors, flags and the cursor: your own styling)
});
```

**Input.** `POST /v1/terminals/{block}/input` (the tool `terminal_input`, full scope) types as your keyboard does:
`{"text":"ls\n"}` pastes (at most 256 KiB; bracketed when the program asked for it), `{"keys":["escape","j"]}` presses
[a key's name](#a-keys-name), 1 to 256 a call, pressed back to back, with 50 ms after an Esc (or Ctrl+[, Ctrl+3) that another key
follows (so a program still reads Esc alone where the person paused, now that a network bunches keys). The answer
(`Pasted into b7k2m.`, `Pressed 2 keys in b7k2m.`) comes once they are in, so send the next input after it and they keep
their order. Type with `keys`: a character is a key, and a dialog that ignores pastes takes keys. Paste with `text`
for long text. The block shows "answered by API · <token name>". A block that isn't a terminal, a subagent's transcript
(read-only) and a shell that has ended ("b7k2m's shell has ended: nothing typed reaches it.") are `422`; a block the
token doesn't see is `404`. The audit log names the call, never what was typed. No resize.
`{"mouse":[{"kind":"press","button":"left","row":3,"col":10},{"kind":"release","button":"left","row":3,"col":10}]}`
clicks as your mouse does: `kind` `press`, `release` or `move`; `button` `left`, `middle`, `right`, `wheel_up`,
`wheel_down`, or null for a move with none held; `row` and `col` a cell of the grid (clamped); `ctrl`, `alt`, `shift`
optional. Home encodes each in the program's mode at that moment and drops the ones it didn't ask for (`modes`); the
answer counts those taken (`Sent 2 mouse events to b7k2m.`). One of `text`, `keys`, `mouse` a call, 1 to 64 events.

#### A key's name

`terminal_input`'s `keys` and `send_keys` name a key as `[ctrl+][alt+][shift+]<key>`, the modifiers in any order, each
once. `<key>` is `enter`, `escape`, `tab`, `backspace`, `delete`, `insert`, `up`, `down`, `left`, `right`, `home`,
`end`, `page_up`, `page_down`, `space`, `f1`–`f12`, or one character as typed, after Shift and the layout (`A`, `@`,
`ç`, `á`). Examples: `ctrl+shift+p`, `alt+left`, `shift+f5`, `ctrl+alt+delete`, `shift+enter`. For a letter home takes
its lowercase as the key and the letter as its text, as a keyboard reports one; the bytes are made at home, in the
modes the program has at that moment, as a key pressed there. A known limit: a shifted symbol with Ctrl or Alt reaches
the kitty keyboard protocol with the symbol as its key, not the one under it.

### Shares (Templum to Templum)

A **share** lets another Templum mirror one workspace. It is an API token limited to that workspace, Control or Full,
made by *Edit… ▸ Share…* (or `share_workspace`), kept with the share's details in `api.json`; its id is `s<token id>`,
and revoking the token revokes the share. The answer of `share_workspace` holds the **connection code** once, in
`code`: `tplc1_` and the base64url of `{"url":…,"token":"tpl_…","pin":…}` (`pin`, the SHA-256 of Templum's
certificate, only for a share made *Directly*). Templum keeps only the token's fingerprint, so the code cannot be
shown again. `way` is `pinned` (the other computer pins Templum's certificate: `address` is `host:port` and the API
must listen on every network with Templum's certificate) or `public` (a tunnel or proxy with a public certificate:
`address` is `https://…`, or `http://` to this computer for an `ssh -L` forward). Refusals say what to fix: "A pinned
share needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS.", "The address must start with https://.".

`GET /v1/me` says who the token is, without asking the UI thread:

```json
{ "name": "Another computer", "scope": "full", "workspaces": ["w3"], "expires": 1764150000, "share": "s4",
  "computer": "winx-desktop", "version": "0.1.6", "api": 1 }
```

`share` is null for a token Settings made; `workspaces` is empty for a share whose workspace was deleted at home (the
share stays, saying "share of a deleted workspace" in Settings, until it is revoked). The state's `shares`, and the
`share` events, are:

```json
{ "id": "s4", "workspace": "w3", "name": "Another computer", "scope": "full", "created": 1761560000,
  "expires": 1764150000, "way": "public", "address": "https://templum.example.com",
  "last": { "at": 1761561000, "addr": "203.0.113.7" }, "mirrored": true }
```

`mirrored` is true while an event stream of its token is open. A token for all workspaces sees every share; a limited
token sees only the share it is. The state also gains a block's `read_only` (true for a terminal that takes no input,
a subagent's transcript; left out otherwise) and a workspace's `icon` (its Lucide name, or null). A workspace this
Templum mirrors from another computer has `remote` (`computer`, `host`, `way`, `status`, `latency_ms`), and its blocks
are never in the state. `revoke_share` with no `share` revokes the calling token's own; a share may revoke itself, and
revoking another takes a Full token for all workspaces.

### Errors and limits

Every error is `{"error":{"code":"…","message":"…"}}`.

| Status | `code` | When |
|---|---|---|
| 400 | `invalid` | Malformed JSON, an argument that doesn't parse, a tool's argument error |
| 401 | `auth` | No token, a wrong, expired or revoked one, a used or stale ticket |
| 403 | `scope` | The token's scope is too low |
| 404 | `not_found` | No such route, tool or resource (a target the token doesn't see is this too) |
| 413 | `too_large` | A body over 1 MiB |
| 415 | `json` | A POST, PUT or PATCH whose `Content-Type` isn't `application/json` |
| 422 | `refused` | The tool refused; its sentence says what to do next (`git` adds `details`, git's stderr, at most 64 KiB) |
| 429 | `slow_down` | Too many failed authentications from the address, calls from the token, or streams; `Retry-After` |
| 503 | `busy` | Too many connections (64), or Templum is quitting |

A terminal stream holds a connection like an event stream: 16 per token, 64 in all (`429`, then `503`).

Request headers within 10 s and 16 KiB; 20 calls a second per token, bursts of 40. Every answer carries
`X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, `X-Frame-Options: DENY`,
a `Content-Security-Policy` of `default-src 'none'; frame-ancestors 'none'` and `Cross-Origin-Resource-Policy:
same-origin`. There is no `Strict-Transport-Security`: a browser would pin the host for a year, and Templum's
certificate (or a tunnel's) changes. CORS is off unless you list origins in Settings (whole
origins, no `*`): a listed origin gets `Access-Control-Allow-Origin` with its own value, and preflights are answered
without a token.

### The audit log and what is kept

`<data>/api/audit.log` has one JSON line per call: time (`at`, Unix seconds), address, token name, what (a tool's
name, a resource's path; cut at 200 characters) and its status. If calls were ever given up because the disk could not
keep up, one line `lost` says how many (`more`). What anyone can make without a token (connections, refused or failed
authentications and handshakes) is in `<data>/api/noise.log`, one line per address and minute with `more` counting the
rest, so a flood of strangers can't fill a log or push the calls out of `audit.log`. A mirror's chatter is grouped the
same way, in `audit.log`: `terminal_input` and `GET /v1/me` calls are one line per token, call, status and minute,
with `more` counting the rest, so a morning of typing can't rotate the other calls out; every other call keeps its own
line. `git` is written `git:<op>`, and `git:look` and `git:diff` (an open popup reads them every 2 s) are grouped as
`terminal_input` is, and so is `read_terminal` (a mirror reads a terminal's history through it). A mirror
that replaces another's session (see `Templum-Mirror`) writes one `mirror_replaced` line. Never a
token, a ticket, a body or typed text. At 1 MiB each becomes `audit.log.1` and `noise.log.1`. `<data>/api/api.json`
holds the settings and the tokens' hashes (`0600`, in a `0700` folder on Unix); a copy of the data folder carries no
token that works. Each token's last use (time and address) is in Settings.

### Versioning

`/v1` grows only by additions: new tools, resources, fields, event names and error codes. Clients ignore what they don't
know. A change that would break a client is `/v2`, served beside `/v1` for at least one release. `app.api` in the state
says the newest version served. The whole schema is `GET /v1/openapi.json`.

## Templum to Templum

Another computer's Templum can mirror one of your workspaces, live: its blocks where you have them, your terminals
showing what they show, your agents' dots and requests, and from there the person types into your terminals, moves your
blocks and answers your agents' questions. **Your computer is the truth.** Nothing runs on the mirror: it draws what the
API above says and sends your keys, moves and answers back through it. Design:
[specs/templum-to-templum.md](specs/templum-to-templum.md).

*How this was checked:* the tests run both ends in one process over loopback (a Templum that mirrors itself), including
typing into a shell, moving a block and answering a question; the screens (the Share… and Connect dialogs, the banner,
Edit…) were checked headlessly, and **not** with two `--dev` instances on a display. Try the steps below with two
`--dev` builds before you rely on them.

### 1. Share a workspace (home)

On the computer that has the workspace: right-click its tile ▸ **Edit…** ▸ **Share…** (or the tool `share_workspace`).

- **Name:** what the share is called on this computer ("Another computer").
- **Can:** **Control** watches and works with blocks, cards, checklists, Git and automations, opens agents and hands
  them tasks, and answers requests; it does not type into terminals, and agents it opens can run commands. **Full** also
  types and clicks into terminals, discards Git changes, and deletes branches, stashes and automations (anyone holding
  the code can run commands on this computer).
- **Expires:** 30 days by default; shorter is safer. Three days before, the mirror's banner says so.
- **The other computer reaches this one:** *Through a tunnel or proxy* (a public `https://` address, for
  [cloudflared](#2-cloudflared) and the like; the API stays on this computer only) or *Directly* (`host:port` of this
  computer, which pins Templum's certificate; the API must listen on every network with Templum's certificate,
  Settings ▸ API ▸ HTTPS). Make code says what to fix when the API doesn't fit.
- **Make code** shows the connection code once (Copy). Templum keeps only a fingerprint, so a lost code means a new
  share. The sidebar tile of a shared workspace gets a small cast mark while another computer mirrors it.
- **Revoke** is in the same Edit… dialog (and in Settings ▸ API); the mirror is cut at once.
  `share_workspace` and `revoke_share` do the same for scripts, see [Shares](#shares-templum-to-templum).

Only one computer can use a code at a time: a second one that connects with it stops the first ("Another computer
connected with this share's code"). Give each computer its own share.

### 2. cloudflared

To reach home from outside, without opening a port, run a tunnel to the API on home:

```sh
cloudflared tunnel --url http://127.0.0.1:7411
```

(or a named tunnel to the API's port). Share *Through a tunnel or proxy* with the tunnel's `https://` address. If the
tunnel sits behind **Cloudflare Access**, make a **service token** and allow it in the Access policy (recommended: the
tunnel then refuses everyone but the mirror); the mirror takes the token's Client ID and Secret in *Connect ▸ More…*.
The tunnel's provider sees the traffic (HTTPS ends at its edge), including what is typed.

### 3. Connect (the mirror)

Sidebar ▸ **Connect to a Templum…**, paste the code. The line under the field says where it goes and how it checks
(`templum.example.com · checks its public certificate`, or `· pins Templum's certificate AB:CD:…`). **More…** takes the
Cloudflare Access Client ID and Secret, and a proxy. The key is kept in this computer's keyring (the Keychain, the
Credential Manager or the Secret Service); with no keyring you may keep it in a file only you can read (a copy of the
data folder would carry it) or let it last until Templum quits. Connect tries it; on success the dialog closes, the
remote workspace opens and the code leaves the clipboard if it still holds it. A failure stays in the dialog in words.

The remote workspace is a tile in the sidebar like any other (with a mark), in Ctrl+Tab and Ctrl+1–9. Right-click it:
**Open**, **Edit…**, **Disconnect** (or **Connect**), **Forget…** (which can revoke the share on home too).
**Edit…** shows the connection (address, way in, whether it trusts an authority this computer added, the Access Client
ID, the proxy, what the share can do, when it ends, the round trip), your own icon and color (none: home's), and
**Paste a new code…** for when the share expired or was revoked. Its **More…** opens with the Access Client ID and the
proxy address this connection has; leave the Client Secret (or the proxy's login) empty and the old one stays, so you
don't need to copy it again, for the same Client ID and address (a new code for another address needs the Client
Secret typed again). The name is home's.

**On a mirrored workspace.** Not mirrored: an editor's content (a card names it). Scrolling back in a mirrored shell (the wheel, Shift+PageUp,
Find) reads home's last 2000 lines, without colors, and again after it changed, and its scrollbar and *N lines below*
work over them; the mouse reaches a program that asked
for it, with a Full share. Notes are edited there as here: what you type goes to home after a pause, checked
against home's text; a change home made in the same place keeps both texts, yours as a new note beside it. Not on a
mirrored workspace: editors, deleting a checklist, drawing, Move
to workspace, Copy and Paste of blocks, Stop. The banner's ⓘ lists them. Fullscreen (grid mode) works there as on your
own workspaces: it tiles home's blocks on this computer and moves nothing at home. The Git popup works on a mirrored workspace:
everything it reads and does is home's `git` (reads with any share, acts with Control, discards, Restore version,
deleting a branch, dropping a stash and aborting with Full, dimmed on a Control share); Fetch, Pull, Push, Sync,
Publish and Generate wait up to 10 minutes, and one a tunnel cuts says it may still be running on home; Generate uses
home's agent; its folder ▾ lists home's folders of the workspace (its folder, its terminals' and its recent ones);
Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a terminal stay on home. Adding, closing and wiring
blocks, naming wires, opening home's agents and handing them tasks work from the mirror, each done at home through its
API. Add ▸ Terminal ▸ and the toolbar's Terminal list home's shells (Default, then the others home has), never this
computer's; the one you pick opens at home (an older Templum at home opens its default only). A block you close or a wire you make shows at once and home's state wins after two seconds. Ctrl+Z there undoes
moves only: it stops once at each of those acts and says to undo the rest on home. A selection of several blocks moved
on the mirror is one Ctrl+Z step per block on home. A kanban is live: dragging cards and columns, adding, renaming and
deleting columns and boards, the card dialog and Send to work from the mirror, each done at home; a card you move,
edit or delete shows at once and home's state wins after two seconds; a card's attachments show by name (open them on
home), and a card has no drafts on a mirror. A checklist is live: its items are checked, added, edited and deleted from
the mirror, each done at home (an item's text goes when you press Enter or leave it); a check, an edit or a delete
shows at once and home's state wins after two seconds. Renaming a checklist and moving its items stay on home. Home's
ink and shapes are drawn, not picked: draw on home.

The Agents popup lists home's agents under "API · winx-desktop" with their task, model, role and usage. The
Automations popup shows home's rules of the shared workspace in a group of their own, and switches, runs, edits,
duplicates and deletes them there (a rule that runs a command, and a delete, need a Full share); this computer's
rules never run against a mirrored workspace: home's automations run only on home, and the mirror never copies,
schedules, runs or fires one. What its agents asked for later is listed there and cancelled at home. The Requests
inbox lists home's requests, aged as home says, one whose block has no viewer here too, and says how many more home
has past 50.

### 4. What each way in checks

- **Pinned** (*Directly*): the certificate's SHA-256 must be the one in the code. After *New certificate* on home, share
  again and paste the new code.
- **Public** (*Through a tunnel or proxy*): the certificate must be valid for the name, signed by an authority the
  system trusts (then Mozilla's list). A company proxy that reads HTTPS shows its own authority: Templum refuses it
  unless you accept it for that remote ("Connect anyway"), because it could read your key and what you type.
- **Loopback** (`http://127.0.0.1…`): plain HTTP to this computer, for an `ssh -L` forward. No proxy is ever
  used for it.

### 5. A proxy

Type it in *Connect ▸ More…* (`http://proxy.corp:8080`, optionally with `user:password@`), or let Templum read
`HTTPS_PROXY`, `https_proxy` or `ALL_PROXY` (not for a host `NO_PROXY` names). Templum opens an HTTP `CONNECT` through
it, then TLS inside, end to end. Basic credentials only: NTLM, Kerberos, PAC files and the system's proxy settings are
not supported, and a proxy that wants them answers 407 ("The proxy … refused the connection").

### 6. When it isn't live

The tile dims, the canvas keeps its last blocks under a veil, agents' dots go grey, typing says "Not connected: what you
type isn't sent." (keys are dropped, never sent late), and moving and answering are disabled.

What the banner says, what it means, and whether it tries again by itself:

- **Reconnecting to winx-desktop… try 3, next in 8 s:** home doesn't answer (off, asleep, no route, the tunnel's 502
  or 503). Retries, waiting longer each time; **Try now** tries at once.
- **This share of API expired**, **winx-desktop revoked this share:** the token is no longer good. No retry: **Paste a
  new code…** after sharing again, or **Forget**.
- **API was deleted on winx-desktop:** its workspace is gone. **Forget**.
- **API is stopped on winx-desktop:** not dimmed; **Start it** starts it on home and shows it there.
- **Another computer connected with this share's code:** two computers used one code. **Connect here again** (the
  other will stop), or **Forget**.
- **winx-desktop's certificate changed:** the pin differs, after a *New certificate* on home, or someone is between
  the two computers. Paste a new code only if you made the change.
- **…'s certificate isn't valid:** expired, for another name, or signed by an unknown authority. Fix it where the
  tunnel or proxy runs, then **Try again**.
- **Cloudflare Access stands in front of …:** add the service token in Paste a new code… ▸ More…, and allow it in the policy.
- **… didn't answer as a Templum:** wrong address or port. **Try again**.

### 7. What a mirror can't do yet

What a workspace shows is mirrored live, and what you do on it that is one discrete change is done at home. These stay
on home (each is its own card):

1. Drawing: drawing, erasing, moving and deleting ink and shapes (they are drawn live).
2. An editor's files: an editor shows as a card.
3. Following home's camera.
4. Reaching home through a tunnel with Templum's own pinned certificate inside it.
5. Undo beyond moves: Ctrl+Z there undoes moves only (a note's text keeps its editor's own Ctrl+Z).
6. A card's attachments: their names and sizes show, open them on home.
7. Drafts of a kanban card.
8. Move to workspace, and Copy and Paste of blocks (Copy and Paste between a mirror and a local workspace stay off for
    good).
9. In the Git popup: Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a terminal.
10. Each subagent's line in the Agents popup (their count shows).
11. Moving several blocks at once as one step at home.
12. Stopping the workspace on home (Start stays).

### 8. Safety

- **A Full code runs commands on home.** Treat it like a password: short expiries, Revoke when done, and never put it
  where an agent reads it.
- **Keep agents you don't trust away from a computer that holds a Full code** (its keyring, or the key file if you
  chose one): whoever reads the key can run commands on home.
- The mirror never relays: no tool, API call or agent of the mirror reaches home's blocks, and what home sends is drawn,
  never run (control characters are dropped, sizes and counts are capped).
- The tunnel's provider sees the traffic; a *Directly* share pins Templum's own certificate instead.
