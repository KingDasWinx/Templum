# The Templum API: your Templum, from anywhere

- **Date:** 2026-10-24
- **Status:** Batches 1 and 2 implemented (kanban card 171, "the Templum ecosystem API"; plans:
  [2026-10-24-api.md](../plans/2026-10-24-api.md), [2026-10-24-api-batch-2.md](../plans/2026-10-24-api-batch-2.md));
  Templum to Templum is designed in [templum-to-templum.md](templum-to-templum.md). For people:
  [docs/api.md](../api.md).
- **Builds on:** [connections.md](connections.md) (the bus, block ids, the tools, the `templum` command line),
  [templum-agent.md](templum-agent.md) (the control server: a tool for every hand action, and their danger),
  [agents-view.md](agents-view.md) (an agent's state), [ask-inbox.md](ask-inbox.md) (the requests),
  [automations.md](automations.md), [settings.md](settings.md), [workspaces.md](workspaces.md)

The person said "pode decidir, fomente bem a ideia para ser uma funcionalidade bem boa" (decide it, grow the idea into
a really good feature) and, on security, "take the proper measures to be safe, with encrypted connections". The calls
below are the orchestrator's, marked *the orchestrator's call*; what the designer settled is marked *the designer's
call*.

## Intent

Card 171, in the person's words (translated): "My idea is that the user can expose an API of their Templum, so we have
an ecosystem. I have a Templum open on one computer and another on another computer; I could connect to the API and
see everything that is happening, move blocks, create blocks, do literally everything. The same for mobile: the person
leaves home and gets an ask_user on the phone, or fires other agents, or sees how things are going. And Templum to
Templum: personal PC and work PC. We need a key system: keys with an expiry date or unlimited. Most important: Templum
only opens the access ports; the user is responsible for exposing it on the web with a domain, VPN or whatever — we
have no server. I thought HTTP/HTTPS, because with a cloudflared tunnel (my case) raw TCP is a hassle. How to sync the
canvas in real time? One user moves something and it changes on the other PC, and terminals are seen live too. Two
parts: 1) a global API with access to everything and access control, all documented, so a user can build their own
dashboard on it; 2) Templum-to-Templum connections. Every feature of the system must be in the API, or everything
breaks."

Today everything Templum does happens in front of its window. An agent that asks a question while the person is out
waits until they come back; a dashboard, a phone or a second computer has no way in. The agents already have two doors
(the block server and the Templum Agent's control server, both on a local socket), but neither is reachable from
outside the computer, and neither has keys a person can hand out and take back.

The **Templum API** is a third door: an HTTP server inside Templum, off until the person turns it on, that answers only
to **tokens** the person makes, with a scope and an expiry, and revokes at will. It serves the same tools the control
server serves, resource-shaped JSON for the things a dashboard lists, and one live stream of everything that changes.
Templum never runs a server of its own: the person points a tunnel, a proxy or a VPN at the port.

## Goals

1. **Off and private by default.** Nothing listens until the person turns the API on in Settings ▸ API; then it
   listens on `127.0.0.1`, where a cloudflared tunnel or a reverse proxy reaches it.
2. **Keys the person controls.** Tokens with a name, a scope (read, control, full), an expiry (a date, N days, never)
   and optionally the workspaces they may see; shown once, stored only as a hash, revoked at once, each with its last
   use.
3. **Everything, in one registry.** Every control tool is `POST /v1/tools/{name}`; the main things have REST
   resources; the OpenAPI document is generated from the same registry, so a tool added is an endpoint and its docs.
4. **Live.** `GET /v1/events` streams a snapshot, then every change: blocks added, moved, resized, removed; agents'
   states; requests opened and answered; cards; automations; resumable after a dropped connection.
5. **The phone case works on batch 1's API alone:** see the requests (questions, plans, yes/no requests, the Templum
   Agent's approvals) and answer them; see the agents and their states; open an agent and hand one a task; read and
   move cards.
6. **Encrypted off the computer.** Any address other than loopback requires TLS: Templum's own certificate, whose
   fingerprint it shows for pinning, or the person's.
7. **Documented for people:** [docs/api.md](../api.md) with a quickstart (turn it on, make a token, run cloudflared,
   curl, events from JavaScript), and `GET /v1/openapi.json` for tools.
8. **Ready for Templum to Templum:** what a second Templum needs to mirror a workspace (workspace-limited tokens,
   stable ids, a state complete enough to draw, a pinned certificate) is in batch 1; terminals' live frames in batch 2.

## Non-goals

*The orchestrator's call* for each:

- A server or relay Templum runs; any hosted service.
- User accounts, passwords, OAuth: tokens only.
- GraphQL; raw TCP; WebSocket (SSE does everything below: [Why SSE](#why-sse)).
- Part 2's connection UI (a remote workspace in the sidebar): its own design,
  [templum-to-templum.md](templum-to-templum.md).
- A mobile app, or a web page served by Templum (the person: "eu disse que queria só a api e o sistema de conexão",
  only the API and the connection system): a phone reaches Templum through the API with a client of its own.

*The designer's call:*

- Changing `config.toml` from the API (`set_setting` stays a planned control tool), or reading files outside what the
  tools already read (an editor's files, attachments' bytes).
- MCP over HTTP (the agents keep the local bus): the API's shape is REST and SSE.

## Anti-goals (a failure even if it works)

- Anything listening on the network that the person didn't turn on, or listening beyond loopback in plain HTTP.
- A token readable from the data folder, a log, a URL, a crash report or the audit log.
- A feature that works in the app and not in the API, from this card on (CLAUDE.md's rule).
- The UI thread waiting on the network, or painting slowed while a client is connected.
- An agent in a terminal getting the API's powers through the bus.
- A stream that grows memory without bound because one client reads slowly.

## Words

- **Token:** the key a client sends in `Authorization: Bearer tpl_…`. Made in Settings ▸ API, shown once.
- **Scope:** what a token may do: **read**, **control** or **full**, each including the one before.
- **Client:** an open connection that showed a valid token.
- **Ticket:** a short-lived, single-use key for `GET /v1/events` from a browser, which can't set headers there.
- **State:** everything the API shows as data, as one JSON document: workspaces, blocks (with their agents), wires,
  requests, boards, cards, automations, usage.
- **Event:** one change to the state, or a happening (a notice), on the stream.

## What a person sees

### Settings ▸ API

A new section after Connections (*the designer's call*: next to the other doors). From top to bottom:

| Field | Control | Saved as |
|---|---|---|
| Turn on the API | Switch, off. Under it, when on: "Listening on http://127.0.0.1:7411" (the address in use, `https` with TLS), with **Copy**, or why it isn't ("Port 7411 is in use: choose another port.") | `enabled` |
| Who can reach it | Dropdown: **This computer only (127.0.0.1)**, **Every network of this computer (0.0.0.0)**, **One address…** (a text field, an IP of this computer) | `listen` |
| Port | Number 1024–65535, default **7411** (*the designer's call*: fixed, so a tunnel's config stays right) | `port` |
| HTTPS | Dropdown: **Off: a tunnel or proxy gives HTTPS** (only with This computer only), **Templum's certificate**, **My certificate…** (two file fields: certificate and private key, PEM) | `tls` |
| Certificate | With Templum's certificate: its SHA-256 fingerprint (`AB:CD:…`, 32 pairs) with **Copy**, **Save certificate…** (writes a `.pem` to install on a phone) and **New certificate** (asks first: "Clients that pinned the old one stop connecting.") | `<data>/api/cert.pem` |
| Allowed web pages | Text, one origin per line (`https://dash.example.com`); empty: no web page may call the API (CORS off) | `origins` |

Picking **Every network** or **One address…** while HTTPS is Off switches HTTPS to Templum's certificate and says so
under the field: "Off this computer, the API needs HTTPS." With This computer only and HTTPS Off, the line under HTTPS
says: "Plain HTTP on this computer: give HTTPS with your tunnel or proxy (cloudflared does)."

**Tokens**, under the fields:

- One row per token: its name; under it, dim, its scope, its workspaces ("all workspaces" or their names), "expires
  in 29 days" / "never expires" / "expired 2 days ago" (yellow), and "last used 3 min ago from 203.0.113.7" / "never
  used". **Revoke** on the right, at once (no question: a token is cheap to make again).
- **New token…** opens a dialog: **Name** (one line, at most 60 characters, required); **Can** (Read: see everything,
  terminals' text included; Control: also act, answer requests, open agents, hand tasks, run automations — the
  default; Full: also type into terminals and press keys — with the line "Anyone with a Full token can run commands on
  this computer."); **Expires** (In 1 day, 7 days, **30 days** (default), 90 days, 1 year, On a date… (`YYYY-MM-DD`,
  the end of that day, local time), Never); **Workspaces** (All, or chosen ones with toggles). **Make token**.
- The dialog then shows the token once, whole, in the terminal's font, with **Copy** and the line "Copy it now:
  Templum keeps only a fingerprint of it and can't show it again." **Done** closes it; the text is gone from memory.
- **Revoke all** under the list (asks: "Revoke all 3 tokens? Every client is cut at once.").

**Connected now**: one row per client: the token's name, its address, since when, "listening to events" when it holds
a stream; **Disconnect** (closes its connection; the token keeps working). "No client is connected." when empty.

### The title bar

While the API is on, a pill before the usage meter: **API** with the number of clients ("API · 2"); tooltip "The API
listens on http://127.0.0.1:7411 · 2 clients connected". A click opens Settings ▸ API. When it can't listen, the pill
is yellow, "API off", tooltip the reason. Off, no pill (*the designer's call*: the person always sees a door that is
open).

### The rest of the app

Nothing else changes: an action that came through the API looks as if the person had made it by hand (a card moved, a
block opened, a question answered: the inbox's row goes), with these words where an author shows:

- A task handed through the API is typed as `[from API · phone, task t7]` (the token's name), so the agent and the
  person can tell.
- A notification sent through the API is titled "API · phone".
- A comment written through the API is the person's (`by: "you"`): the token is theirs (*the designer's call*).

## The API's shape

*The orchestrator's call:* HTTP only. JSON REST under `/v1`; real time by Server-Sent Events; input by POST.

### Why SSE

An event stream is one long HTTP response, which cloudflared, Cloudflare, nginx and Caddy pass through as is, and which
browsers resume by themselves (`EventSource`, `Last-Event-ID`). Everything here sends from Templum to the client;
everything the client sends is a POST. WebSocket would add an upgrade every proxy must allow and a second protocol,
for nothing the plan needs (*the designer's call*: batch 2's terminal input is a POST per keystroke burst, under
interactive typing rates; if a measured round trip through a tunnel proves too slow for typing, a later card may add
one WebSocket for input only).

### Authentication

- **`Authorization: Bearer <token>` is the only way in** (*the orchestrator's call*). No cookie, no basic auth, no
  token in a URL (a URL lands in proxy logs, browser history and referrers).
- **Tickets:** `POST /v1/tickets` (authenticated) answers `{"ticket":"tkt_…","expires_in":30}`. A ticket opens one
  `GET /v1/events?ticket=…` within 30 seconds, once, with its token's scope and workspaces; it is the only key a URL
  may carry, and only on the event stream (and batch 2's terminal streams). Revoking the token voids its tickets.
- **A token** is `tpl_` and 43 base64url characters: 256 random bits from the system's generator (ring's
  `SystemRandom`). The prefix lets secret scanners (GitHub's) find a leaked one (*the designer's call*).
- **Stored as** its SHA-256, in hex; a presented token is hashed and compared with every stored hash in constant time.
  (A slow hash is for guessable passwords; a 256-bit random token can't be guessed, *the designer's call*.)
- **Failed authentication** (no token, a wrong one, an expired or revoked one, a used or stale ticket): `401` with
  `WWW-Authenticate: Bearer` and `{"error":{"code":"auth","message":"…"}}`, the same body whatever the reason.

### Scopes

*The orchestrator's call* for the three; *the designer's call* for how a tool gets one:

| Scope | May |
|---|---|
| **read** | Every GET, the event stream, and the tools that only read (`overview`, `list_agents`, `list_shells`, `recent_folders`, `read_terminal`, `list_cards`, `read_note`, `read_checklist`, `wait_for`) |
| **control** | Also every other tool whose danger in the control server is **–**, **Ask own** or **Ask own when losing** ([templum-agent.md](templum-agent.md#tools)): opening agents, handing and interrupting tasks, blocks, wires, cards, notes, checklists, showing; answering questions, plans and requests to connect; running, switching and saving automations that run no command |
| **full** | Also the tools whose danger is **Ask** or **Ask each** (`send_to_terminal`, `send_keys`, and those to come: `run`, `set_setting`, deletions that lose work), saving an automation that runs a command, deleting an automation, answering a request to delete or merge a worktree, and answering the Templum Agent's approvals |

- A tool's scope comes from the registry: a control tool is **read** when the registry marks it as reading
  (`templum_mcp::control::reads`), **full** when `control::rule` gives Ask or Ask each, **control** otherwise. A new
  tool is control until marked otherwise, and a dangerous one is already marked for the Templum Agent's gate.
- **The scope replaces the gate.** A call through the API never shows the Templum Agent's approval: the token is the
  person's own hand, and its scope says how far it reaches (*the designer's call*). So answering the Templum Agent's
  approvals takes **full**: a token that may run the call itself may approve it. (templum-agent.md's "approvals are
  answered only by clicks and keys in Templum's popup, never by a tool, a chat message or a CLI" gains: "or by the
  API's full tokens, the person's own hand".)
- Too low a scope: `403` `{"error":{"code":"scope","message":"send_to_terminal needs a Full token."}}`.

### Workspaces a token may see

A token made for chosen workspaces (*the orchestrator's call*; the rules are *the designer's call*):

- It sees those workspaces, their blocks and wires, the requests of their blocks, the boards a kanban block of theirs
  shows (with those boards' cards), the notes and checklists a note or checklist block of theirs shows, the drawings of
  those workspaces, and the automations whose Where is one of them. It sees no usage (an account's,
  not a workspace's), no automation for all workspaces and none of the Templum Agent's approvals.
- It acts only there: every block, workspace, board, card, request and automation a call names must be one it sees;
  `create_workspace` is refused. A target it doesn't see is answered exactly as one that doesn't exist ("There's no
  block k9x1d: GET /v1/blocks lists them."), so a limited token can't learn what lies outside.
- A workspace it names that is deleted is gone for it; a block moved out of its workspaces leaves its view (a
  `delete` event), one moved in joins it.
- The check runs on the UI thread, on the live workspaces, at the call. Which arguments name targets is one list in
  the registry (`block`, `a`, `b`, `near`, `connect`, `for`, `workspace`, `board`, `card`, `request`, `automation`);
  a test fails when a tool has an argument the list doesn't classify.

### Resources

Every GET reads a fresh state from the UI thread (one round trip), filtered for the token
(*the designer's call*: no cache to go stale).

| Method and path | Answers | Scope |
|---|---|---|
| `GET /v1/state` | The whole state: `{app, workspaces, blocks, wires, requests, boards, cards, notes, checklists, drawings, automations, schedules, usage}` | read |
| `GET /v1/workspaces`, `/v1/workspaces/{workspace}` | Workspaces in the sidebar's order | read |
| `GET /v1/blocks`, `/v1/blocks/{block}` | Blocks, back to front; `?workspace=w3` filters | read |
| `GET /v1/agents` | The blocks running an agent, with its state | read |
| `GET /v1/terminals/{block}/text` | `read_terminal`; `?lines=200&last_command=true` | read |
| `GET /v1/notes/{block}/text`, `/v1/checklists/{block}` | `read_note`, `read_checklist` | read |
| `GET /v1/requests`, `/v1/requests/{request}` | What waits for the person, oldest first | read |
| `GET /v1/boards`, `/v1/boards/{board}`, `/v1/cards/{card}` | Boards with their columns' card ids; a card whole | read |
| `GET /v1/automations`, `/v1/automations/{automation}` | The person's automations, with their next and last runs | read |
| `GET /v1/usage` | The agents' plan limits | read |
| `GET /v1/tools` | The registry: every tool's name, description, input schema and scope | read |
| `GET /v1/openapi.json` | The OpenAPI 3.1 document, generated | read |
| `GET /v1/events` | The event stream ([Events](#events)) | read |

The state, in short (*the designer's call*; the OpenAPI document has every field):

```json
{ "app": { "version": "0.1.5", "api": 1, "shown": "w3" },
  "workspaces": [{ "id": "w3", "name": "API", "folder": "/home/me/api", "color": "blue", "shown": true,
                   "stopped": false, "opened": 1761559000 }],
  "blocks": [{ "id": "b7k2m", "workspace": "w3", "kind": "terminal", "title": "Claude Code · api",
               "folder": "/home/me/api", "rect": { "x": 0, "y": 0, "w": 720, "h": 480 }, "z": 3,
               "opened_by": null, "board": null,
               "agent": { "agent": "claude", "state": "needs_you", "asks": "Allow Bash?", "subagents": 0,
                          "since": 1761560000, "model": "opus", "effort": "high",
                          "usage": { "context_percent": 41.0, "tokens": 120000, "cost_usd": 1.92 },
                          "task": { "id": "t7", "text": "Fix the login", "by": "k9x1d", "since": 1761559900 },
                          "role": "reviewer" } }],
  "wires": [{ "a": "b7k2m", "b": "k9x1d", "label": null }],
  "requests": [{ "id": "r12", "kind": "question", "block": "b7k2m", "workspace": "w3", "since": 1761560100,
                 "summary": "Which database?", "why": { "card": { "id": 152, "title": "Attachments" }, "task": null,
                 "agent": "Choosing the store decides the migration." },
                 "questions": [{ "question": "Which database?", "header": "DB", "multi": false,
                                 "options": [{ "label": "Postgres", "description": null, "preview": null }] }],
                 "plan": null, "buttons": null, "stopped": false, "scope": "control" }],
  "boards": [{ "id": 3, "name": "Sprint", "columns": [{ "id": 7, "title": "Doing", "cards": [40, 41] }] }],
  "cards": [{ "id": 40, "board": 3, "column": 7, "title": "Login", "description": "…", "priority": "high",
              "assignee": { "block": "b7k2m" }, "checklist": [{ "id": 41, "text": "Build", "done": false }],
              "comments": [{ "number": 51, "by": "b7k2m", "text": "…", "at": 1761550000, "edited": false }],
              "attachments": [{ "name": "shot.png", "size": 48128 }] }],
  "notes": [{ "id": 7, "name": "Plan", "text": "first\nsecond", "color": "blue", "cut": false,
             "rev": 1761550000123, "changed": 1761550000 },
            { "id": 5, "name": "Old", "text": "", "color": null, "cut": false, "rev": 1761550000100,
              "changed": 1761540000, "workspace": "w3" }],
  "checklists": [{ "id": 12, "name": "Release", "items": [{ "id": 13, "text": "Tag it", "done": false }] }],
  "drawings": [{ "id": "w3", "drawings": [{ "kind": "stroke", "color": "blue", "width": 2.0,
                                            "points": [[0, 0], [100, 100]] }] }],
  "automations": [{ "id": 1, "name": "Morning review", "on": true, "workspace": "w3", "when": { "…": "…" },
                    "do": { "…": "…" }, "runs": 3, "last": { "at": 1761555600, "went": "ran", "about": "…" },
                    "next": 1761642000, "sentence": "Every weekday at 09:00, type into abc12" }],
  "schedules": [{ "id": "s3", "by": "b7k2m", "to": "k9x1d", "text": "Check the deploy", "next": 1761562000,
                  "every": 600, "until": 1762166800 }],
  "usage": { "limits": [{ "agent": "claude", "at": 1761559000,
                          "windows": [{ "minutes": 300, "used_percent": 62.0, "resets_at": 1761570000,
                                        "scope": null }] }] } }
```

- Ids are the app's own: block ids (`b7k2m`), workspace ids (`w3`), board, card, column, item and comment numbers,
  automation numbers. A request's id is `r<n>` for the inbox's (a question, a plan, a yes/no request) and `a<n>` for the
  Templum Agent's approvals.
- An agent's `state` is the Agents view's ([agents-view.md](agents-view.md#an-agents-state)): `needs_you`,
  `starting`, `waiting_subagents`, `working`, `done`, `idle`; `since` is when it last changed kind, in Unix seconds.
  Times are Unix seconds everywhere.
- An agent's `task` is the task it works on (`id` `t7`, its `text` as Templum keeps it, at most 2,000 characters, `by`
  the block that handed it, `"api"` for the API, null for a block the token doesn't see, an automation or the Templum
  Agent) and its `role` the role it was opened with; both null when none. `schedules` are the deliveries agents asked
  for (`schedule`): `by` the asking block, `to` the block it goes to, `every` in seconds (null: once), `next` and
  `until` in Unix seconds. A workspace-limited token sees the schedules whose `to` block it sees, with `by` null when
  it doesn't see that block, and a task's `by` the same way (`"api"` is always seen).
- A request's `kind` is `question`, `plan`, `connect`, `remove_worktree`, `merge_worktree` or `approval`; `buttons`
  holds a yes/no request's two words (`["Allow","Deny"]`); `scope` is what answering it takes; an approval carries
  `tool`, `said`, `details` and `facts` (Templum's words) and `reason` (the model's).
- A card's attachments give names and sizes, never paths (*the designer's call*).
- `notes` holds the notes a note block shows (a note block has `notes`: `{"tabs": [7, 9], "shown": 7}`), their
  `text` cut at 256 KiB (`cut` true then), each with `rev` (its text's revision: a number that grows each time the text
  changes and at each `edit_note` of it, from the Unix time in ms when Templum started) and `changed` (Unix seconds),
  and the notes no block shows that were last open in a workspace (`closed_in` in `notes.json`), with no text and
  `workspace` that workspace's id (kanban card 204); `checklists` the checklists a checklist block shows (that block has
  `checklist`: the id); `drawings` one item per workspace with ink, its id the workspace's, in paint order as the
  canvas file keeps them, at most 2,000 drawings and 100,000 points (a shape counts 2), the oldest left out past
  either. A limited token sees the notes and checklists its visible blocks show, the notes last closed in its
  workspaces, and the drawings of its workspaces.

### Tools

*The orchestrator's call:* the API is built on the registry the control server already has.

- **`POST /v1/tools/{name}`** with the tool's arguments as the JSON body runs any tool of the registry: every control
  tool ([templum-agent.md](templum-agent.md#tools)) and the API's own below. The answer is
  `{"text":"Created card 40.","workspace":"w3","block":"b7k2m"}` (the tool's text, and what it touched when Templum
  knows it). A refusal is `422` with the tool's sentence. A malformed argument is `400` with the parser's sentence.
- **The API's own tools** (in `templum_api`, served only by the API: automations are the person's, not an agent's,
  automations.md's non-goal stands):

  | Tool | Arguments | Does | Scope |
  |---|---|---|---|
  | `answer_request` | `request`, then one of `answers` (a list per question of the labels picked, and at most one other text), `allow` (true or false), `dismiss` (true); `note` | Answers a request as the inbox's buttons do (an approval's `allow` is Allow once; `note` is a denial's words); `Answered.` | control; full for a worktree's and an approval |
  | `save_automation` | `automation` (as `automations.json` keeps one: `name`, `on`, `workspace`, `when`, `do`, `max_runs`, `cooldown`), `id` (to replace one) | Saves it as the editor's Save does, with the editor's checks and words; a new one keeps its `on` (default on); its id | control; full when its Do runs a command |
  | `set_automation` | `automation`, `on` | Its switch | control |
  | `run_automation` | `automation` | Run now | control |
  | `delete_automation` | `automation` | Deletes it | full |
  | `cancel_schedule` | `schedule` (`s3`) | Cancels a delivery an agent asked for later, as the Automations popup's Cancel does; `Cancelled s3.`; one the token doesn't see is "There's no schedule s3: GET /v1/state lists them." | control |
  | `recent_folders` | `workspace` | The workspace's recent folders (`Space::recent`), newest first, a folder a line, as the Git popup's folder menu lists them; empty when there are none. A token limited to some workspaces gets only those inside their folders, links followed, each as its real path, once (kanban card 203); a mirror of another computer is refused with its sentence | read |
  | `edit_note` | `block`, `note` (a tab of that block), then any of `name` (one line; blank is Untitled), `color` (`red`, `yellow`, `green`, `cyan`, `blue`, `magenta` or `none`), `text` with `rev` | Renames, tints, or replaces the note's whole text (at most 256 KiB) when its `rev` is still `rev` (else "Note 7 changed since: its newer text and rev are in the state."; a note whose text the state cuts refuses a text); the text goes through the note's editor, one Ctrl+Z step, home's caret kept (kanban card 204) | control |
  | `new_note` | `block`, then `name`, `text` | A new note as a shown tab of that note block (`text` at most 256 KiB); `Made note 12 in b7k2m.` | control |
  | `open_note` | `note`, then `block`, or `workspace` with `x`, `y` (both or neither) | Where the note is open, that tab is shown; else a tab of `block`, or a new note block of `workspace`; `Opened note 9 in b7k2m.` A token limited to some workspaces reaches the notes its blocks show and those last closed in its workspaces; any other is "There's no note 9: GET /v1/state lists them." | control |
  | `close_note` | `block`, `note` | Closes that tab as its × does (a blank note goes); `Closed note 7 in b7k2m.` | control |
  | `delete_note` | `note` | Deletes it for good, from every block, as the Notes dialog's trash does; `Deleted note 7 "Plan".` | full |

- **`git`** (Templum to Templum, part 2f; docs/api.md's Git): one of the Git popup's operations in a repository of a
  workspace (`workspace`, `folder`, `op` and its arguments; never raw git arguments), the API's own because the popup
  is the person's. The scope is the operation's: reads (`find`, `look`, `diff`, `history`, `show`, `blame`, `branches`,
  `unmerged`, `default_branch`, `compare`, `stashes`, `stash_diff`, `remotes`, `last_message`, `measure`, `trash`) take
  any token, acts take Control, and what can lose work (`discard`, `discard_hunk`, `restore_version`, `delete_branch`,
  `stash_drop`, `abort`) takes Full; `GET /v1/tools` lists "read, control or full, by op". The registry parses and
  checks the call (`Own::Git`, `templum_git::ops::Op::check`: a malformed argument is `400` before the UI thread); the
  glue finds the repository off the UI thread and, for a token limited to some workspaces, requires its root and its
  main working tree inside their folders (`403`, "That repository lies outside API's folder."); the operations of one
  repository run in turn and each is killed after 10 minutes. The answer is `{"text", "git", "workspace"}`; a `422` of
  `git` adds `details`, git's stderr (at most 64 KiB), to the error. The audit log writes `git:<op>`, and `git:look`
  and `git:diff` are grouped as `terminal_input` is. The server's `Audited` marker in the response carries the name.
- **`move_block`** joins the control server now, as templum-agent.md's batch 3 lists it, with `block`, `x`, `y`, `w`,
  `h` (any of them; the rest keep): a step of that workspace's Ctrl+Z, `–` for the Templum Agent's gate. Its
  `workspace` argument (a move to another workspace) stays for the control server's batch 3 (*the designer's call*).
- **The card tools' grown arguments** (Templum to Templum, part 2d; the block server's too): `create_card` takes
  `labels`; `edit_card` takes `title`, `description` (blank clears it), `labels` (`{"red": true, "blue": false}`) and
  `remove_items`; `move_card` takes `before` (a card of the column; the card goes just before it).
- **The board tools** (Templum to Templum, part 2d) join the control server, `Control` scope: `delete_card` (`card`),
  `edit_board` (`board`, `name` or `delete`), `edit_column` (`board`, `column`, `title`, `before` (a column id or
  `"end"`), `delete`) and `show_board` (`block`, `board` or `new` with `columns`). Each is one step of the kanban's
  Ctrl+Z at home; the board and card arguments are targets, so a limited token reaches only the boards its kanbans
  show (`NOT_FOUND` for the rest). `delete_card`, and a delete of a board or column with cards, ask the Templum
  Agent's person first unless it made them (`AskOwn`, `AskOwnWhenLosing`); through the API they run as you.
- **The checklist tools** (kanban card 205) join the control server, `Control` scope: `edit_checklist` (`block`, then
  `name`, or `item` with `before` (an item id or `"end"`), or both) renames the checklist a checklist block shows or
  moves one of its items, one step of its Ctrl+Z; `new_checklist` (`block`, `name`) shows a new checklist in a
  checklist block that shows none (refused on one that shows one). `block` is their target, so a limited token
  reaches only its workspaces' checklists (`NOT_FOUND` for the rest); `name`, `item` and `before` name nothing.
  Danger `–`.
- **`edit_comment` and `delete_comment`** join the control server (templum-agent.md's table). Through the API they act
  as you: `edit_comment` changes a comment by `you`, `delete_comment` deletes any; the Templum Agent changes only its
  own. `GET /v1/tools` describes them as the control server does.
- **REST aliases** name the common calls (*the designer's call*: each one is a row of the registry's route table that
  names its tool; path parameters and the body become the tool's arguments, query values typed by its schema):

  | Method and path | Tool |
  |---|---|
  | `POST /v1/workspaces` | `create_workspace` |
  | `POST /v1/blocks` | `add_block` |
  | `PATCH /v1/blocks/{block}` | `move_block` |
  | `DELETE /v1/blocks/{block}` | `close_block` |
  | `POST /v1/show` | `show` |
  | `POST /v1/wires` / `DELETE /v1/wires/{a}/{b}` | `connect` / `disconnect` |
  | `POST /v1/agents` | `open_agent` |
  | `POST /v1/agents/{block}/task` | `delegate` |
  | `POST /v1/agents/{block}/interrupt`, `/stop` | `interrupt`, `stop_agent` |
  | `POST /v1/terminals/{block}/text` / `/keys` | `send_to_terminal` / `send_keys` |
  | `POST /v1/requests/{request}/answer` | `answer_request` |
  | `POST /v1/boards/{board}/cards` | `create_card` |
  | `PATCH /v1/cards/{card}`, `POST /v1/cards/{card}/move`, `POST /v1/cards/{card}/comments` | `edit_card`, `move_card`, `comment_card` |
  | `POST /v1/automations`, `PUT /v1/automations/{automation}` | `save_automation` |
  | `PATCH /v1/automations/{automation}`, `POST /v1/automations/{automation}/run`, `DELETE /v1/automations/{automation}` | `set_automation`, `run_automation`, `delete_automation` |

- **Who acts.** A call through the API acts as the person: no approval, comments by `you`. Its tasks (`delegate`,
  `open_agent`'s prompt) are handed by the reserved asker `api` (a terminal id is `<pid>-<n>`, never `api`, as
  `templum-agent` is never one), typed as `[from API · <token name>, task t7]`; their end, a watch's match
  (`wait_for` with `wait: false`) and every other notice Templum would type into an asker's terminal become `notice`
  events instead ([Events](#events)).
- **Tools that wait** (`delegate` with `wait`, `wait_for`) hold the HTTP request up to their timeout (3600
  s at most). A client that goes away drops what it waited for, as an agent's closed connection does. Cloudflare cuts a
  response that sends nothing for 100 seconds: through it, hand the task without `wait` and listen for its `notice`
  (docs/api.md says so).

### Events

`GET /v1/events` (*the orchestrator's call*: SSE, resumable ids, heartbeats; the shape is *the designer's call*):

- `Content-Type: text/event-stream`, `Cache-Control: no-store`, `X-Accel-Buffering: no` (nginx would buffer it).
- **On connect:** `event: snapshot`, the whole state as the token sees it. Then one event per change:

  ```
  id: 8f3a-1042
  event: block
  data: {"op":"put","id":"b7k2m","value":{…the block, whole…}}

  id: 8f3a-1043
  event: request
  data: {"op":"delete","id":"r12"}

  id: 8f3a-1044
  event: notice
  data: {"text":"[task t7 ended …] …","block":"b7k2m","workspace":"w3"}
  ```

- **Event names** are the state's collections, singular: `workspace`, `block`, `wire` (id `a-b`, the two block ids
  sorted), `request`, `board`, `card`, `note`, `checklist`, `drawing` (the workspace's id), `automation`, `schedule` (`s3`), `usage`
  (id `usage`); `put` carries the whole object, `delete` its
  id. A block that moves to another workspace is one `put` with its new `workspace`. Agents' states ride on their
  blocks. A client mirrors the state by applying puts and deletes to the snapshot.
- **`notice`** is the one happening that isn't state: a task the API handed ended or its agent needs its person, a
  watch the API set matched, an automation's Notify me (`{"text":"Morning review: Look","workspace":"w3",
  "automation":1}`, `automation` left out of the others). It goes to every stream whose token sees its block or
  workspace; one of a rule for all workspaces goes to the tokens for all workspaces.
- **Ids** are `<run>-<n>`: `run` is random at each start of the server, `n` counts up. **Resuming** with
  `Last-Event-ID` (EventSource sends it by itself) gets a fresh `snapshot`, then the notices after that id when it is of
  this run and the server still keeps them (the last 256, at most an hour old); from another run, the snapshot alone.
  State is never replayed: the snapshot is the truth.
- **How changes are found:** while at least one stream is open, the UI thread builds the state at most every 250 ms
  and hands it to the server, which compares it, per stream, with what that stream last saw (filtered for its token)
  and sends the differences. Nothing is built while no stream is open (*the designer's call*: one comparison of plain
  data instead of a hook in every feature, so a feature in the state is live without more code).
- **Heartbeat:** a comment line (`: ping`) every 15 seconds, under every proxy's idle timeout (Cloudflare's is 100 s).
- **Backpressure:** each stream has a queue of 256 events. A client that falls behind loses its queue and gets a new
  `snapshot` when it catches up; the server never holds more than that per client.
- **Cut:** revoking a token, its expiry (checked at each heartbeat), Disconnect, turning the API off and Templum
  quitting end its streams.
- **Batch 2's terminal streams** follow the same rules on their own path: [Batches](#batches).

### Errors and limits

| Status | `code` | When |
|---|---|---|
| 400 | `invalid` | Malformed JSON, an unknown argument value, a tool's argument error |
| 401 | `auth` | No token, a wrong, expired or revoked one, a used or stale ticket |
| 403 | `scope` | The token's scope is too low |
| 404 | `not_found` | No such route, tool or resource (a target the token doesn't see is this too) |
| 413 | `too_large` | A body over 1 MiB |
| 415 | `json` | A POST, PUT or PATCH whose `Content-Type` isn't `application/json` |
| 422 | `refused` | The tool refused (its sentence says what to do next) |
| 429 | `slow_down` | Too many failed authentications from the address, or too many calls from the token; `Retry-After` |
| 503 | `busy` | Too many connections, or Templum is quitting |

Every error body is `{"error":{"code":"…","message":"…"}}`. *The designer's call* for the limits: a body of 1 MiB
(`send_to_terminal` and a plan take 256 KiB of text); request headers within 10 s and 16 KiB; 64 connections at once;
8 event streams per token; 20 calls a second per token, bursts of 40 (each call is work on the UI thread).

### Versioning

`/v1` grows only by additions: new tools, resources, fields, event names and error codes. Clients ignore what they
don't know. A change that would break a client is `/v2`, served beside `/v1` for at least one release
(*the designer's call*). `app.api` in the state says the newest version served, `app.version` Templum's.

## Security

*The orchestrator's call* for every requirement below; *the designer's call* for the details named so.

### Off and local by default

- The API is off until Settings ▸ API turns it on, and listens on `127.0.0.1:7411` when it is. No data folder
  (`--bench`, a second Templum without one): no API.
- **Plain HTTP only on loopback.** The server refuses to start in plain HTTP on any other address (a check in the API
  crate, tested), whatever the file says.

### Encryption

- **TLS is built in** with rustls (in the tree) and ring's crypto: TLS 1.2 and 1.3, ALPN `http/1.1`.
- **Templum's certificate:** made at the first need with rcgen, an ECDSA P-256 key, valid five years, for `localhost`,
  `127.0.0.1` and `::1`, kept in `<data>/api/cert.pem` and `key.pem`. Its fingerprint is the SHA-256 of its DER,
  shown as 32 hex pairs, as Node's `fingerprint256` gives it: a client pins it (docs/api.md shows Node and curl; Templum
  to Templum will). **New certificate** replaces both files. *The designer's call:* names don't matter for pinning, and
  a browser warns about any self-signed certificate.
- **The person's certificate:** a PEM certificate chain and a PEM private key, read at each start of the server; a
  file that doesn't load keeps the API off, with the reason in Settings and the title bar.
- No `Strict-Transport-Security`: a browser would pin the host for a year, and Templum's certificate (or a tunnel's)
  changes. (Revised after the security review.)

### Every answer

`X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, `Referrer-Policy: no-referrer`,
`X-Frame-Options: DENY`, `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`,
`Cross-Origin-Resource-Policy: same-origin`.

### Browsers

- **CORS is off** unless the person lists origins. A listed origin (compared whole: scheme, host, port; no `*`, *the
  designer's call*: a wildcard would let any page holding a token call in) gets `Access-Control-Allow-Origin` with its
  own value, `Vary: Origin`, and preflights answered with the methods, `Authorization`, `Content-Type` and
  `Last-Event-ID`, for 10 minutes. No credentials mode: there are no cookies.
- A POST, PUT or PATCH must be `application/json`: a form or a `no-cors` request from a page can't be one.

### Brute force and floods

- **Failed authentications** (credentials shown that weren't good; none shown is not one) are counted per client
  address, an IPv6 /64 as one: after 10 within 10 minutes, the address's failures are answered `429` for 2 seconds, then
  twice as long at each further failure, up to 15 minutes (`Retry-After`). A good token always passes, so no address
  can lock the person out, and a failure's connection is closed. The table is capped at 4096 addresses, swept once a
  second; past the cap the rest share one entry. Connections: 64, 16 per token; off loopback, 8 that have shown no
  token per address (an IPv6 /64; the newest stranger evicts the oldest, also for an address the limiter is
  refusing: its users open connections in parallel); nobody is refused at accept, because the token is looked
  at only once it speaks (a shared public address, a proxy that isn't loopback, the shared overflow entry); a full
  table lets in a new one by evicting the oldest stranger of the address holding the most strangers.
- **The client address** is the connection's peer, unless the peer is loopback (a tunnel, a proxy on this computer):
  then the last entry of `X-Forwarded-For` (the one the proxy added; `CF-Connecting-IP` is not believed, a client can
  send it), else the peer (*the designer's call*: without this, a tunnel makes every client one
  address and an attacker would lock the person out).
- **Floods:** the limits of [Errors and limits](#errors-and-limits).

### Visibility

- The title bar's pill; Settings' clients with Disconnect; each token's last use (time and address).
- **The audit log**, `<data>/api/audit.log`, written by the server: one JSON line per call: time, address, token
  name, what (a tool's name, a resource's path), its status; and, if calls were given up because the disk could not
  keep up, one `lost` line with the count (`more`). Never a token, a ticket, a body or typed text. At 1 MiB it becomes
  `audit.log.1` (replacing the older one) and a new file starts (*the designer's call*: 2 MiB at most).
- **The noise log**, `<data>/api/noise.log`: what anyone can make without a token (connections, refused or failed
  authentications, failed TLS handshakes), one line per address and minute with `more` counting the rest, in a file pair
  of its own (1 MiB, then `noise.log.1`), so a flood of strangers from any number of addresses can never rotate the
  calls out of `audit.log`.

### Persistence

- `<data>/api/api.json` holds the settings and the tokens' hashes, written through a temporary file and a rename, in a
  folder made `0700` with files `0600` on Unix (the certificate's key too). A file Templum can't read is set aside as
  `api.json.bak`, the API stays off, and a notification says so.
- **Why not `config.toml`** (*the designer's call*): it is often kept in a dotfiles repository and synced between
  computers; a listening port, its tokens and its certificate belong to one computer.
- A copy of the data folder carries no token that works: only hashes.

### Local processes

- The bus refuses a hello whose client is `api` (the API's calls are made inside Templum, never on the socket), so an
  agent in a terminal can't borrow the API's reach (*the designer's call*, tested in templum-mcp).
- Tokens are never in a terminal's environment. An agent with a shell reaches `127.0.0.1:7411` like any program, and
  meets `401` without a token.

### Threat model

| Threat | What stops it, or limits it |
|---|---|
| **A leaked token** | Its scope (read can't act), its workspaces, its expiry (30 days by default). Revoke cuts it and its streams at once; Revoke all. Its last use and the audit log show misuse. Never in a URL, so not in logs or history; `tpl_` lets scanners find it in a public repository. |
| **A stolen data folder** (a backup, a synced copy) | No working token: SHA-256 hashes of 256 random bits. The certificate's key is there: an attacker who also sits on the network could pose as this Templum to clients that pinned it: **New certificate** (clients re-pin) and Revoke all. Files are `0600`, the folder `0700`, against other users of the computer. |
| **An exposed LAN port** | Off by default, loopback by default. Off loopback, TLS only. Bearer only, on every route but CORS preflights. Rate limits. The title bar shows it is open. |
| **A malicious web page** in the person's browser | No ambient credential (no cookie), so it can't call in the person's name; JSON only, so no form posts; CORS off unless listed; DNS rebinding gets nothing without a token; tickets single-use, 30 s. Security headers keep an answer from being framed or read as HTML. |
| **Brute force** | 2²⁵⁶ tokens: that is what stops guessing. The per-address growing refusal answers failures sooner and keeps the log quiet; it does not slow guessing (authentication runs first). Tunnel-aware addresses; the audit log. |
| **A client that floods or reads slowly** | 64 connections, header and body limits, 20 calls a second per token, 256 events queued per stream, then a snapshot. The UI thread builds the state 4 times a second at most, only while a stream is open. |
| **Someone on the network between** | TLS off loopback; a tunnel's hop to Templum is loopback; HSTS over TLS; pinning. |
| **An agent on this computer** | The bus refuses `api` hellos; tokens never reach terminals. A Full token pasted into an agent gives it the person's hand: docs/api.md says to keep tokens out of agents. |

What a token is worth: **control** lets its holder make agents act, and an agent in bypass mode runs what it is told.
docs/api.md says so, and recommends **read** for dashboards, Cloudflare Access (or a VPN) in front of a tunnel as a
second lock, and short expiries.

## Constraints

- **Crates** (*the orchestrator's call*): the API is a new UI-free crate, `templum-api` (the server, tokens, TLS, the
  registry and its OpenAPI, the state's types and their comparison, the events, the audit log), tested headlessly with
  real HTTP. `crates/templum` holds only the glue: the state built from the workspaces, the calls served on the UI
  thread, Settings ▸ API and the title bar's pill. `templum-mcp` gains `move_block`, `control::reads`, the bus's refusal
  of `api` hellos and its connection numbers for the API, and the check of an answer to a question.
- **The same path as the control server:** the API's threads hand each call to the UI thread as a bus `Message`
  (`client: "api"`, the token's id as its terminal) through the same channel the bus's threads write to, and wait for
  its line; the UI thread never touches a socket and never waits on the network.
- **Dependencies** (*the orchestrator's call*: in the tree first): hyper 1 (its `server` and `http1` features, new on a
  crate in the tree), hyper-util (`tokio`: its IO adapter and timer), http-body-util, bytes, tokio (`rt`, `net`,
  `time`, `sync`, `io-util`, all already on), tokio-rustls and rustls (ring), rustls-pki-types (its PEM reader), ring
  (SHA-256, random), base64, form_urlencoded, futures, serde, serde_json; ureq in the tests. **New:** `rcgen` with
  `ring` and `pem` (MIT or Apache-2.0), which brings `yasna` (MIT or Apache-2.0) and `pem` (MIT): a certificate is
  ASN.1 that no crate in the tree writes, and writing DER by hand is where certificates go wrong. `cargo deny check`
  passes with them.
- **The server** runs on its own thread with a single-threaded tokio runtime, and its blocking waits for the UI's
  answers on tokio's blocking pool (64 threads at most).
- **Performance:** with the API off, nothing runs; on with no stream, nothing runs but the listener; with streams, one
  state built 4 times a second at most, its comparison on the server's thread. `--bench` and `--bench-mixed` keep
  `p95_ms` ≤ 16.7 (they have no data folder, so no API: the pull request says so and pastes their lines).
- **Tests** (CLAUDE.md): unit tests in `templum-api` and `templum-mcp`; integration tests that start the server on an
  ephemeral loopback port and call it with ureq in plain HTTP and over TLS; headless workspace tests for the glue. None
  depends on HOME, config or installed agent CLIs; none sleeps to synchronize (they wait on conditions, with a
  deadline).
- **Every feature in the API** (*the orchestrator's call*, CLAUDE.md's new rule): from this card on, a feature ships
  its API surface — a tool in the registry, or its data in the state — and its line in docs/api.md, in the same change.
  A feature that is a control tool is in the API by itself; one that is only UI adds its data to the state's types.
- Specs to update as batch 1 lands: templum-agent.md (`move_block` built; the gate's sentence on the API's full
  tokens), connections.md (the bus refuses `api`), settings.md (Settings ▸ API), ask-inbox.md (answered from the API),
  automations.md (the API's tools; the non-goal for agents stays), ARCHITECTURE.md (the crate, `workspace/api.rs`),
  docs/guide.md (the API in a few lines), this file's Status and As built.

## Batches

*The orchestrator's split, kept* (*the designer's call*: each batch is usable on its own; batch 1 alone makes the
phone case work).

- **Batch 1 (the plan):** the crate; tokens, tickets, scopes and workspace limits; settings and their file; TLS with
  Templum's certificate or the person's; the server with its limits, headers, CORS, rate limits and audit log; the
  registry with `POST /v1/tools/{name}`, the aliases, `move_block`, the API's own tools, `GET /v1/tools` and the
  OpenAPI document; the resources; the event stream with snapshots, puts, deletes, notices, resume, heartbeats and
  backpressure; Settings ▸ API and the title bar's pill; docs/api.md.
- **Batch 2: live terminals and a finer canvas.**
  - `GET /v1/terminals/{block}/stream` (SSE, read scope): `event: frame` with the terminal's size, cursor, title and
    rows, each row its text and its style runs (foreground, background as palette indexes or RGB, bold, italic,
    underline, inverse); first the whole screen, then only the rows that changed, at most 20 frames a second. The
    frames are read on the server's thread with the terminal's lock taken by `try_lock` (a held lock skips a frame,
    never waits), so neither the PTY's thread nor painting is slowed; a terminal's `Arc` reaches the server through
    the glue. A client behind gets the newest frame, whole, never a backlog. The scrollback is `read_terminal`'s.
  - `POST /v1/terminals/{block}/input` (full scope): `{"text":…}` as a paste, `{"keys":[…]}` as `send_keys`' names,
    or `{"mouse":[…]}` as the mouse does, encoded at home in the program's mode; no resize (a viewer scales the frame,
    as zoom does).
  - The canvas while it moves: the state's tick goes to 100 ms while a block is dragged or resized, so a mirror follows
    a drag; the shown workspace's camera as an optional `view` event.
  - **Designed** (plan: [2026-10-24-api-batch-2.md](../plans/2026-10-24-api-batch-2.md); each *the designer's call*):
    - **A frame** is `event: frame` with `data` `{"size":{"cols":120,"rows":40},"cursor":{"row":3,"col":10,
      "shape":"block","blinking":false},"title":"vim main.rs","whole":true,"rows":[{"y":0,"runs":[…]}]}`. `cursor` is
      null while the program hides it; `shape` is `block`, `underline` or `beam`; `title` is the program's own (OSC
      0/2), null without one (the block's title is in the state). `whole` frames hold every row; the others only the
      rows that changed, each with its `y`; size, cursor and title are in every frame. A frame is sent when any of it
      changed. Frames carry ids like events, but a terminal stream doesn't resume: it always opens with a whole frame.
    - **A run** carries its own text: `{"text":"main","fg":4,"bg":"#1e1e2e","bold":true}`, so no client counts bytes
      or UTF-16 units. `fg` and `bg` are absent for the theme's default, a number 0–255 for a palette index (the
      viewer's palette draws it, as zoom draws with the viewer's fonts), or `"#rrggbb"`; a palette slot the program
      set (OSC 4, 10, 11) comes as its RGB. `bold`, `dim` (added: agents draw hints dim), `italic`, `underline` (any
      kind) and `inverse` are present only when true. Hidden text (SGR 8) comes as blanks; a wide character's spacer
      cell is left out (the character takes its two columns again where it is drawn); combining marks follow their
      character; trailing blanks with the default background are trimmed.
    - **The frame is the live screen**, never the view the person scrolled back to, and never the selection.
    - **What a mirror needs:** a test redraws a screen, as a frame carries it, into a `Terminal::viewer` (cursor moves,
      SGR, erase-line, DECSCUSR, OSC 2) and reads back the same screen.
    - **Reading:** a terminal counts its changes (`SharedTerm::changes`, bumped wherever its grid is marked dirty), so
      the server's reader, every 50 ms, reads only the terminals that changed, with `try_lock_unfair` (a held lock skips
      that terminal until the next look), and never holds its own table's lock while it reads. With no terminal stream
      open the reader sleeps and the server keeps no terminal.
    - **The terminals reach the server** as a table the glue lends (block, workspace, the terminal's `SharedTerm`):
      when a stream opens (the server asks the UI thread `templum/terminal`, which checks the token's workspaces and the
      block, then lends every terminal), and at each tick while a terminal stream is open. A stream whose block left the
      table (closed) or whose workspace its token no longer sees ends; the client gets `404` when it opens it again.
    - **Backpressure:** a terminal stream queues 20 frames (a second); past that it drops them and its next frame is
      whole, of the newest screen.
    - **Limits:** a terminal stream is a connection, so it counts in the 64 and in its token's 16, and opening one is a
      call (the token's 20 a second). No cap of its own: a mirror streams the terminals it shows.
    - **Errors:** a block that doesn't exist or isn't the token's to see is `404` with the block words; a block that
      isn't a terminal is `422` "k9x1d isn't a terminal.".
    - **Input is the API's own tool `terminal_input`** (`block`, then `text` or `keys`; full scope), so it is in the
      registry, `GET /v1/tools` and the OpenAPI document, and `POST /v1/terminals/{block}/input` is its alias. It is the
      person's keyboard, not an agent's `send_keys`: no task check, no screen read back, its answer as soon as the keys
      are pressed (`Pasted into b7k2m.`, `Pressed 2 keys in b7k2m.`). Keys are 1 to 256 a call, pressed back to back, with 50 ms
      only after a lone Esc (or Ctrl+[, Ctrl+3) that another key follows, so the pair doesn't read as Alt+key; an agent's
      `send_keys` waits between all of its keys. Text is a paste of at most 256 KiB.
      The block shows "answered by API · <token name>" as it does for `send_keys`. A client sends its next input after
      the answer to the last, so keys keep their order. Refused: a block that isn't a terminal, a subagent's transcript
      (read-only), a shell that has ended ("b7k2m's shell has ended: nothing typed reaches it.").
    - **`view`:** the state gains `view`, `{"workspace":"w3","x":…,"y":…,"w":…,"h":…,"zoom":1.0}`: the world rectangle
      the shown workspace's canvas shows (its camera and the canvas's size at the last paint) and the zoom; null for a
      token that doesn't see the shown workspace. `GET /v1/events?view=true` adds `event: view` (its `data` the view,
      or null) each time it changes; other streams don't get it.
    - **The fast tick** (100 ms) also runs while the canvas is panned by the pointer, so a stream that follows the
      view keeps up.

## Templum to Templum

Part 2 of card 171 has its own design: [templum-to-templum.md](templum-to-templum.md). A workspace's Edit… dialog
shares it as a token limited to it and one connection code; the other Templum pastes the code and mirrors the workspace
in a remote workspace of its sidebar, live, through this API (home is the truth). It builds on what batches 1 and 2
give: workspace-limited tokens, the state and its deltas, `move_block`, Templum's certificate and its fingerprint,
terminal frames and `terminal_input`; it adds `GET /v1/me`, shares, a block's `read_only`, a workspace's `icon`, a
frame's `modes` and more key names. Home's side is built (plan
[2026-10-25-t2t-batch-1a-home.md](../plans/2026-10-25-t2t-batch-1a-home.md)): the `share_workspace` and
`revoke_share` tools and their aliases, the connection code (`templum_api::code`), `Templum-Mirror` and
`event: replaced`, and the audit log's grouping of `terminal_input` and `GET /v1/me`. The mirror's client and the
remote workspace are built too (plans 1b to 1d): batch 1 of Templum to Templum is implemented, and nothing of it
needed another route, tool or field than the above. A mirror's own state shows its remote workspace as `remote`, never
its blocks or its home's requests.

## Left to the builder

The pill's and the rows' exact looks (theme tokens only), the dialog's layout, the wording of tooltips not quoted here,
the constants' and functions' names, how the route table is written, the OpenAPI document's descriptions beyond the
tools' own, the audit line's field names, and how the server's tasks are arranged on its runtime.

## As built

### Batch 1

What the builder chose where the design left it open, and what differs:

- **Code:** `crates/templum-api/` (`store.rs` settings, tokens and their file; `tls.rs`; `model.rs` the state and its
  comparison; `registry.rs` and `openapi.rs`; `server.rs`; `events.rs`; `audit.rs`), the glue in
  `crates/templum/src/workspace/{api.rs,api_state.rs}`, `settings/api.rs` and `title_bar.rs`'s pill. The control
  server's `control_call` takes an `Actor` (the Templum Agent, or a token).
- **Failed authentications:** the address's failures are refused from its **tenth** (the plan's unit test said the
  eleventh; the spec's sentence and the integration test say the tenth). A failure made after a refusal ends, while the
  address is still in trouble (within 10 minutes of it), doubles the wait; ten minutes of calm start it over.
- **A POST, PUT or PATCH with a body must say `application/json`;** with no body, no type is needed (the review's
  finding: a body-less `curl -X POST` was `415`).
- **Body limits:** an oversize body is read on to 1 MiB more and dropped so the client can read the `413`; past that
  the connection closes. A body of another type is read too before its `415`, and the connection stays. A body must
  arrive within 30 seconds.
- **Connections past 64** are answered `503 busy` in plain HTTP (64 more at once, then dropped); over TLS they are
  closed. A handshake takes at most 10 seconds.
- **Workspace limits** also cover what the spec lists only by argument name: `save_automation`'s `id`, its
  `automation.workspace` (required for a limited token, and one it sees) and the blocks its `do` types into.
  `for` (a card's assignee) takes `anyone`, an agent's id or a block the token sees. `create_workspace` is `403 scope`.
  The server filters every state it gets for the token as well, whatever the UI thread sent.
- **The streams' waker** is a stored `Waker`, not a `Notify`; a stream ends and leaves the hub at once on revoke, and
  its `Connection: close` ends the connection with it.
- **A closed `delegate` with `wait`** frees the task it handed (`handed` is none); other waits just go, as an agent's
  closed connection drops them.
- **Origins** change on a running server (`Server::set_origins`); any other setting restarts it. `api.json` is written
  at once on the UI thread; a token's last use only once a minute and at quit.
- **Cargo.lock** gains `templum-api`, `rcgen`, `yasna`, `pem`, and, as lock-only entries that are not compiled,
  rcgen's optional `x509-parser` and its dependencies (`asn1-rs`, `der-parser`, `oid-registry`, `nom`…).
- **After the security review (sixteen findings):**
  - A token limited to some workspaces: `overview` is refused; `list_cards` and `create_card` must name a board it sees
    (and the error never lists the others); a workspace or board in an argument is resolved as the tool resolves it
    (an id first, then a name in either case) and the id is what is checked and passed on; `add_block`/`open_agent`
    folders must lie, once canonicalized, inside a folder of its workspaces (so must an automation's); a deleted
    workspace's id leaves every token's list.
  - `move_block` clamps the size to the biggest grid a terminal may have and bounds x and y to ±10⁷.
  - Setting, running or replacing a command automation takes a Full token.
  - Authentication comes first and the limiter slows only failures, keyed on the last `X-Forwarded-For` of a loopback
    peer (`CF-Connecting-IP` is not believed), IPv6 by /64, 4096 addresses at most, swept once a second; a failure's
    connection closes; 16 connections per token; a full table evicts the oldest stranger.
  - The audit log is written by a thread of its own and aggregates connection and authentication noise per address
    and minute (`more` counts the rest); HSTS is gone.

- **After the second security review (nine findings):**
  - An automation a limited token saves: `do.agent.folder` and `do.command.folder` (and `folder` of `add_block` and
    `open_agent`) are written back as the canonical path that was checked, so what is stored and run is what was
    checked (`"."` is the workspace's absolute folder); with no folder, a workspace without one is refused. The store
    keeps, per automation, the real folders of the token's workspaces (`confined` in `api.json`); each run checks the
    folder it opens in against them and is Skipped "Target missing: …" when a link was swapped in. The person's own
    save in the editor, or deleting it, lifts that. A limited token's `worktree` on `open_agent` needs the
    repository's main tree to lie inside one of its workspaces' folders.
  - Connections: off loopback, 8 that showed no token per address (an IPv6 /64); a full table evicts the oldest
    stranger of the address holding the most. (A peer the limiter refuses was refused at accept here; the third review
    undid that.)
  - The audit log counts noise in `Audit::write`, before the queue (a table capped at 4096, keyed like the limiter),
    queues only a key's first line and writes its count when the window ends (looked at once a second at most); a
    call line waited up to 200 ms for room in a full queue instead of being dropped. (Never waits now: see the third
    review.)
  - An open stream ends when its token's workspace list is no longer the one it opened with (the sweep compares
    them); the client opens it again.
  - `flush_api` (quit) drains the uses the server still holds before writing.
  - Settings ▸ API: a field is taken on Enter or Blur; a value its setter refuses (a port outside 1024–65535) is
    replaced in the field by the setting, with the reason under it.
  - Streams: an expired token's end at the next publish; the state is compared only when it changed or a stream opened,
    once per kind of caller. A client coming or going is told to the UI (`templum/clients`), which keeps the pill's
    count. A body-less POST needs no Content-Type. The address, port and certificate fields take Enter or leaving the
    field; an edit that changes nothing restarts nothing.
- **After the third security review (nine findings):**
  - Nobody is refused at accept: the limiter only slows failed authentications, so a good token from a shared address
    (CGNAT, an office, a VPN exit, a proxy that isn't loopback) gets in. (The third review also held a refused address
    to one stranger; the fourth undid that, see below.)
  - `api.json` keeps `saved_by` (automation → the id of the token that saved it) beside `confined`, for every rule
    saved through the API. Revoke, Revoke all and expiry (swept at each snapshot, every 5 s) turn those rules off and
    notify "Automation <name>, saved by token '<token name>', was turned off."; the person's own save or delete clears
    `saved_by` as it clears `confined`.
  - The audit writer keeps its files open (reopened after a rotation), collects the noise lines due under the lock and
    writes them after releasing it, and a call line never waits: noise may fill only half of the queue (a counter the
    writer decrements), a call that finds the queue full is counted as `lost` and the writer logs it as one line.
    `Drop` closes the channel (the writer drains it and ends) and waits about a second for a done signal instead of
    joining without a limit. Noise has its own file pair, `noise.log` (1 MiB + one old file); `audit.log` holds only
    lines with a token (and the `lost` line).
  - The canonical path a rule keeps and a folder is opened in is written back in the simplified form (`dunce`):
    `C:\x`, not `\\?\C:\x`, which cmd.exe refuses as a folder. Paths are compared canonical; only the form written
    back changes.
  - A limited token's `worktree` is checked in the background task, before `add_worktree`, against the main tree *git*
    reports (`Repo::main_tree`, which obeys `commondir` and `core.worktree`); the workspace no longer reads `.git`
    files itself, so a planted one can't move the worktree outside the token's folders.
  - On Windows a folder on `UNC`, `VerbatimUNC` or `DeviceNS` is refused before the filesystem is asked about it
    (the SMB session would offer the person's NTLM hash), unless one of the token's workspace folders is on that
    share (`refuses_share`, over `std::path::Prefix`, tested on every platform).
  - `start_api` drains the old server's uses first.
  - **Folder limits are not a sandbox.** They guard what a token can *name* (where it opens a terminal, an editor, an
    agent or a worktree). A process the token starts inside a folder has the person's reach.
- **After the fourth security review (ten findings):**
  - Connections: the cap of 8 strangers per address is the same for an address the limiter refuses: its users open
    connections in parallel (a dashboard, a proxy on a docker bridge, CGNAT), and a cap of one had them abort each
    other. "A good token always gets in" holds against guessing; whoever shares the address and floods connections
    can still starve the connections of that address that haven't authenticated yet.
  - The credentials are checked and the connection marked as authenticated under one lock, so an accept in between
    can't evict it as a stranger. (No test: it needs an accept interleaved with a request; the lock is the proof.)
  - On Windows `share_of` fails closed: only `Disk` and `VerbatimDisk` are the computer's own. `std` matches `\\?\UNC\`
    case-sensitively, so `\\?\unc\x\y` and `\\?\GLOBALROOT\Device\Mup\…` come as a plain `Verbatim` and reach an
    SMB share all the same; a `Verbatim` prefix is refused unless a folder of the token is on the same one. Tested on
    every platform over `Prefix` values.
  - The audit log cuts `what` to 200 characters (a `/v1/tools/{name}` name was the caller's, up to 16 KiB). Noise is
    counted by (address, what, token): a `slow_down` carries a token, and another token's was added to the first's line.
  - A rule saved by a token that is revoked or expired is Skipped "Target missing: its token was revoked" (in
    `automation_target`) until the sweep turns it off; one the token had already turned off is told of ("…stays off.")
    instead of dropped silently.
  - `confined` and `saved_by` entries for automations that don't exist are dropped at load (the ids restart when
    `automations.json` was set aside), unless the automations couldn't be read.
  - A limited token's worktree outside its folders is `403` `scope`, as any other folder.
- **Merged with Automations batch 2 (card 150):** an event rule's `cooldown` is in the state (null for a time rule),
  and the API saves event rules (`when.agent`, `when.command`) as the file keeps them.
- **Unfinished, in [minors.md](../minors.md#api):** an approval's `since` is when it first showed, and a changed
  certificate on disk is read at the next start.

### Batch 2

What the builder chose where the design left it open, and what differs:

- **Code:** `crates/templum-terminal/src/screen.rs` (`SharedTerm::screen`, `changes`, `same`; `Screen`, `Styled`,
  `Paint`), `crates/templum-api/src/terminals.rs` (the frame, the table of lent terminals, the reader),
  `server.rs` (the route, tickets, `Server::{set_terminals, terminal_streams}`, the reader task), `events.rs` (a queue
  of its own size, the `view` event), `registry.rs` (`terminal_input`), and in the glue `workspace/api.rs`
  (`api_terminals`, `api_terminal`, `api_input`, `api_tick`) and `api_state.rs` (`view`). `Pane`'s keys loop is
  `press_each` (`delegate.rs`), shared by `send_keys` and `terminal_input`.
- **The server keeps only the terminals a stream names** (not every terminal the glue lends): a block's grid is held
  from its first stream's opening to its last stream's end, and a closed block's is let go at the tick after it.
- **A held lock skips that terminal for the round** (nothing is recorded, so the next look reads it); a stream that
  is behind, or just opened on a terminal that hasn't changed, is sent the stored screen whole without a read.
- **A behind stream is resynced by the next look**, so a client never sees `behind` for more than a look (50 ms): its
  queue holds the one whole frame when it next reads.
- **`terminal_input` is parsed by the server too** (every own tool is, now): a call the UI thread would refuse for its
  arguments (`{}`, an unknown key, 17 keys) is `400` and never reaches it.
- **`view` is null before the first paint** (the canvas has no size yet); the server filters it for a limited token as
  it filters the rest of the state; it is null in grid mode too (the camera stays put there, the canvas shows tiles).
  The tick is also 100 ms while the canvas is panned by the pointer, and for 250 ms after the camera moves any other
  way (touchpad pan, wheel or pinch zoom, a flight).
- **The `view` event is sent after the deltas of the same publish**, once per change and per kind of caller (they
  share the frame); a stream that is behind gets it in its snapshot.
- **A subagent's transcript is lent like any terminal.** `api_terminals` (`workspace/api.rs`) lends every terminal
  block, so a stream of a Claude Code subagent's read-only block works (it shows the transcript, as `read_terminal`
  does); only input refuses it. A mirror can't tell them apart: the state's `kind` says `terminal` for both.
- **A terminal stream's queue is bounded in bytes too:** 20 frames or 8 MiB, whichever comes first; past either the
  stream is behind and the next look sends one whole frame.
- **A queue ignores what is pushed after its stream ended**, so a late frame can't outlive a cut stream.
- **Verified:** the integration tests run the real server over HTTP (`tests/terminals.rs`, `tests/events.rs`), the glue's
  tests run it against a headless GPUI workspace (`workspace/tests/api.rs`). The curl lines of `docs/api.md` were not
  tried against a window of the dev build (the builder had no display to turn the API on in).
