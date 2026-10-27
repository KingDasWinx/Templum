# Templum to Templum: a workspace of another computer, as if it were here

- **Date:** 2026-10-25
- **Status:** Batch 1 implemented (kanban card 171, part 2; plans 2026-10-25-t2t-batch-1a…1d: home's side; the
  mirror's client; the remote workspace on the canvas; acting from the mirror, Edit…, the end-to-end test and
  `--bench-mirror`). Batch 2 implemented (parts 2a–2h; plans 2026-10-25-t2t-batch-2a-footing,
  2026-10-25-t2t-batch-2b-canvas, 2026-10-25-t2t-batch-2c-terminals, 2026-10-25-t2t-batch-2d-boards,
  2026-10-25-t2t-batch-2e-checklists-notes-ink, 2026-10-25-t2t-batch-2f-git-home,
  2026-10-25-t2t-batch-2g-git-mirror and 2026-10-26-t2t-batch-2h-agents-automations-inbox)
  ([Batch 2](#batch-2-the-whole-workspace-live)). Since then: grid mode (kanban card 206), home's shells in
  Add ▸ Terminal ▸ (kanban card 201), home's recent folders in the Git popup's folder menu (kanban card 203), notes
  made, edited, renamed, colored, reopened and deleted from the mirror (kanban card 204, plan
  2026-10-26-mirror-notes: [Notes on a mirror](#notes-on-a-mirror-kanban-card-204)). Later batches (see [Later batches](#later-batches)) are
  sketched.
- **Builds on:** [api.md](api.md) (the API: tokens, scopes, workspace-limited tokens, TLS and the fingerprint, the
  state and its event stream, terminal frames, `terminal_input`), [workspaces.md](workspaces.md) and
  [cockpit.md](cockpit.md#6-the-workspaces-screen) (the sidebar, the Edit… dialog), [ask-inbox.md](ask-inbox.md)
  (requests), [connections.md](connections.md) (block ids, the bus), [shell-integration.md](shell-integration.md)
- **For people:** [docs/api.md](../api.md#templum-to-templum).

The person said, for this card: "Pode decidir, fomente bem a ideia para ser uma funcionalidade bem boa … não precisa
ficar perguntando para mim, sobre segurança tome medidas cabíveis para ser seguro e criptografado as conexões." (Decide,
grow the idea into a really good feature, don't ask; on security, take the proper measures: safe and encrypted.) The
orchestrator's calls are marked *the orchestrator's call*; everything else the designer settled is marked *the
designer's call*.

## Intent

The person has a personal computer and a work computer. At the office they want to keep working on what runs at home,
"good enough that the user on the other connection feels as if the project were local". Templum only opens a port; the
person exposes it (a domain, a VPN, a cloudflared tunnel, their own case). There is no Templum server.

Part 1 built everything a second Templum needs on home's side: workspace-limited tokens, a state complete enough to
draw, its live deltas, terminal frames and typed input. Part 2 is the two ends that use it:

- **Share**, on home: a workspace's Edit… dialog makes a token limited to that workspace and shows one **connection
  code** to paste on the other computer.
- **Connect**, on the other computer: the code becomes a **remote workspace** in the sidebar. It mirrors home's
  workspace: its blocks where they are, its terminals live, its agents' states, its requests. Typing, moving a block and
  answering a request go to home through the API; the mirror draws what home says next. Nothing of home's runs on the
  mirror (*the orchestrator's call*: **home is the truth**).

## Goals

1. **The person's case works in batch 1:** from the work computer, through a cloudflared tunnel, open home's workspace,
   see it live, type into its terminals with a real keyboard, move and resize its blocks, answer its agents' requests.
2. **Two clicks on each side.** Home: Edit… ▸ Share… ▸ Make code ▸ Copy. Mirror: Connect to a Templum… ▸ paste ▸
   Connect.
3. **Encrypted, and checked.** Always TLS off loopback. A direct connection pins Templum's certificate; a tunnel's is
   checked against the web's authorities, and a network that reads HTTPS (a company proxy) is told plainly.
4. **The key is kept safely.** The token lives in the system's keyring, never in a log, `config.toml`, the data folder
   (unless the person chooses a file) or a crash.
5. **Honest when it isn't live.** A remote that drops dims and freezes, says it is reconnecting and reconnects by
   itself; one that is cut says why and what to do; the round trip shows.
6. **Feels local where it can, says so where it can't.** What a mirror doesn't do yet is shown, never silently broken.
7. **Every feature in the API** (*the orchestrator's call*, CLAUDE.md): shares are in home's API state and tools; a
   remote workspace is in the mirror's state.

## Non-goals

- A server, relay or rendezvous Templum runs; NAT traversal; discovery on the LAN (*the orchestrator's call*: Templum
  only opens ports).
- Running anything of home's on the mirror, or syncing files, Git or editors' files between the two.
- A mirror acting as a relay: the mirror's agents, block server, control server and API never reach home through it
  (*the designer's call*: a chain of tokens is a hole).
- Two-way sync, merging or offline edits: the mirror has no copy of its own to reconcile.
- Sharing several workspaces with one code; sharing between two people (the same person's two computers is the case).
- A mobile client (part 1's API is the phone's way in).
- End-to-end encryption *through* a tunnel (see [Later batches](#later-batches)).

## Anti-goals (a failure even if it works)

- A token readable in the mirror's data folder, a log, `config.toml`, a crash, a URL or a process's environment.
- A connection that silently trusts a certificate it wasn't told to trust, or follows a redirect with the token.
- The mirror's UI thread waiting on the network or the keyring; painting slowed by a remote.
- Memory that grows with what home sends (a flood of frames, a huge state, a slow UI).
- Keys typed while offline delivered later, blind.
- A block of the remote workspace written into the mirror's notes, boards, checklists or canvas files.
- Something on the mirror that looks like it works and does nothing at home.
- A revoked share that keeps showing home live.
- *(batch 2)* **A mirror that schedules, runs or re-runs any of home's automations,** or that offers a remote
  workspace to its own automations ([the rule](#the-rule-homes-automations-run-only-at-home)).
- *(batch 2)* **Home's content reaching the mirror's own agents, API or stores:** a remote note, card, diff or screen
  sent to a local terminal, given to the mirror's commit-message agent, or pasted into a local workspace.
- *(batch 2)* **A Git operation that takes arbitrary arguments,** or that reaches a repository outside the share's
  workspace.
- *(batch 2)* **An act that seems done on the mirror and isn't at home:** every act shows home's answer (a refusal in
  its words); what the mirror shows ahead of home (a hold) lasts 2 s at most.
- *(batch 2)* **A popup that waits on home:** every call is answered through a channel the UI drains, never awaited on
  the UI thread.

## Words

- **Home:** the Templum whose workspace is shared. **The mirror:** the other Templum, showing it.
- **Share:** a token limited to one workspace, Control or Full, with an expiry, made from that workspace's Edit…
  dialog. It is an API token like any other, with its share's details beside it.
- **Connection code:** one line holding the address, the token and (direct only) the certificate's fingerprint. Shown
  once, like a token.
- **Remote workspace:** the mirror's sidebar tile and canvas for a share.
- **Way in:** **pinned** (direct: LAN, VPN, Tailscale; the mirror pins Templum's certificate) or **public** (a tunnel
  or a proxy with a public certificate; the mirror checks the web's authorities).

## What a person sees

### On home: sharing a workspace

**The Edit… dialog** gains a **Sharing** part below Folder (*the orchestrator's call*: share lives in the workspace's
Edit… dialog). Unlike the rest of the dialog, what it does applies at once, not on Save (*the designer's call*: a
revoke must not wait for Save).

- With no share: "Let another Templum mirror this workspace: watch it live, move its blocks, answer its agents, type
  into its terminals." and **Share…**.
- One row per share: its name; under it, dim, "Full" or "Control", "expires in 29 days" / "never expires" / "expired
  2 days ago" (yellow), and "mirrored now from 203.0.113.7" (with the accent dot) / "last used 3 h ago from
  203.0.113.7" / "never used"; **Revoke** on the right, at once, no question (a share is cheap to make again). Revoking
  ends its streams, so the mirror goes dark at once.

**Share…** opens a dialog (*the designer's call* for its fields):

| Field | Control |
|---|---|
| Name | One line, required, at most 60 characters, default "Another computer" |
| Can | **Full** (default): "Also type and click into terminals, discard Git changes, delete branches, stashes and automations. Anyone with the code can run commands on this computer." **Control:** "Watch and work with blocks, cards, checklists, Git and automations, open agents and hand them tasks, answer requests. No typing into terminals; agents it opens can run commands." (Words since batch 2a.) |
| Expires | In 1 day, 7 days, **30 days** (default), 90 days, 1 year, Never |
| The other computer reaches this one | **Through a tunnel or proxy** (cloudflared, Caddy, nginx, your own certificate), with **Address**: `https://templum.example.com` (an optional path prefix is kept), remembered for the next share. **Directly** (same network, VPN, Tailscale), with **Address**: host and port, prefilled with this computer's address on its main network and the API's port, `192.168.1.5:7411` (a Tailscale name may replace it) |

- The default way in follows Settings ▸ API: Templum's certificate on a network other than loopback picks Directly;
  anything else picks Through a tunnel.
- **The API must fit the way chosen**, and the dialog says so with one button that fixes it (*the designer's call*: a
  share must work when its code is pasted):
  - API off: "Sharing needs the API. **Turn it on** (this computer only, port 7411)." for a tunnel; for Directly,
    "**Turn it on** for every network, with Templum's certificate".
  - Directly while the API listens only on this computer, or without Templum's certificate: "Directly needs the API on
    every network with Templum's certificate. **Apply**".
  - Through a tunnel with an address that isn't `https://` (or `http://` to `127.0.0.1`, `::1` or `localhost`, for an
    `ssh -L` forward): "The address must start with https://."
- **Make code** makes the token and shows the code once, whole, in the terminal's font, with **Copy**, and: "Paste it in
  the other Templum: sidebar ▸ Connect to a Templum… Templum keeps only a fingerprint of its key: this code can't be
  shown again." **Done** closes it; the text is gone from memory.

**The tile** of a workspace that is mirrored now (a share's event stream is open) gets a small cast mark at its
bottom-right; tooltip "Mirrored now on Another computer (203.0.113.7)". The Home's card shows the same
(*the designer's call*: the person always sees who is watching).

**Elsewhere on home:**

- Settings ▸ API lists a share's token with the other tokens, its line saying "share of API"; Revoke there is the same.
- **New certificate** in Settings ▸ API also says how many direct shares it breaks: "Clients that pinned the old one
  stop connecting, 2 mirrors connected directly among them: share again for them."
- **Deleting a workspace** leaves its shares' tokens seeing nothing (part 1 already takes the id out of every token's
  list, so a later workspace given the same id is never theirs), and the mirror says "deleted" from `GET /v1/me`
  (*the designer's call*: revoking them would turn every mirror's message into "revoked", since part 1 answers every
  `401` alike). Settings ▸ API lists them as "share of a deleted workspace" until they are revoked there, expire, or
  the mirror's Forget revokes them.
- What a mirror does looks as part 1 says an API call does: typed keys show "answered by API · Another computer", a
  task handed is `[from API · Another computer, task t7]`.

### On the mirror: connecting

The sidebar gets a **Connect to a Templum…** button under **+** (a plug icon, tooltip of the same words; *the
designer's call*: visible, where the person looks for workspaces). The Home's header gets the same button
beside New workspace, and the keymap a `connect_templum` command with no default key.

**The Connect dialog:**

- **Connection code:** a field, focused. As soon as it holds a code, a line under it says what it is:
  "templum.example.com · checks its public certificate" or "192.168.1.5:7411 · pins Templum's certificate
  AB:CD:…:9F", or what is wrong: "This isn't a connection code: one starts with tplc1_.", "The code is cut short: copy
  it again whole.", "This code is from a newer Templum: update this one.", "This code's address isn't encrypted:
  Templum connects in plain HTTP only to this computer itself."
- **More…** (folded): **Cloudflare Access service token**, Client ID and Client Secret, for a tunnel with Cloudflare
  Access in front (docs/api.md recommends it): every request carries them (*the designer's call*: the second lock the
  docs recommend must not lock the mirror out). **Proxy**, for a network that only reaches out through one
  ([The client](#the-client)).
- Under the field, where the key goes: "The key is kept in this computer's keyring (Keychain / Credential Manager / the
  Secret Service)." With no keyring that answers: "This computer has no keyring Templum can use: the connection lasts
  until Templum quits." and a switch, off: **Keep the key in a file only you can read** (`0600` in the data folder),
  with "A copy of the data folder would carry it." ([Where the key is kept](#where-the-key-is-kept)).
- **Connect** tries it: "Connecting…"; on success the dialog closes and the remote workspace opens; a failure stays in
  the dialog in the words of [When it isn't live](#when-it-isnt-live). The code leaves the field's memory either way, and
  if the clipboard still holds it after a success, Templum clears it (*the designer's call*).

### The remote workspace

- **Its tile:** home's color and icon (or initials), and a small remote mark at its bottom-right. Tooltip:
  "API, on winx-desktop · 84 ms". It joins the sidebar's order (dragged like any tile), Ctrl+Tab, Ctrl+1–9 and the
  Home (its card: "Mirror of winx-desktop · live · 3 terminals" and "1 Claude Code working").
- **A banner** over the canvas's top edge, window chrome at a fixed scale like the Templum Agent's pill: "Mirror of
  **API** on **winx-desktop** · Full · 84 ms", an ⓘ that lists what isn't mirrored yet, and **Disconnect**. The round
  trip turns yellow past 250 ms, with the tooltip "Each key waits about 300 ms for winx-desktop's answer." Three days
  before the share expires: "· ends in 3 days: share again on winx-desktop".
- **Its canvas:** home's blocks at home's rectangles, back to front, with home's titles, agents' dots and chips (state,
  model, context), and the wires between them. The camera is the mirror's own: the first time, it shows what home's
  canvas shows when home shows that workspace (the state's `view`), else it fits the blocks; then it stays where the
  person leaves it, kept across restarts. Zoom, levels of detail, snapping and focus are local, as everywhere.
- **Terminals** show home's live screen, drawn by a viewer, in the mirror's own fonts and colors (home's palette
  numbers take the mirror's palette, as part 1 says). Home's whole grid fits the block. Selecting and copying text,
  and find, work on the live screen.
  - A Claude Code subagent's transcript says "Read-only: a subagent's transcript" in its footer.
  - With a Control share, the first key typed into a terminal shows "Typing needs a Full share: share again on
    winx-desktop." once per terminal.
  - Scrolling back isn't mirrored in batch 1: the wheel over a remote terminal shows, once, "Only the live screen is
    mirrored for now."
- **Typing** in a focused remote terminal is the mirror's keyboard on home's terminal: every key and combination,
  F-keys, Alt and Ctrl combos, arrows with modifiers, Home, End, PageUp, PageDown, Insert, Delete, dead keys and IME
  text; Paste is a paste. Templum's own shortcuts stay the mirror's, and those that yield to programs yield as they
  would at home (the frame says when a program took over).
- **Moving and resizing** a block works as locally, with snapping and Ctrl+Z; home moves its block and the mirror keeps
  following home.
- **Fullscreen** (grid mode: the shortcut, the button beside ✕, the block menu's item) works as locally, enabled with
  its usual tooltip. It is the mirror's own view of home's blocks: it calls nothing at home, and home's blocks keep
  their places there. A tile home moves goes back to home's place when the grid ends, one home closes gives its place
  to its neighbour, a block added from a tile's menu takes half of that tile, and the index keeps the canvas's camera.
  Like the rest of a mirror's canvas, it isn't kept across a restart.
- **Requests:** the Requests button counts the remote workspace's requests while it is shown; the inbox lists them
  and answers them (a question's options, a plan's Approve or Reject, Allow or Deny). One that arrives plays the sound
  and shows the notification a local one would, titled "API on winx-desktop".
- **Blocks not mirrored yet** (notes, checklists, editors, boards in batch 1): a card in the block's place, with its
  kind's icon, its title and "Not mirrored yet: open it on winx-desktop." Ink and shapes aren't shown yet; the ⓘ says so.
- **Not on a remote workspace yet** (each disabled where it shows, with the tooltip "Not on a mirrored workspace
  yet"): adding blocks (toolbar, menus, drops), closing blocks, wiring, the drawing tools, Move to
  workspace, Copy and Paste of blocks, Stop, the Git popup.
- **Right-click the tile:** Open, Edit…, Disconnect (or Connect), Forget…. **Edit…** shows the connection (address,
  way in, the pin, what the share can do, when it expires, the round trip), **Paste a new code…**, and the local look
  (icon, color); the name is home's ("named on winx-desktop"). **Forget…** asks "Forget API on winx-desktop?" with
  **Also revoke the share on winx-desktop** (on while connected): it revokes the share with its own token, deletes the
  key and removes the tile.

### When it isn't live

*The orchestrator's call:* a remote that drops dims its tile and freezes its blocks; it reconnects with backoff and
`Last-Event-ID`. The words and which states retry are *the designer's call*.

While it isn't live, the tile is dimmed, the canvas keeps its last blocks and frames under a dim veil, the agents' dots
go grey, and typing shows "Not connected: what you type isn't sent." (keys are dropped, never queued). Moving and
answering are disabled.

| What happened | The banner says | Retries by itself |
|---|---|---|
| Home doesn't answer: refused, timed out, no route, a name that doesn't resolve, or the tunnel's or the proxy's 502, 503, 504, 530 | "Reconnecting to winx-desktop… try 3, next in 8 s. Live 2 min ago." **Try now** | Yes |
| A stream went silent for 45 s (three missed heartbeats) | The same | Yes |
| 429 or 503 `busy` | The same, after `Retry-After` | Yes |
| 401, and the share's expiry has passed | "This share of API expired. Share again on winx-desktop, then **Paste a new code…**" · **Forget** | No |
| 401 otherwise | "winx-desktop revoked this share. Share again there, then **Paste a new code…**" · **Forget** | No |
| `GET /v1/me` lists no workspace (it was deleted at home) | "API was deleted on winx-desktop." · **Forget** | No |
| The workspace is stopped at home (`stopped`, no blocks) | Not dimmed: a card in the canvas's middle, "API is stopped on winx-desktop." **Start it** (`show`, which starts it and shows it on winx-desktop's screen; the button's tooltip says so) | — |
| `event: replaced`: another computer connected with this share's code | "Another computer connected with this share's code, so this one stopped. Each computer needs its own share." **Connect here again** · **Forget** | No |
| Pinned, and the certificate's fingerprint differs | "winx-desktop's certificate changed since you connected. If you made a New certificate there, share again and paste the new code. If you didn't, someone may be between the two computers: don't connect." **Paste a new code…** · **Forget** | No |
| Public, and the certificate isn't valid | "templum.example.com's certificate isn't valid: it expired / it is for another name / no authority this computer trusts signed it. Fix it where the tunnel or proxy runs, then **Try again**." A self-signed one adds: "If it is Templum's own, share Directly instead: it is pinned." | No |
| Public, valid only through an authority this computer added | [A private authority](#a-private-authority) | No |
| Cloudflare Access answered (a redirect to `*.cloudflareaccess.com`, or its 403) | "Cloudflare Access stands in front of templum.example.com. Add a service token in Paste a new code… ▸ More…, and allow it in the Access policy." | No |
| Something that isn't a Templum answered (HTML, a 404 without Templum's error body) | "templum.example.com didn't answer as a Templum (HTTP 404, text/html). Check that the tunnel points at port 7411." **Try again** | No |
| Home is too old (Templum's 404 for `GET /v1/me`) | "winx-desktop runs a Templum that can't be mirrored: update it." **Try again** | No |
| The proxy refused (anything but its 502, 503, 504, which are "home doesn't answer") | "The proxy proxy.corp:8080 refused the connection (407: it asks for a login Templum can't give)." **Try again** | No |
| The keyring refused the key (locked, denied) | "Templum couldn't read this connection's key from the keyring. **Try again**, or **Paste a new code…**" | No |

- **Backoff:** 1 s, doubling to 30 s, each wait ±20%; back to 1 s once a connection has been live for a minute.
  **Try now** retries at once (*the designer's call*).
- **Resuming:** the event stream reopens with `Last-Event-ID`; home sends a fresh snapshot (part 1's rule), which
  replaces the mirror's blocks; terminal streams reopen whole.
- **Starting Templum** connects every remote that has its key, off the UI thread; until then the tile shows
  "Connecting…" and the canvas is empty.
- **Disconnect** closes every connection and keeps the remote, dimmed: "Disconnected. **Connect**".

## The shape

### The connection code

*The designer's call* (the orchestrator left its format open):

- `tplc1_` followed by the base64url (no padding) of a JSON object:
  `{"url":"https://templum.example.com","token":"tpl_…","pin":"ab12…"}`. `pin` (the certificate's SHA-256, 64 hex
  characters, no colons) is there only for a pinned share. `url` is the address with its scheme, port and optional
  path prefix; the mirror calls `<url>/v1/…`.
- `1` is the format's version: a mirror that meets `tplc2_` says the code is from a newer Templum. Unknown JSON fields
  are ignored. The prefix lets a secret scanner be taught to find a leaked code, as `tpl_` does for tokens.
- The way in is read from the code, never guessed: **a pin means pinned; `https://` without one means public;
  `http://` is accepted only for a loopback host**. The mirror never falls back from one to another.
- No names in the code: the mirror reads home's computer name and workspace name from home once connected, so a code
  can't claim to be someone else's.

### Trust: how the mirror checks who answers

*The orchestrator's call:* always TLS unless loopback; two trust modes. How the mirror tells them apart and what each
checks is *the designer's call*.

| Way in | TLS check | Server name (SNI) |
|---|---|---|
| **Pinned** | The leaf certificate's SHA-256 equals the pin, compared in constant time; its handshake signature is verified with ring's algorithms. Names and dates are not looked at: Templum's certificate is made for `localhost` and lasts five years, and the pin is its identity | The host when it is a name; none for an IP |
| **Public** | The web PKI as the system checks it (rustls-platform-verifier): chain, name, dates, by Windows' and macOS's own verifiers (which fetch a root Windows hasn't loaded yet, and obey the computer's policies) and on Linux with the system's roots (rustls-native-certs). Then the [private authority](#a-private-authority) check | The host |
| **Loopback** (`http://127.0.0.1`, `::1`, `localhost`) | None: plain HTTP to this computer, for an `ssh -L` forward or a test | — |

- TLS 1.2 and 1.3, ALPN `http/1.1`, ring's crypto, as the server.
- **No redirects are followed**, and the token, the Access headers and every call go only to the code's host: a
  redirect is "didn't answer as a Templum".
- **The way in must fit the address, checked before anything is dialed:** Loopback only with plain `http://` and a
  host that is this computer (`127.0.0.1`, `::1`, `localhost`); Pinned and Public only with `https://`. Anything else
  never sends the key: the remote is cut as "didn't answer as a Templum" (no address it can use). So a rewritten
  index can't make the key travel in clear off this computer, nor to a "TLS" nobody checks.
- **A changed pin is never accepted by a click.** The way out is a new code, which carries a new token too (*the
  designer's call*: a "trust the new certificate" button would undo pinning exactly when it matters).

#### A private authority

A work computer may sit behind a company proxy that reads HTTPS: it installs its own authority in the system's roots
and signs a certificate for every site. Through it, the token and every frame of home's terminals pass in the clear
to the company, and a Full token would let it run commands at home. The person's case is exactly a work computer.

So after the system accepts a public certificate, the mirror checks the chain the server sent again with Mozilla's
list (webpki-roots, already built for the updater). If only the system accepts it, the connection is refused with:
"HTTPS to templum.example.com is signed by an authority this computer added, not a public one. A company proxy or
security program may be reading this connection: your key, what home's terminals show and what you type. With a Full
share it could run commands on winx-desktop. Connect only if you added that authority yourself (a home lab, your own
proxy)." **Don't connect** (default) · **Connect anyway**. Connect anyway is kept for that remote only
(`trust_added_authority`) and shown in its Edit…; a new code asks again (*the designer's call*: the system's check
stays the one the orchestrator chose, and the person learns what it means before trusting it). A server that sends
its chain without the intermediate certificate lands here too (the system may fetch the missing one, Mozilla's check
doesn't); cloudflared's edge, Caddy and nginx with a full chain send it whole, and the message's tooltip says "or the
server doesn't send its whole chain".

### The client

*The designer's call* for all of it; the orchestrator asked to prefer what the tree has.

- **One thread for every remote**, with a single-threaded tokio runtime, as the server has: connections, streams,
  calls, timers, backoff and the keyring's calls (on its blocking pool). It talks to the UI thread only through
  channels the UI thread drains and a wake-up; the UI thread never waits on it.
- **HTTP:** hyper 1's client connections (`hyper::client::conn::http1`, its `client` feature, already compiled in the
  tree) over tokio-rustls, one connection per stream, no pool. Calls take two keep-alive connections: one for typing
  (its order matters), one for everything else.
- **Server-Sent Events** are read by a small parser of the client's own (fields `event`, `data`, `id`, comments), with
  a cap of 16 MiB per event: a bigger one drops that stream, which reopens.
- **Connections per remote,** under home's 16 per token: one event stream, at most 11 terminal streams, one each for
  calls, slow calls and typing, one spare for a reconnect's overlap.
- **Proxies:** the Connect dialog's More… has **Proxy** (`http://proxy.corp:8080`, optional), prefilled from
  `HTTPS_PROXY` (or `https_proxy`, `ALL_PROXY`) unless `NO_PROXY` names the host, and kept with the remote. Through it
  the mirror opens an HTTP `CONNECT`, with Basic credentials when the proxy's URL holds them (kept in the keyring with
  the key, never in the index), then its TLS inside, end to end. The system's own proxy settings and PAC files are not
  read, nor NTLM or Kerberos asked (*the designer's call*: a work network often has a proxy, and an app started from the
  Start menu or the Dock doesn't get the variables, so the person can type it; a refusal is told, never guessed around).
  **This computer is never reached through a proxy** (a Loopback remote ignores any proxy; `proxy_from_env` answers
  none for `127.0.0.1`, `::1`, `localhost`, as curl does): the key would otherwise cross the proxy in clear.
- **Every request** carries `Authorization: Bearer`, the Access headers when set, `User-Agent: Templum/<version>
  (mirror)` and `Templum-Mirror: <session>`, a random value new at each connect or reconnect.
- **A reconnect clears the old connections** (*the designer's call*: a laptop that slept or changed networks leaves
  its connections open at home, where nothing notices them dead for many minutes, and they would hold the token's 16).
  When a share token opens an event stream with a session other than the one its connections carry, home first sends
  `event: replaced` on that token's other streams, then closes every connection of the token from another session. A
  mirror that gets `replaced` while its own connections are alive stops and says so (see
  [When it isn't live](#when-it-isnt-live)), so two computers holding the same code don't knock each other off in turn.
  Tokens Settings made keep part 1's rules.
- **Liveness:** a stream that sends nothing for 45 s is dead. When no call ran for 15 s, the mirror calls
  `GET /v1/me`: it keeps the round trip fresh and learns a changed expiry.

### Mirroring the state

- **Opening:** `GET /v1/me` (the token: its scope, its workspace, its expiry; home's computer name), then
  `GET /v1/events?view=true`. The snapshot is the truth; puts and deletes change it; a new snapshot replaces it
  (part 1's rules). The mirror reuses `templum-api`'s state types to read them.
- **A remote workspace is a workspace** on the mirror (*the designer's call*: the sidebar, switching, the camera,
  levels of detail, selection, snapping, moving and undo then work as they do), marked as remote, with these rules:
  - **Never saved** as a canvas: only its index entry ([Persistence on the mirror](#persistence-on-the-mirror)). Its
    blocks come from home each time.
  - **Never in the mirror's stores:** no note, checklist, board or card of home's is written to the mirror's
    `notes.json`, `checklists.json` or `boards.json`.
  - **Home's block ids stay home's:** they live in the remote's own table, not in the ids the mirror's agents name
    blocks by, so no local tool can reach a remote block, and a local block with the same five letters is never
    confused with it.
  - **Never a relay:** the mirror's block server and control server don't see remote blocks (`overview` lists the
    remote workspace as "API (mirror of winx-desktop)" with no blocks; a tool naming it is refused "API is a mirror of
    winx-desktop: act on it there."); the mirror's API shows the remote workspace, never its content
    ([What the mirror's API shows](#what-the-mirrors-api-shows)).
- **Blocks by kind:** a `terminal` is a viewer (as a subagent's block is); `kanban`, `note`, `checklist` and `editor`
  are a "not mirrored yet" card in batch 1 (one `Body` arm for it); a kind the mirror doesn't know is that card too,
  with its kind's name.
- **Agents:** a remote terminal's dot, chips and "needs you" notice come from its block's `agent` in the state, not
  from hooks.
- **The UI thread applies what changed** at most once a frame: the client keeps, per remote, the newest state not yet
  taken (a later one replaces it), so a busy UI never piles up deltas.

### Terminals

- **Which stream:** the terminals on screen at Full detail, nearest the canvas's center first, at most 11, and always
  the focused one; the UI thread tells the client when that set changes, at most every 100 ms. Others keep their last
  frame (a terminal that comes into view gets live within a round trip). A remote workspace not on screen streams no
  terminal; its state stream stays open, for requests and dots (*the designer's call*: home paints only what is on
  screen, the mirror streams only what it paints).
- **A frame becomes bytes for the viewer** on the client's thread, which feeds the viewer's terminal as a PTY reader
  would and wakes the UI (painting never waits on it). A whole frame clears the screen; a row is a cursor move, its runs
  in SGR and an erase to the line's end; then the cursor (position, DECSCUSR shape, shown or hidden) and the title.
  The test batch 2 wrote, which redraws a frame into a viewer and reads back the same screen, becomes this code.
- **What home sends is drawn, never run** (*the designer's call*): every control character (C0, C1, ESC) in a run's
  text or the title is dropped before the bytes are made, the title is cut at 256 characters, colors are parsed strictly
  (0–255 or `#rrggbb`), and a frame larger than 2000 × 1000 cells (home's own largest grid) is refused. A home that
  went wrong can show false things; it can't make the mirror's terminal do anything.
- **Memory:** a viewer keeps no scrollback; per terminal the client holds at most one frame not yet fed (a newer one is
  merged in: its rows replace those rows, a whole frame replaces everything).
- **The size is home's:** the viewer has home's columns and rows, and its cells are scaled so that grid fills the
  block's content (the smaller of the two ratios), at the nearest of the zoom's font steps, so the glyph cache stays
  bounded. Resizing the block at the mirror sends a move; home re-grids; a whole frame brings the new size.
- **Modes:** each frame says whether a program took over ([What home's API gains](#what-homes-api-gains)), so the
  mirror yields the shortcuts home would yield, and a later batch can send the mouse.

### The keyboard

*The orchestrator offered two ways:* extend `terminal_input`'s named keys, or add raw bytes encoded by the mirror.
**The designer's call: named keys, encoded at home.** The bytes a key makes depend on the terminal's modes at the
moment it arrives: application cursor keys, the kitty keyboard's flags (which Claude Code and Codex turn on: that is how
Shift+Enter reaches them), bracketed paste. Only home knows them live; the mirror sees them a round trip late. Home
already encodes keys for its own keyboard (`templum-terminal`'s `encode_key`), so a named key reaches the program as a
key pressed at home does, and raw bytes would be a second encoder that is wrong whenever a mode changed in flight.

- **A key's name:** `[ctrl+][alt+][shift+]<key>`, the modifiers in any order. `<key>` is `enter`, `escape`, `tab`,
  `backspace`, `delete`, `insert`, `up`, `down`, `left`, `right`, `home`, `end`, `page_up`, `page_down`, `space`,
  `f1`–`f12`, or one character as typed, after Shift and the layout (`A`, `@`, `ç`, `á`). `ctrl+shift+p`, `alt+left`,
  `shift+f5`, `ctrl+alt+delete`. Today's names (`shift+tab`, `ctrl+<letter>`, one character) mean what they meant. For
  a letter, home takes its lowercase as the key and the letter as its text, as GPUI reports one.
  (A known limit: a shifted symbol with Ctrl or Alt reaches the kitty protocol with the symbol as its key, not the
  unshifted key under it; the mirror would have to send both, if a program ever needs it.)
- **The mirror names what GPUI reports:** a key with modifiers by its name; a character typed (dead keys and IME
  commits included) as one-character keys; AltGr's characters as characters (the same rule `encode_key` applies);
  Templum's own shortcuts never leave the mirror.
- **`terminal_input`'s `keys`** take these names, 1 to 256 per call, pressed back to back, with 50 ms after a key
  that sends a lone ESC (`escape`, `ctrl+[`, `ctrl+3`, `shift+escape`; not `alt+…`) that another key follows (so a
  program still reads Esc alone where the person paused, now that the network bunches keys). `send_keys` takes the same names and keeps its 16 keys and 50 ms between each.
- **Order and pace:** per terminal, one call in flight; keys typed meanwhile wait and go in the next call, up to 1024
  waiting (past that: "Typing faster than winx-desktop answers: some keys were dropped.", said once while the queue
  stays full). A paste is `text` (bracketed when the program asked for it, at home), at most 256 KiB ("Paste at most
  256 KiB into a mirrored terminal."). A call starts no sooner than 60 ms after the previous one did (at most about
  16 a second, under home's bucket of 40 and 20 a second), so autorepeat batches instead of draining the bucket; a
  call home answers 429 or 503 `busy` is made again after `Retry-After` (at least 60 ms), the keys behind it still
  waiting, and only after ten answers of "busy" in a row (or 5 s of waiting for them, whichever is first: a late key
  would land in whatever the program shows by then) is it given up and the block told it dropped keys. `Retry-After`
  counts as at most 30 s. Moves and answers retry on the ten answers only.
- **No predictive echo** (*the designer's call*): a character shows when home's frame shows it. The round trip is in
  the banner instead.

### Moving and resizing

*The orchestrator's call:* a move goes to home and the mirror draws what the next event says. Drawing only that would
make a drag trail the pointer by a round trip and a 100 ms tick, so (*the designer's call*):

- While the person drags or resizes, the block follows the pointer locally, and nothing is sent; on release the mirror
  sends one `move_block` per block it moved (a drag sent as it went would be dozens of Ctrl+Z steps at home and, for a
  selection, past the token's 20 calls a second).
- Until home's state shows the rectangle sent, or 2 s after the release, the mirror's rectangle stands over home's;
  then home's wins, whatever it is (the person at home moved it too: the last move home took is the truth).
- Every rectangle the person changes on the mirror is sent: a drag, a resize, Ctrl+Z and Ctrl+Shift+Z, a selection
  moved together. A refusal (`422`) puts the block back where home has it and shows the refusal's sentence.
- A known limit: a selection of five blocks moved on the mirror is five Ctrl+Z steps at home, one per `move_block`. A
  `move_block` for several blocks would make it one; nothing in batch 1 needs it.

### Requests

The state's `requests` of the remote workspace's blocks feed the mirror's inbox for that workspace, drawn as local
requests are; an answer is `answer_request` through the API (control scope; a worktree's request needs a Full share,
and says so where its buttons are). A request answered at home, or by another mirror, leaves on the next delete event.
The Templum Agent's approvals are never seen by a share (part 1's rule for limited tokens).

### What home's API gains

*The orchestrator's call:* what part 2 adds on home is in the API. The shapes are *the designer's call*; each is an
addition to `/v1`.

- **`GET /v1/me`** (read): who the token is.

  ```json
  { "name": "Another computer", "scope": "full", "workspaces": ["w3"], "expires": 1764150000, "share": "s4",
    "computer": "winx-desktop", "version": "0.1.6", "api": 1 }
  ```

  `computer` is the host's name (sysinfo's, already in the tree); `share` is null for a token Settings made.
- **`shares`** in the state, and `share` events:

  ```json
  { "id": "s4", "workspace": "w3", "name": "Another computer", "scope": "full", "created": 1761560000,
    "expires": 1764150000, "way": "public", "address": "https://templum.example.com",
    "last": { "at": 1761561000, "addr": "203.0.113.7" }, "mirrored": true }
  ```

  `mirrored` is true while an event stream of its token is open. A token for all workspaces sees every share; a limited
  token sees only the share it is.
- **Tools** (the API's own, like the automations': *the designer's call*, an agent never makes a way into the
  computer, so the control server doesn't have them):

  | Tool | Arguments | Does | Scope |
  |---|---|---|---|
  | `share_workspace` | `workspace`, `name`, `scope` (`control` or `full`), `expires` (Unix seconds, or null for never), `way` (`public` or `pinned`), `address` | Makes the share as the dialog does, with its checks and words; the answer's `text` is "Shared API as s4.", and its `code` field holds the connection code, once | full, and a token for all workspaces |
  | `revoke_share` | `share` (omitted: the calling token's own) | Revokes it; its streams end | full and a token for all workspaces; any share may revoke itself |

  Aliases: `GET /v1/shares`, `POST /v1/workspaces/{workspace}/shares`, `DELETE /v1/shares/{share}`. Refusals:
  "A pinned share needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS." and "The address must start with
  https://.". Settings ▸ API's settings themselves stay the person's to change in the app (part 1's non-goal).
- **A block's `read_only`:** true for a terminal that takes no input (a subagent's transcript); left out otherwise.
- **A workspace's `icon`:** its Lucide name, or null; the mirror draws home's tile.
- **A frame's `modes`:** `{"full_screen":true,"mouse":true,"kitty_keys":true}`, each present only when true: the
  three a program takes the keyboard by at home (`Modes`, read with the screen under the same `try_lock`).
- **`terminal_input`'s keys** as [The keyboard](#the-keyboard) says.
- **The mirror's session:** `Templum-Mirror` and `event: replaced`, as [The client](#the-client) says, for share tokens.
- **The audit log groups a mirror's chatter** (*the designer's call*): `terminal_input` and `GET /v1/me` calls are one
  line per token, call and minute, with `more` counting the rest, as the noise log does, so a morning of typing can't
  rotate the other calls out of `audit.log`'s 2 MiB. Every other call keeps its own line.
- **The share is kept with its token** in `api.json` (`share`: workspace, way, address), so Settings ▸ API, the Edit…
  dialog and the state read it from one place; revoking the token is revoking the share.

### What the mirror's API shows

A remote workspace is in the mirror's state as a workspace with `remote`:
`{"computer":"winx-desktop","host":"templum.example.com","way":"public","status":"live","latency_ms":84}` (`status`:
`connecting`, `live`, `reconnecting`, `cut`, `refused`, `disconnected`), and its blocks never are (*the designer's
call*: data in the state is this feature's API surface, CLAUDE.md's "or its data in the API's state"; connecting needs
a code, a secret the API doesn't take).

### Persistence on the mirror

`workspaces/index.json`'s entry of a remote workspace (id `r<n>`, the smallest unused) gains `remote`: the `url`, the
`way`, the `pin`, home's workspace id, the keyring entry's name, `trust_added_authority`, the Access Client ID, the
proxy's address without its credentials, whether the key is in a file, and the camera. No `<id>.json` is written for it. None of these is a secret. An older Templum
reading the index shows it as an empty workspace and drops the connection when it next writes the index (a downgrade
loses the connection, never a secret).

## Security

### Where the key is kept

*The orchestrator asked* for the system's keyring or a `0600` file, justified. **The designer's call: the system's
keyring**, with a file only when there is none and the person asks for it.

- **Why the keyring.** Part 1 promises that "a copy of the data folder carries no token that works": home keeps only
  hashes. A key in a mirror's data folder would break that promise on the mirror, which holds a working key to home
  (often Full). A data folder gets copied, backed up and synced; the agents that run on the mirror run as the person and
  read files. The keyring keeps the key out of the folder, encrypted at rest under the person's login (Keychain, the
  Credential Manager's DPAPI, the Secret Service's collection), and an agent would have to ask for it on purpose rather
  than find it in a `grep`. The file would be simpler; it would also be the one place on either computer where a copy
  of the folder is a working key.
- **The crates:** `keyring-core` 1 (its `Entry`, `set_default_store` and the `mock` store the tests use) with
  `apple-native-keyring-store` (macOS, `keychain`), `windows-native-keyring-store` (Windows) and
  `zbus-secret-service-keyring-store` (Linux and the BSDs, `rt-async-io-crypto-rust`: `secret-service` 5 on zbus 5
  with async-io, which the tree already builds for the terminals' scopes, with RustCrypto's aes, cbc, hkdf and sha2 and
  `num`, which the lock already has, though a newer aes or cbc may come as a second version). New: those four and
  `secret-service`, all MIT or Apache-2.0, maintained by the keyring-rs project; the plan checks the lock's diff and
  `cargo deny check` (the `keyring` umbrella crate's defaults would bring stores Templum doesn't use).
- **Entries:** service "Templum" ("Templum (dev)" for `--dev`), account `remote-<8 random characters>`; the share's
  token and, when set, the Access Client Secret and the proxy's credentials, as one JSON value.
- **The key also holds where it may go:** the code's address, the way in (Loopback, Public, or Pinned with its pin),
  whether an authority the computer added was accepted, and the proxy's address. The index (`workspaces/index.json`)
  holds the same for the UI, but anything that writes the data folder (an agent on the mirror) can rewrite it, and the
  keyring cannot be written without the person's login. So before the first connection of a start the remote's values
  are compared with the key's, and if any differs the key is not sent anywhere: the remote is cut as "Templum couldn't
  read this connection's key" with "This connection's address or way in changed outside Templum, so its key isn't
  sent. Connect again with a new code." Otherwise an edit of the index to `https://attacker.example`, whose public
  certificate is valid, would carry the Full token there at the next start. The `0600` file and the memory copy hold
  the same fields.
- **No keyring** (a Linux session without a Secret Service, a keyring that refuses): the connection lasts until Templum
  quits, unless the person turns on **Keep the key in a file only you can read**: `<data>/remotes/<entry>.key`, `0600`
  in a `0700` folder, written through `private_write` as `api.json` is. Removed by Forget.
- **On macOS** an update changes Templum's ad-hoc signature, so the Keychain may ask once whether Templum may read its
  item; denying it is the keyring's refusal in [When it isn't live](#when-it-isnt-live).
- **Read and written off the UI thread** (an unlocked keyring may prompt); read once per start, then held only by the
  client's thread.
- **Never shown:** the key is a type whose `Debug` and `Display` print `tpl_…`; no error, log line, notification,
  panic message, `config.toml`, index entry or environment variable ever holds it, nor the code. A test formats every
  error and state of a remote and finds no `tpl_` in them.

### What crosses and who sees it

- **Pinned:** only home and the mirror. **Public through cloudflared:** the mirror, Cloudflare's edge (it ends TLS),
  then cloudflared's own encrypted tunnel to home, then loopback. Every hop is encrypted; Cloudflare sees what passes.
  docs/api.md says so, and recommends Cloudflare Access with a service token and short expiries.
- **Home's side** is part 1's: the share is a token, with its limits, audit log lines, last use and rate limits; home's
  per-token limits bound a mirror even if it went wrong.

### Threat model

| Threat | What stops it, or limits it |
|---|---|
| **A leaked code** (a chat, a screenshot, the clipboard's history) | It is a token: one workspace, Control or Full, an expiry (30 days by default), Revoke in Edit… or Settings. Shown once; `tplc1_` for scanners; the mirror clears the clipboard after connecting. Home's tile shows when it is mirrored and from where. |
| **A copied or stolen mirror data folder** | No key in it: the keyring holds it (unless the person chose the file, warned). |
| **An agent on the mirror computer** | The key is never in a file Templum writes (by default), an environment or a tool's answer; the mirror never relays, so its agents and API can't reach home. On Linux and Windows a process of the same user can still ask the keyring: docs/api.md says to keep agents you don't trust away from a mirror with a Full share. |
| **Someone between, on a direct network** | The pin: a different certificate is refused, and only a new code moves past it. |
| **Someone between on a tunnel's path; a company proxy reading HTTPS** | The web PKI checks name and dates; an authority the computer added is refused unless the person accepts it for that remote, told what it means. |
| **The tunnel's provider** | Sees the traffic by the person's choice; Cloudflare Access as a second lock; short expiries. End-to-end through tunnels is a later card. |
| **A home that went wrong, or a code to someone else's Templum** | The mirror draws and never runs: control characters dropped, sizes capped, events capped at 16 MiB, no redirects. The banner names the computer and the address the mirror is connected to. What the person types goes to that Templum, as with any remote shell. |
| **A revoked or expired share** | Its streams end at once (part 1); every call is `401`; the mirror stops, says why and never retries a `401`. |
| **Home's certificate key stolen** (from home's data folder) | Pinned mirrors could be fooled until home makes a New certificate, which breaks the pins; the person shares again. |
| **A mirror that floods** (a bug) | Home's limits per token (16 connections, 20 calls a second) and the mirror's own budget under them. |
| **Two hands on one block** | Home is the truth: the last move home took wins; the mirror's own rectangle lasts 2 s after a release at most. |
| *(batch 2)* **A mirror running home's automations** | Never: home's rules live only in home's `automations.json` and run only in home's look. The mirror draws them and edits them through the API; its own rules never offer or target a remote workspace, a stale one is Skipped, and its event rules never hear a remote agent. |
| *(batch 2)* **Home's content reaching the mirror's agents** | Send to lists only the remote's terminals; Generate runs at home; Copy and Paste of blocks between a remote and a local workspace are off; the mirror's API, control server and block server see no remote content. |
| *(batch 2)* **A Control share now reaching further** (commits, pushes, cards, agents) | It is the person's own hand, as a Control token always was (it could open agents); the Share… dialog says so. Everything that loses work needs Full: discards, deleted branches and stashes, aborted merges, deleted automations, command automations, typing and the mouse. |
| *(batch 2)* **Git through a share** | Named operations only, never raw arguments; the repository confined to the workspace's folders (root and main tree, canonical, network shares refused); paths inside the repository; hooks run as at home. |
| *(batch 2)* **A home gone wrong, with batch 2's state** | Every new collection capped and cleaned where it enters; drawings' points finite and bounded; `git` answers at most 8 MiB; one terminal's history at a time; the state still within 64 MiB. |

## Constraints

- **Crates** (CLAUDE.md): the client is UI-free and lives in `templum-api`, which becomes "Templum's HTTP API, both
  ends" (*the designer's call*: it reuses the state's and frames' types, `tls.rs`'s fingerprint and `store.rs`'s
  private writes; a crate of its own would duplicate them). Its new parts: the connection code, the client (TLS with
  both ways in, proxies, calls, the SSE reader, backoff), a remote's state (snapshot and deltas applied, the newest kept),
  frames to viewer bytes, and the key's keeping. `templum-terminal` gains the key names in `named_key` and the modes
  in its screen. `crates/templum` holds the glue: the remote workspaces, their blocks, the streams' set, typing, moves,
  requests, the dialogs, the sidebar's button and marks, and the share tools on home's side.
- **Dependencies** (*the orchestrator's call*: the tree first): hyper's `client` feature (already built, for gpui's
  HTTP client), tokio-rustls, webpki-roots 1 (built, for ureq), ring, base64, serde_json. **New:**
  `rustls-platform-verifier` 0.7 (MIT or Apache-2.0, the rustls project's), because the system's roots read as a list
  (rustls-native-certs) miss the roots Windows loads only when its own verifier asks for them, and a work computer is
  often Windows; it brings, all already in `Cargo.lock`, rustls-native-certs and openssl-probe on Linux,
  security-framework on macOS, windows-sys on Windows. And the keyring's crates above.
- **The UI thread never waits** on the network or the keyring; it drains channels.
- **Painting:** a remote terminal is a viewer, painted as any terminal; frames are fed off the UI thread. A new
  `--bench-mirror` runs one Templum as both ends: the `--bench-mixed` cockpit shared over loopback and its remote
  workspace on screen, its terminals streaming; `p95_ms` ≤ 16.7, pasted in the pull request.
- **Memory is bounded:** per remote, one pending state, at most 11 terminal streams, one pending frame each, events of
  at most 16 MiB, 1024 waiting keys; viewers without scrollback.
- **Tests** run on Linux, macOS and Windows, with no agent CLI, no real keyring (keyring-core's mock store, or the file
  in a temporary folder), no network beyond loopback, no sleeps (conditions with a deadline):
  - unit: the code (round trip, cut short, newer, plain HTTP refused), the pinned verifier (a match, a mismatch, a
    bad signature), the public check with verifiers the test builds from roots it makes (the client takes its "system"
    and "public" verifiers as rustls verifiers; rcgen: an authority and a leaf for `localhost`; a chain the test's
    "system" verifier accepts and its "public" one doesn't is the private authority), `CONNECT` through a proxy the
    test runs on loopback, the session's `replaced` (two clients, one code), the SSE
    reader (fields, multi-line data, comments, the cap), backoff, frame to bytes (sanitized: an ESC in a run is
    dropped), the key names (each new name to its bytes in each mode), the redacted key;
  - an integration test in `templum-api`: a server and a client in one process, over plain loopback and over TLS
    pinned: snapshot, deltas, a terminal's frames, `terminal_input`, `GET /v1/me`, a revoke that cuts the client with
    `401`, a changed pin refused, the server stopped and started again and the client back with `Last-Event-ID`;
  - the end to end test (*the orchestrator's call*): a headless workspace that shares one of its workspaces and
    mirrors it in a remote workspace over loopback, in one process: home's blocks appear at home's rectangles;
    what a shell at home prints shows in the mirror's viewer; a key typed on the mirror reaches home's terminal; a move
    on the mirror moves home's block; a question answered on the mirror is answered at home; Revoke in the Edit…
    dialog cuts the mirror; the remote is never saved as a canvas and no local tool sees its blocks.
- **Every feature in the API:** docs/api.md gains `GET /v1/me`, `shares`, the share tools, `read_only`, `icon`,
  `modes`, the key names, `Templum-Mirror` and `replaced`, and the audit log's grouping, and a "Templum to Templum"
  section for people (share, cloudflared, connect, what each way in checks, Cloudflare Access with a service token, a
  proxy, what a mirror can't do yet).
- **Specs to update as batch 1 lands:** api.md (its Templum to Templum section points here), workspaces.md and
  cockpit.md (remote tiles, the Connect button, the Edit… dialog's Sharing), settings.md (shares among the tokens),
  ARCHITECTURE.md (`templum-api`'s client, the remote workspaces' glue), CLAUDE.md (`--bench-mirror`), this file's
  Status and As built.

## Batches

*The orchestrator's call:* each batch usable on its own; batch 1 the smallest that makes the person's case real;
later batches only as far as batch 1 prepares for them.

- **Batch 1: keep working from another computer.**
  - Home: the Edit… dialog's Sharing and the Share dialog; the code; the tile's mark; shares in Settings ▸ API (and
    those of a deleted workspace); New certificate's count. API: `GET /v1/me`, `shares` and `share` events,
    `share_workspace`, `revoke_share`, `read_only`, `icon`, `modes`, the key names, the mirror's session and
    `replaced`, the audit log's grouping.
  - Mirror: Connect to a Templum…; the code; both ways in and the private authority check; the proxy; Cloudflare
    Access's service token; the keyring and the file; the remote tile, banner and canvas; terminals live (11 at most);
    the keyboard; moving and resizing; requests with their sound and notification; the "not mirrored yet" cards and the
    disabled actions; every state of [When it isn't live](#when-it-isnt-live), Start of a stopped workspace included;
    Edit…, Disconnect, Forget; the mirror's API state.
  - `--bench-mirror`, the tests above, docs/api.md.
- **Batch 2: the whole workspace, live, with its popups:** designed in [Batch 2](#batch-2-the-whole-workspace-live)
  below, built in parts 2a–2h.

### Batch 2: the whole workspace, live

The person, for this batch (PT-BR, translated): "now batch 2 … send an agent to do batch 2 completely … does batch 2
include the integration with the Git tab, the inbox, automations and agents? A warning about automations: every run
always happens on the main computer. It must never happen on the remote computer, it makes no sense; imagine the
automations duplicated, everything would break. … Only come back when everything is ready and completely
integrated." They asked not to be asked: every open question below is *the designer's call*, the one most
recommended for the system.

Batch 1 shows home's workspace and lets the person type, move blocks and answer requests. Batch 2 makes the remote
workspace a whole workspace: every block home has is drawn live, everything a person does to a workspace is done at
home through home's API, and the four popups that act on a workspace (Git, Requests, Agents, Automations) work on the
remote one. Batch 1's rules stand unchanged: home is the truth; nothing of home's runs on the mirror; never a relay;
everything home sends is capped and cleaned where it enters; the share's scope gates every act and the mirror says why
in words; the UI thread never waits on the network; painting stays under the bench's bar; memory doesn't grow with what
home sends.

#### The rule: home's automations run only at home

*The person's call, a hard rule.* Every run of one of home's automations (at its time, on its event, Run now) happens
at home, in home's own look, and never on a mirror. A mirror that ran them would run each one twice.

- **Home's automations live only in home's `automations.json`.** The mirror draws them from home's state and changes
  them only through home's API (`save_automation`, `set_automation`, `run_automation`, `delete_automation`), each of
  which acts at home. The mirror never copies one into its own `automations.json`, never schedules it, never runs it,
  never fires it on an event.
- **The mirror's own automations are for its local workspaces only.** Their **Where** never lists a remote workspace; a
  new one starts on the first local workspace, not the one shown; none of their targets (an agent, a terminal, a
  folder) can be a remote block. A rule saved before batch 2 whose Where is a remote workspace runs nothing: each of
  its runs is Skipped "Target missing: API is a mirror of winx-desktop: its automations run there." (This fixes the
  minor "Automations default to, and offer, a remote workspace".)
- **The mirror's event rules never hear a remote agent.** A remote pane's state comes from home's state, never from
  hooks or looks, and is never an event of the mirror's automations: a rule on "any Claude Code" fires for the
  mirror's own Claude Codes only.
- **An agent's schedule of home's is home's:** the mirror lists it and cancels it at home, never delivers it.

Part 2h proves each line with a test.

#### Goals

1. **Everything on screen is live:** boards, checklists, notes, ink and shapes are drawn from home's state by the views
   a local block uses. Only editors stay batch 1's card.
2. **What a person does to a workspace works on a remote one,** done at home: adding and closing blocks, wires,
   opening an agent, handing a task, cards and columns, checklist items, scrolling a terminal back, the mouse in a
   terminal.
3. **The four popups are integrated:** the Git popup works on home's repositories; the Requests inbox lists every
   request of home's for the workspace; the Agents popup lists home's agents; the Automations popup shows and edits
   home's rules, which run at home.
4. **One rule says what the mirror can do,** and what it can't is dimmed with its reason: not connected, the share's
   scope, a home whose Templum is older, or not on a mirror yet.
5. **Every act the mirror needs is in home's API,** with its scope and its lines in docs/api.md (CLAUDE.md's rule).

#### Left out of batch 2

Each is its own kanban card (*the designer's call*):

1. Drawing from the mirror: drawing, erasing, moving and deleting ink and shapes (they are drawn live).
2. An editor's files through the API: an editor stays a card.
3. Following home's camera.
4. Sealed through tunnels: Templum's pinned TLS inside one WebSocket.
5. Undo on a mirror beyond moves: an undo at home that undoes the mirror's own steps, not the person's at home.
6. A card's attachments from the mirror: thumbnails, opening and attaching (their names and sizes show).
7. Kanban drafts on a mirror: a remote card's dialog has no Save draft.
8. Move to workspace, and Copy and Paste of blocks on a remote workspace. (Copy and Paste of a block between a
   remote and a local workspace stays off for good: it would write home's content into the mirror's stores, batch 1's
   anti-goal.) Grid mode, left out at first, works (kanban card 206): it is the mirror's own view.
9. In the Git popup on a remote: Clone a repository…, Choose another folder… (a picker of home's folders), the ⚙
   beside Generate (home's Settings), Open in editor, and Run in a terminal on a failed command.
10. In the Agents popup on a remote agent, each subagent's line (their count shows); the dot that travels along a wire
    while a task is handed.
11. A `move_block` for several blocks at once: the minor "a selection of several blocks moved on the mirror is one
    Ctrl+Z step per block at home" stays.
12. Stopping a remote workspace at home from the mirror (it would cut the mirror's own view; Start stays).

Editing notes from the mirror (their text, names, colors and tabs, making, opening, closing and deleting notes),
left out at first, is designed (kanban card 204): [Notes on a mirror](#notes-on-a-mirror-kanban-card-204).
Home's shells in Add ▸ Terminal ▸, left out at first, are there (kanban card 201): [The canvas](#the-canvas).
Home's recent folders in the Git popup's folder menu are there (kanban card 203); Choose another folder… (item 10)
stays: [The Git popup on a remote workspace](#the-git-popup-on-a-remote-workspace).
Renaming a checklist, reordering its items and showing another checklist in a block, left out at first, work
(kanban card 205): [Checklists](#boards-checklists-notes-ink).

Why these: the person named Git, the inbox, automations and agents, and "completely" is read as everything a person
reads on a workspace, and every act they take on it that is one discrete change. Editing a note's text is not one: two
computers typing into one text a round trip apart needs a design of its own for conflicts, and a half-done one loses
text (kanban card 204 is that design). Checklist items are discrete (an id and a value), so adding, editing, checking
and deleting them are in batch 2. Items 1 and 10 are cheap but rare from another computer; 2 and 4 widen what crosses
the network (files, a second TLS); 3, 5–9, 11 and 12 are conveniences whose absence the mirror says in words.

Its anti-goals and threats are with batch 1's, marked *(batch 2)*: [Anti-goals](#anti-goals-a-failure-even-if-it-works)
and the [threat model](#threat-model).

#### What works on a remote workspace

Every act below is a call to home's API through the mirror's client. The scope is the one home's registry gives the
call; the mirror dims what its share can't do with the words of [One rule](#one-rule-for-what-the-mirror-can-do).

| The person, on the mirror | Home runs | Share |
|---|---|---|
| Adds a terminal, a kanban, a checklist or a note | `add_block` (with `x`, `y`; a terminal's `shell`) | Control |
| Lists home's agents and shells in the menus | `list_agents`, `list_shells` | any |
| Lists home's recent folders in the Git popup's folder menu | `recent_folders` | any |
| Opens an agent | `open_agent` (with `x`, `y`) | Control |
| Closes a block | `close_block` | Control |
| Wires or unwires two blocks, names a wire | `connect` (with `label`), `disconnect` | Control |
| Hands an agent a task | `delegate` | Control |
| Types, pastes, clicks or scrolls in a program | `terminal_input` (`keys`, `text`, `mouse`) | Full |
| Scrolls a terminal back | `read_terminal` | any |
| Moves, makes, edits, comments on or deletes a card; edits columns and boards | `move_card`, `create_card`, `edit_card`, `comment_card`, `edit_comment`, `delete_comment`, `delete_card`, `edit_column`, `edit_board`, `show_board` | Control |
| Checks, adds, edits or deletes a checklist item | `check_item`, `add_items`, `edit_item`, `delete_item` | Control |
| Renames a checklist, moves its items; a new checklist in a block that shows none | `edit_checklist`, `new_checklist` | Control |
| Types in, renames, recolors a note; makes, closes, reopens one | `edit_note`, `new_note`, `close_note`, `open_note` | Control |
| Deletes a note | `delete_note` | Full |
| Reads a repository in the Git popup | `git` (its reads) | any |
| Stages, commits, pushes, switches and makes branches, stashes, reverts… | `git` | Control |
| Discards changes, restores a version, deletes a branch, drops a stash, aborts a merge | `git` | Full |
| Switches, runs, saves (no command) or duplicates an automation | `set_automation`, `run_automation`, `save_automation` | Control |
| Saves an automation that runs a command; deletes one | `save_automation`, `delete_automation` | Full |
| Cancels an agent's schedule | `cancel_schedule` | Control |
| Answers a request | `answer_request` | Control; Full for a worktree's |

#### The canvas

- **Adding:** the toolbar's buttons, the canvas menu's **Add ▸** (Terminal ▸, home's shells: below; Kanban board;
  Checklist) and their shortcuts work on a remote workspace. The block goes where it would go locally (the menu's
  click, else where a new block goes in the mirror's view), sent as `x` and `y`; home sizes it by its kind. A checklist
  is added as the local Add adds one, named, through `add_block`'s `text`. **Note** adds a note block at home the same
  way (kanban card 204; home names its note Untitled); **Editor** says "Not on a mirrored workspace yet".
- **The new block** shows when home's state brings it, a round trip later; the mirror then gives it the keyboard, as a
  local add does, unless the person focused something else meanwhile (or 5 s passed since home's answer). No
  placeholder stands in for it (*the designer's call*: a round trip is short, and a ghost would be a block that isn't).
- **Choosing a shell** (kanban card 201): **Terminal ▸** (the toolbar's Terminal, the canvas menu's and a tile's
  **Add ▸ Terminal ▸**) lists home's shells as home's own menu lists them, from home's `list_shells` (read with
  home's agents, and again when the menu opens a minute or more after the last read): **Default (*zsh*)**, home's
  default, then each other shell found there (fish, PowerShell 7…); "Asking winx-desktop…" under Default until the
  first answer. The chosen one is sent by its name as `add_block`'s `shell`; Default sends none, and Ctrl+Shift+T
  opens it without a menu, as at home. A home whose Templum is older has Default only, and a dimmed **Other shells**
  says "winx-desktop's Templum is older: update it to do this from here." (`Deed::AddShell`, which needs `add_block`'s
  `shell`). The mirror never lists its own shells there. *Security:* `list_shells` gives names only, never a
  program's path; home opens only a shell of its own list by that name and refuses any other ("There's no shell
  "fish": list_shells lists them."), so a share can't make home run a program of its choosing. A name the mirror
  reads is kept only when it already is one clean line (at most 32 shells besides the default, each once): it is
  never cleaned into another name.
- **Opening an agent:** **AI agent ▸** (the canvas menu, the toolbar) lists home's agents, from home's `list_agents`
  (read when the remote connects, and again when the menu opens a minute or more after the last read; "Asking
  winx-desktop…" until the first answer): installed ones first, the others greyed, as at home. Choosing one opens
  "Open *Claude Code* on winx-desktop" with the workspace's folder (tagged "Workspace folder"), its terminals' folders
  (from home's state, each once) and a path field ("A folder on winx-desktop: inside the workspace's folder, or whole");
  **Open** sends `open_agent` with the agent, the folder and `x`, `y`. There is no Personal space, recent folders or
  folder picker: home's are not known to the mirror, and a share's folder must lie inside the workspace's anyway. A
  refusal ("That folder lies outside API's folder.") stays in the dialog.
- **Closing:** ✕, the menu's Close and the shortcut. A terminal whose state shows an agent asks first and says why, as
  locally (a command running at home can't be told from the state, so only agents ask); then `close_block`. The block
  leaves the mirror at once (its fade, as locally) and comes back if home refuses, or still lists it 2 s later.
- **Wires:** the Wire tool, **Connect to ▸** and **Disconnect from ▸**, a selected wire's Delete and its menu's
  Disconnect, and **Rename wire…** (`connect` with `label`).
- **Hand a task…** (*the designer's call*): the block menu of a remote terminal whose state shows an agent has it. A
  dialog names the agent and its block, takes the task in a field of several lines (at most 256 KiB) and **Hand over**
  (Ctrl+Enter) sends `delegate`. Home types it as `[from API · Another computer, task t7]`; its end comes back as a
  [notice](#notices). A Control share can't type, so this is how it gives an agent work; the local canvas has no such
  item (the person types).
- **Menus** open on a remote workspace (the canvas's, a block's, a wire's), with every item that works there and the
  others dimmed with their reason as tooltip (this fixes the minor "A remote terminal has no right-click menu").
  **Shortcuts** that work act; the others say "Not on a mirrored workspace yet" once in a notification (this fixes the
  minor about shortcuts that answer nothing).
- **Undo:** on a remote workspace the mirror's Ctrl+Z still undoes moves (batch 1), and only moves. Any other act
  leaves a mark in its undo stack: Ctrl+Z reaching it says "On a mirror, Ctrl+Z undoes moves only: undo the rest on
  winx-desktop." and stops there, so it never undoes an older move by surprise.
- **Holds** (one rule, batch 1's for moves made general): what the person changed and sees at once (a block moved or
  closed, a card moved, an item checked, a wire made or removed) stands over home's state until home's state shows it
  or 2 s pass (`HOLD`), then home's state wins; a refusal ends it at once and shows its sentence.

#### Terminals

**Scrolling back:**

- The wheel up, Shift+PageUp or Find over a remote terminal whose frame's `modes` has neither `full_screen` nor `mouse`
  reads home's history: `read_terminal` with 2000 lines (`MAX_LINES`). The client feeds those lines into the viewer's
  scrollback, above the live screen, on its own thread (the UI thread never holds the text), then reopens the stream
  for a whole frame. The wheel, selecting, copying and Find then work in it as locally. The once-said "Only the live
  screen is mirrored for now." goes. The lines the person went back before the history came (the first wheel step,
  the first Shift+PageUp) wait for it (`RemoteSpace::back`) and the view goes back by them once it is there, unless
  they typed meanwhile.
- History lines have no colors: `read_terminal` serves text (*the designer's call*: history as frames would be a
  second stream API; text is what home already serves). The live screen keeps its colors.
- **Fresh at each look:** the history is read again each time the person starts scrolling back from the bottom, when
  the terminal changed since the last read. Lines that scroll off home's screen while the person reads show at the
  next scroll back.
- **Memory:** one terminal's history at a time per remote (scrolling back in another drops the first's), at most 2000
  lines; dropped when its stream ends.
- While the history is read the wheel scrolls what the viewer has; nothing waits.

**The mouse** (*the designer's call*: encoded at home, as keys are, because only home knows the program's mouse mode
at the moment the event arrives):

- While a frame's `modes` has `mouse`, a press, a release, a drag and the wheel over a remote terminal go to home's
  program as `terminal_input`'s `mouse` events, at the cell under the pointer in home's grid. Shift held keeps the
  mouse for selecting text, as locally.
- Moves with no button held are sent only while `modes` has `mouse_moves` (new: the program asked for every motion,
  `?1003`). A move goes only when its cell changes, and moves waiting in the queue merge into the newest, so a drag
  never queues hundreds.
- With `full_screen` and no `mouse`, the wheel is arrow keys, as locally, sent as keys.
- Mouse events ride the terminal's typing queue (one call in flight), so they keep their order with keys.
- With a Control share, the first press says once per terminal "Using the mouse in a terminal needs a Full share:
  share again on winx-desktop."; the wheel then scrolls the history where it can.

#### Boards, checklists, notes, ink

A remote kanban, checklist or note block is the same `Body` as a local one, drawn by the same view, reading the
remote's own copies of home's boards, cards, checklists and notes: kept in `RemoteSpace`, made from home's state,
never saved, never in the app's stores (`boards.json`, `checklists.json`, `notes.json` never hold one). Its edits go to
home as tools. Editors, and kinds the mirror doesn't know, stay batch 1's card.

**Boards:**

- Drawn as at home: the board the block shows (the state's `board`), its columns and cards' faces (number, labels,
  title, priority, checklist, description mark, comments, the assignee by its remote block's title).
- Dragging a card sends `move_card` with its column and `before` (held). Dragging a column's title sends
  `edit_column` with `before`. **+ Add a column**, a column's **Rename…** and **Delete** (which asks first, as locally)
  are `edit_column`.
- **The card dialog** is the same dialog. Done sends what the person changed, as it applies only that locally:
  `edit_card` with the title, description, priority, `for`, label toggles and checklist items they changed. A new
  card's Done sends `create_card`, then its comments with `comment_card` once home answers the card's number. Comment
  sends `comment_card` at once; editing and deleting a comment send `edit_comment` and `delete_comment` (the person's
  own to edit, any to delete, as locally); **Delete card** (asks first) sends `delete_card`. The assignee's list is the
  remote workspace's terminals.
- **No drafts** on a mirror: leaving a changed card asks with **Save** and **Discard** only. **Attachments** show their
  names and sizes; Attach and opening one are dimmed: "Attachments are on winx-desktop: open them there."
- **The header's menu:** **All boards…** lists the boards the share sees (those its kanban blocks show) and **New
  board** (`show_board` with `new`); a row sends `show_board` with that board; **Rename board…** and **Delete board…**
  (asks first, counting its cards) send `edit_board`.
- **A card's menu:** Open, **Send to ▸** (the remote workspace's terminals only; a paste, so Full), Delete card.

**Checklists:**

- Drawn from home's checklist (the state's `checklist`). Checking or unchecking an item sends `check_item` (held);
  **Add an item** sends `add_items`; an item edited in place sends `edit_item` on Enter or when its field loses the
  keyboard (locally every key is written; on a mirror the whole value goes once; Esc drops it); its ✕ sends
  `delete_item`.
- **The name** (kanban card 205) is a field as an item's is: Enter or the keyboard leaving sends `edit_checklist`
  with `name` once (nothing when unchanged; blank sends "Checklist", which home shows for a blank name), Esc drops it.
  The new name shows at once (held).
- **Dragging a row** onto another sends `edit_checklist` with the item and `before`, the row whose place it took; onto
  the list's empty space or + Add an item, `before: "end"`. It shows there at once (held, by the item that follows
  it at home); a drop that changes nothing sends nothing. A drag carries its block, so an item of another block
  (another workspace's, whose ids are this computer's) is never sent.
- **All checklists…** (the header's button, the block's menu, the canvas menu's Checklists…) lists home's
  checklists that the mirrored workspace's blocks show, which is what the share sees (a limited token sees a checklist
  only when a block of its workspaces shows it, and a checklist is open in one block at most), with their progress and
  "Open"; a click shows the block that has it, as locally. **New checklist**: in a block that shows none, home's
  `new_checklist` there; else home adds a checklist block beside it (`add_block` at `x`, `y`), or in the view without
  one. The trash deletes nothing: "Not on a mirrored workspace yet". A Checklists dialog acts only while its workspace
  is shown, whenever a mirror is in it (home's ids and this computer's are alike): "Another workspace is shown now:
  nothing was done."
- A remote block that shows no checklist offers New checklist and Open a checklist…, as locally, dimmed with the
  rule's words when the mirror can't (`Deed::OpenChecklist`).

**Notes** (batch 2e showed them; kanban card 204 made them editable, as built in
[Notes on a mirror](#notes-on-a-mirror-kanban-card-204)):

- Home's tabs and notes' text, live. The tab shown is the mirror's own choice, starting on home's (*the designer's
  call*: a view, as the camera is, not an edit).
- Typed in as locally: the text goes to home after a pause, checked against home's text; a change of home's in the same
  place keeps both texts (the person's as a new note beside it). The tabs' + and ✕, Rename…, Color ▸, All notes… and
  the empty block's buttons are home's acts, dimmed with the rule's words when the mirror can't. Selecting, Copy,
  **Copy all** and Find work; **Send to ▸** lists the remote workspace's terminals only.
- A note home cut (past 256 KiB) ends with "… The rest is on winx-desktop." and is read-only.

**Ink and shapes:** home's drawings of the workspace, painted above the blocks by the local painter, their colors by
name in the mirror's theme. The drawing tools stay dimmed ("Drawing on a mirror isn't there yet: draw on
winx-desktop."), and a drawing can't be selected.

#### The Git popup on a remote workspace

- **The same popup** (Changes, History, Branches, Stashes, the diff panel, the commit area, compare, conflicts, blame,
  file history, Recently discarded), opened by Ctrl+Shift+G and the Git button on a remote workspace. Everything it
  reads and does runs at home, in home's repository, through the API's `git` tool
  ([below](#the-git-tool)).
- **The folder:** the focused remote terminal's (its `folder` in home's state), else another remote terminal's, else
  the workspace's; home finds the repository from it. The folder menu lists the workspace's folder and its terminals'
  folders, then home's recent folders of the workspace (kanban card 203: `recent_folders`, read when the popup opens
  and again when the menu opens, for its next opening), each once, less the one shown and those inside its
  repository; **Refresh** and **Open a terminal here** (`add_block` at home, in that folder). A recent folder chosen
  points the Git there at home, as the others do. *Security:* home answers a share only the recent folders inside its
  workspace's folder, links followed, as their real paths: never one the share couldn't open. The header's terminal
  count lists the remote terminals whose folder lies in the repository.
- **Reads:** the status every 2 s while the popup is open (one call each, as the local poll), the tab's list and the
  diff panel as the tab asks. **Acts:** each an operation at home; the popup's rules stay (one command at a time per
  Git, the queue, the busy buttons).
- **Network operations** (Fetch, Pull, Push, Sync, Publish) and **Generate ✨** go on the client's slow connection and
  wait up to 10 minutes. Through a tunnel that cuts an answer silent for 100 s (Cloudflare does), a long one says
  "winx-desktop didn't answer in time: it may still be running there." and the popup reads the status again.
- **Generate ✨ runs at home,** with home's commit-message agent and Settings ▸ Git: the mirror's own agent never sees
  home's diff (never a relay). If home never asked its first-use question, the call is refused "Generate a message on
  winx-desktop once first: it asks there whether the diff may go to the agent's provider." The ⚙ beside it is dimmed.
- **Discard** keeps home's trash (in home's data folder): its notification's **Undo** and **Recently discarded…**
  restore from it at home.
- **Send to ▸** (a diff line) lists the remote workspace's terminals only. Open in editor and Run in a terminal aren't
  offered; a failed command's notification has **Details** (home's whole stderr).
- **Scope:** reads take any share. Operations that can lose work (Discard of a hunk, a file or all, Restore this
  version…, Delete a branch, Drop a stash, Abort a merge or rebase) take a Full share; on a Control share their buttons
  and items are dimmed with "Needs a Full share: share again on winx-desktop.". Every other act takes Control.
- **Kept:** a remote's Git (folder, tab, the message being written) lives in memory only and is gone after a restart
  (*the designer's call*: a remote has no canvas file, and its index entry holds no content).

#### The Agents popup

- **Remote agents listed:** every agent of every remote workspace, grouped under the remote's name ("API ·
  winx-desktop"), with the same rows, order, chips, search and selection. A remote that isn't live lists its agents
  dimmed, at their last state.
- **State:** home's `agent.state`, as the remote block's dot already says it. **Done** is the mirror's own (*the
  designer's call*): in memory, set when home's state goes from working, or waiting for subagents, to idle or done
  while the person isn't looking at its block on the mirror, cleared when they look at it there. Home's own `done` is
  home's person's mark and reads as idle on the mirror: looking on one computer doesn't clear the other's.
- **Details:** From (the agent's title from home's `list_agents`; model, effort and role from the state), the state
  and what it asks, **Task** (home's new `agent.task`), **Card** (the inbox's rule over home's cards), **Usage**
  (home's), **Subagents** (their count), **Screen** (the viewer's screen when its terminal streams; else
  `read_terminal` of 12 lines when it is selected, again every 2 s while it stays selected and the popup is open).
- **Go to terminal** shows the remote workspace and gives the block the keyboard; **Open request** opens the remote's
  inbox at it. The sidebar's badge counts remote Done agents too. A remote terminal's effort shows in its usage chip's
  tooltip (this fixes the minor).
- No controls in the list, as locally: Hand a task… is in the block's menu.

#### The Automations popup

- **The list:** the mirror's own automations first, then a group per remote workspace, "On winx-desktop · API", with
  home's rules whose Where is that workspace (what the share sees), drawn as rows are, with home's switch, next and
  last runs, and a dim line "Rules for all of winx-desktop's workspaces aren't shown here." A remote that isn't live
  dims its group and its acts.
- **On a home rule:** the switch sends `set_automation`; **Run now** sends `run_automation` (it runs at home);
  **Duplicate** sends `save_automation` of the copy, off; **Delete** (asks first) sends `delete_automation` (Full).
  The group's **+** starts a new rule for home, its Where fixed to that remote workspace.
- **The editor** for a home rule is the same form, its lists filled from home: Who and Send a prompt's Agent from home's
  agent blocks of the workspace (the state), Who's kinds and Start an agent's list from home's `list_agents`, A
  terminal from home's terminals; a Folder is home's (inside the workspace's folder, or whole). Save checks the form's
  own rules (times, dates, required fields), then sends `save_automation` (with `id` to replace); home's refusal shows
  in the red line; until Save, Revert or a restart the draft is the mirror's, in memory only, never saved on the
  mirror. Run a command
  needs a Full share (Save dimmed with the words).
- **Under a home rule's foot:** "Saved from another computer, it turns off when this share ends. Save it on
  winx-desktop to keep it." (*the designer's call*: part 1's rule that a token's automations end with the token stays;
  the person is told before it surprises them).
- **Notify me** of a home rule notifies at home and, through a [notice](#notices), on the mirror.
- **Asked by agents:** home's agents' schedules of the workspace's blocks (the state's new `schedules`), under the
  remote's group, with **Cancel** (`cancel_schedule`).
- **The badge** counts home's rules' failed, skipped and missed runs too, since the popup was last opened.
- And [the rule](#the-rule-homes-automations-run-only-at-home) above.

#### The Requests inbox

Batch 1 lists and answers home's requests. Batch 2 finishes it (each fixes a minor):

- A request's age is home's `since`, not when the mirror heard of it.
- A request whose block the mirror doesn't draw (past the 500 blocks) is listed, without Go to terminal.
- Past the 50 listed, the list ends with "And N more on winx-desktop."

Nothing else belongs in the inbox. It holds requests only (questions, plans, requests to connect, worktree requests);
the Templum Agent's approvals stay in its chat and a share never sees them (part 1); a task's end and a watch's match
are not requests but [notices](#notices).

#### Notices

Home's `notice` events (a task handed through the API ended or its agent needs its person, a watch set through the API
matched, and, new, an automation's Notify me) show on the mirror as notifications titled "API on winx-desktop", the
text's first line as their body (cleaned, 300 characters); a click shows the remote workspace and its block. Every
notice home sends for the workspace shows, whichever client of the person's handed the task (*the designer's call*:
all of them are the person's). The requests' cap applies: 3 in 60 s per remote, the rest in one "N more notices from
winx-desktop" a minute at most.

#### One rule for what the mirror can do

One function on the mirror answers, for an act on a remote workspace, whether it can be done now, else why; every
button, item, shortcut and drag asks it, and a dimmed control shows its answer as its tooltip:

1. "Not connected." while the remote isn't live.
2. "winx-desktop's Templum is older: update it to do this from here." when home's `GET /v1/tools` (read at each
   connect) lacks the tool. An argument home's schema lacks is left out when the act still does what the person meant
   without it (a block placed by home, a card at its column's end), else the act is dimmed the same way. A state
   without batch 2's fields shows notes and checklists as batch 1's card, "winx-desktop's Templum is older: update it
   to see this here."
3. "Needs a Full share: share again on winx-desktop." when the share's scope is below the call's. The mirror computes
   it with the registry's own `scope` (the same crate as home's); home's `403` still wins if they differ.
4. "Not on a mirrored workspace yet" for what batch 2 leaves out.

This replaces batch 1's blanket `on_remote` refusals. The banner's ⓘ lists what is left (editors,
deleting a checklist, drawing, attachments, Move to workspace, Copy and Paste of blocks). What only changes the mirror's own view (panning,
zooming, grid mode) is no deed and is never dimmed.

**The Share… dialog's words** follow what a share now does: **Control**: "Watch and work with blocks, cards,
checklists, Git and automations, open agents and hand them tasks, answer requests. No typing into terminals; agents it
opens can run commands." **Full**: "Also type and click into terminals, discard Git changes, delete branches, stashes
and automations. Anyone with the code can run commands on this computer."

#### What home's API gains

Additions to `/v1`; every field and tool below ships its lines in docs/api.md in the part that adds it.

**The state** (each a collection with its singular event name, `put` and `delete` as today):

```json
{ "notes": [{ "id": 7, "name": "Plan", "text": "…", "color": "blue", "cut": false }],
  "checklists": [{ "id": 12, "name": "Release", "items": [{ "id": 13, "text": "Tag it", "done": false }] }],
  "drawings": [{ "id": "w3", "drawings": [
      { "kind": "stroke", "color": "red", "width": 4, "highlight": false, "points": [[10.0, 20.5], [12.0, 21.0]] },
      { "kind": "arrow", "color": "foreground", "width": 2, "from": [0, 0], "to": [120, 40] }] }],
  "schedules": [{ "id": "s3", "by": "b7k2m", "to": "k9x1d", "text": "Check the deploy", "next": 1761562000,
                  "every": 600, "until": 1762166800 }] }
```

- **`notes`:** the notes a note block shows (its tabs), `text` cut at 256 KiB with `cut` true past it; all the
  texts of a state together take at most 8 MiB as JSON (a snapshot is one event, and a client takes 16 MiB of it), the
  notes past it sent with `cut` true and no text. A note block gains `"notes": {"tabs": [7, 9], "shown": 7}`.
- **`checklists`:** the checklists a checklist block shows. A checklist block gains `"checklist": 12` (null: none).
- **`drawings`:** one item per workspace (its id), its ink and shapes in paint order as the canvas file keeps them;
  at most 2000 drawings and 100,000 points, the oldest left out past either (*the designer's call*: drawings have no
  ids, and a workspace's list changes as a whole when a stroke is erased).
- **`schedules`:** agents' live schedules (`every` in seconds, null for once).
- **An agent** gains `task`, `{"id":"t7","text":"…","by":"b7k2m","since":1761560000}` (`text` as home keeps it, 2000
  characters; `by` the handing block, `"api"` for a task the API handed, null for one the token doesn't see) and `role`
  (spawn_agent's, or null).
- **A frame's `modes`** gains `mouse_moves` (the program asked for every motion).
- **`notice`** gains the automations' Notify me: `{"text":"Morning review: …","workspace":"w3","automation":1}`.
- **A limited token** sees a note when a note block of its workspaces shows it, a checklist likewise, the drawings of
  its workspaces, and a schedule whose `to` block it sees (`by` is null when it doesn't see that one). An automation's
  notice goes to the streams that see the rule's workspace (a rule for all workspaces: tokens for all workspaces).

**New control tools** (in the control server, so the Templum Agent has them too and the registry serves them; each
rule is the control server's danger, which gives the API's scope):

| Tool | Arguments | Does | Danger → scope |
|---|---|---|---|
| `delete_card` | `card` | Deletes it; Ctrl+Z at home brings it back | Ask own → control |
| `edit_board` | `board`, then `name` or `delete: true` | Renames it, or deletes it with its cards (Ctrl+Z brings them back) | Ask own for a delete with cards; – otherwise → control |
| `edit_column` | `board`; `column` (left out: a new column), `title`, `before` (a column of the board, or `"end"`), `delete: true` | Adds, renames, moves or deletes (with its cards) a column | Ask own for a delete with cards; – otherwise → control |
| `show_board` | `block` (a kanban), then `board` or `new` (a name, with `columns`) | Shows that board in the block, or a new board (always new, whatever names exist) | – → control |

**New control tools of kanban card 205:** `edit_checklist` (`block`, then `name`, or `item` with `before`: an item
id or `"end"`, or both; renames the checklist the block shows, blank reading "Checklist", or moves one of its items;
one step of its Ctrl+Z) and `new_checklist` (`block`, `name`: a new checklist in a checklist block that shows none,
refused on one that shows one). Danger –, so Control.

**Tools of kanban card 204** (the API's own, Control except `delete_note`, Full): `edit_note`, `new_note`, `open_note`,
`close_note`, `delete_note`, and `rev`, `changed` and `workspace` in the state's notes. See
[Notes on a mirror](#notes-on-a-mirror-kanban-card-204).

**Into the control server** from the block server: `edit_item` and `delete_item` (on any checklist block, as the
person's acts are), `edit_comment` and `delete_comment` (through the API, the person's: edits a comment `by: "you"`,
deletes any, as the card dialog does; through the control server, the Templum Agent's own). Danger: –, and Ask own
for `delete_item`.

**Grown arguments:**

- `add_block`, `open_agent`: `x`, `y`, the block's top-left in canvas units, within ±10⁷ (as `move_block`): it goes
  there, as a local add at a click does; without them, as today.
- `connect`: `label` (one line, at most 60 characters; blank removes). On two blocks already wired it renames their
  wire (one Ctrl+Z step), else wires them with it.
- `create_card`: `labels` (the six colors' names).
- `edit_card`: `title`, `description`, `labels` (`{"red": true, "blue": false}`: turned on, turned off; the rest
  keep), `remove_items` (item ids).
- `move_card`: `before` (a card of that column: it takes that card's place; left out: the end). A card of another
  column: "Card 41 isn't in Doing."
- `terminal_input`: `mouse`, 1 to 64 events,
  `{"kind":"press","button":"left","row":3,"col":10,"ctrl":false,"alt":false,"shift":false}` (`kind` `press`,
  `release` or `move`; `button` `left`, `middle`, `right`, `wheel_up`, `wheel_down`, or null for a move with none
  held; `row` and `col` in the terminal's grid, clamped), encoded at home in the program's mode at that moment
  (`Terminal::mouse`). One of `text`, `keys`, `mouse` per call; an event the program didn't ask for is dropped, and
  the answer counts those taken ("Sent 3 mouse events to b7k2m."). Full.

**The API's own new tools:** `cancel_schedule` (`schedule`; ends it as the Automations popup's Cancel does; control)
and `git` (below). The registry's lists: `schedule` joins the targets (its `to` block must be one the token sees);
every other new argument (`before`, `delete`, `new`, `labels`, `remove_items`, `label`, `mouse`, `op`, the `git`
operations' own) joins the arguments that name nothing, and the test that classifies every argument keeps passing.

##### The git tool

*The designer's call:* one API tool, `git`, with named operations. It is the API's own, not the control server's: the
Git popup is the person's, as automations are, and agents use git in their terminals.

- **Arguments:** `workspace`; `folder` (a folder at home, whole or relative to the workspace's; the repository is
  found from it); `op`; and the operation's own arguments.
- **One operation per job the Git popup runs today.** `templum_git::ops` defines them as one enum (`Op`, with serde)
  and runs them with one function (`ops::run(repo, op, trash)`), which the local popup's background jobs call too: the
  job bodies move there from `crates/templum/src/workspace/git_*.rs`, so the popup and the API can't drift, and the
  logic is tested headlessly where it belongs (`templum-git` is UI-free). templum-git's result types gain serde.

  | Scope | Operations |
  |---|---|
  | read | `find` (the repository of a folder), `look` (status, operation in progress, author), `diff` (working changes, staged or not, all or some paths), `history` (a page; a search by message, author or path), `show` (a commit's message, files and diff), `file_history`, `blame`, `branches`, `compare` (a branch's commits and diff against a base), `stashes`, `stash_diff`, `remotes`, `conflict` (a file's conflict regions), `trash` (Recently discarded) |
  | control | `init`, `set_author`, `stage`, `unstage`, `stage_all`, `stage_hunk`, `unstage_hunk`, `commit` (with `amend`), `undo_commit`, `fetch`, `pull`, `push`, `sync`, `publish`, `switch` (leaving changes stashed or bringing them), `new_branch`, `rename_branch`, `merge`, `checkout` (a commit), `tag`, `revert`, `stash`, `stash_apply`, `stash_pop`, `continue`, `resolve` (a conflict with one side), `mark_resolved`, `restore` (from the trash), `ignore`, `message` (Generate) |
  | full | `discard` (files, or all), `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop`, `abort` |

  The exact names and arguments are the planner's, from the jobs; the scope column is the rule: an operation that can
  lose work is Full.
- **The answer:** `{"text":"Committed a1b2c3d.","git":{…}}`, `git` the operation's result as templum-git types it.
  A refusal is `422` with git's first line as `message` and its whole stderr as `details` (at most 64 KiB; an
  addition to the error body, for `git` only). Git asking for a login it can't get says "git on winx-desktop asks for
  a login: run it in a terminal there."
- **At most 8 MiB per answer:** a diff past it keeps every file's row and drops the hunks of its largest files first,
  each marked `cut`; the panel shows "Too large to show from winx-desktop." for them.
- **Confined:** for a token limited to some workspaces, the repository's root and its main tree (git's answer,
  `Repo::main_tree`) must lie, canonical, inside a folder of its workspaces, with part 1's network-share rules; else
  `403 scope`, "That repository lies outside API's folder.". The paths an operation names are relative to the
  repository, never absolute, never through `..`, and must stay inside it. Arbitrary git arguments are never taken:
  only these operations. Git runs the repository's hooks (pre-commit, pre-push) as it would at home: the repository is
  the person's own, inside the workspace's folder.
- **One call at a time per repository** among the API's `git` calls (a queue per repository root); home's own popup
  keeps its queue, and git's lock refusal, if both act at once, is told in git's words.
- **`message`** runs home's commit-message agent as Generate does, and is refused while home's first-use question was
  never answered.
- **The audit log:** one line per call, `what` naming the operation (`git:push`); `look` calls are grouped per token
  and minute, as `terminal_input` is, since an open popup reads every 2 s.

#### The mirror's side

- **Answered calls:** the client's `Command::Call` takes any tool and its arguments and hands its answer (home's JSON,
  or the refusal with its status and sentence) to the UI through a one-shot the UI awaits in a task, never on the UI
  thread. Busy answers are retried as moves are (ten tries); a call made while the remote isn't live is refused at
  once, "Not connected."; a call whose attempt is dropped is refused "Couldn't reach the other computer." (batch 1).
  The answer's cap is 1 MiB, 8 MiB for `git` and `read_terminal`.
- **Calls wait in a bounded queue:** 64 per remote; past it a call is refused at once, "winx-desktop is slow to
  answer: try again."; a block's moves keep only the newest waiting (this fixes the minor "The calls channel of a
  remote is unbounded").
- **A slow connection:** `git`'s network operations and `message` go on a third call connection, with a 10-minute
  limit, so a long push never holds a card's move. To stay within home's 16 connections a token: one event stream, at
  most **11** terminal streams (was 12), typing, calls, slow calls, and one spare (*the designer's call*).
- **The state handed out cheaply:** taking a remote's newest state costs a few pointer copies, not its bytes (items
  and collections shared, copied only when changed), and the UI applies only the items that changed since the last
  take. This fixes the minor "The whole State is cloned on each wake of a mirror", which batch 2 makes pressing: it
  reads notes, boards and drawings that batch 1 ignored.
- **Home's tools** (`GET /v1/tools`) are read at each connect and kept with the remote, for the rule above.
- **Notices** reach the UI as `Update::Notice`.
- **The remote's copies** (`RemoteSpace`): boards, cards, checklists, notes, drawings, schedules and home's agents
  (`list_agents`), each cleaned and capped once where it enters, as batch 1 cleans names (one line through `clean_to`
  where it is a line; a text keeps its line breaks and loses every other control character):

  | What | At most |
  |---|---|
  | Notes | 1000 a remote; 256 KiB each |
  | Checklists | 1000 a remote; 2000 items each; 4000 characters an item |
  | Cards | a title of 300 characters, a description of 64 KiB, 500 items, the newest 200 comments of 16 KiB |
  | Drawings | home's caps, again; finite points within ±10⁷; widths 1–64; an unknown color is `foreground` |
  | Automations | 500 a remote; the texts of a rule 64 KiB |
  | Schedules | 200 a remote; their text 300 characters |
  | An agent's task | 2000 characters |
  | Home's agents (`list_agents`) and tools | 100 agents; 500 tools |

- **Never a relay, still:** the mirror's API, control server and block server never see a remote block, note, card,
  checklist, drawing, repository or rule (the mirror's state keeps showing the remote workspace as `remote` alone).
  Send to on a remote workspace lists its terminals only; Generate runs at home; block Copy and Paste stay off.

#### Constraints (batch 2)

- **Crates** (CLAUDE.md): `templum-git` gains serde on its types and `ops` (UI-free, tested headlessly);
  `templum-api` gains the state's new collections and fields, the `git` and `cancel_schedule` tools, `terminal_input`'s
  mouse, and the client's answered calls, slow connection, tools list and cheap state; `templum-terminal` gains
  `mouse_moves` in its modes and a viewer's scrollback the client fills; `templum-mcp` gains the new control tools and
  their dangers; `crates/templum` holds the glue (home's state and tools, the remote's copies, the views on a remote,
  the popups). No new dependency: serde is in the tree.
- **Performance:** every part that changes what the mirror paints runs `--bench-mirror` and pastes its line; the
  bar stays `p95_ms` ≤ 16.7. From part 2d the mirrored cockpit's kanban and note are drawn live (batch 1 drew them as
  cards), and from 2e home's canvas in `--bench-mirror` carries 200 strokes of 50 points (the ink bench's), drawn on
  the mirror. Part 2g runs `--bench-git`. Home builds its state as today (only while a stream is open, at most 4 times
  a second); a note's 256 KiB cap bounds what typing in a big note costs a stream.
- **Memory** grows only by the caps above, one terminal's history, and one Git answer per remote Git.
- **Tests** run on Linux, macOS and Windows, with no agent CLI, no real keyring, no network beyond loopback, no sleeps;
  `git` is on CI already (templum-git's tests run it); shell halves are `#[cfg(unix)]`. Each part adds its cases to the
  end-to-end test, `workspace/tests/mirror_end_to_end.rs`: one headless workspace both ends, over loopback, in one
  process.
- **Minors** this batch fixes leave docs/minors.md in the commit of their fix: no mouse input through the API; the
  automations' remote workspace; the whole state cloned on each wake; the unbounded calls channel; no right-click menu
  on a remote terminal; shortcuts that answer nothing; a remote terminal's effort; a request with no viewer; only 50
  requests listed; a request aged from the mirror.
- **Specs to update as batch 2 lands:** api.md, kanban.md, checklist.md, cockpit.md (notes and ink on a mirror),
  git.md, agents-view.md, automations.md (the rule, a remote's group), ask-inbox.md, connections.md (wires named
  through `connect`, Hand a task…), context-menus.md (a remote's menus), templum-agent.md (the new control tools and
  their danger), ARCHITECTURE.md, docs/api.md (each field and tool, and its Templum to Templum section's "What a mirror
  can't do yet"), this file's Status and As built.

#### Parts

Each part is usable and reviewable on its own and ends with its tests passing; they go in this order. 2f needs nothing
before it (home only) and may be built beside 2b–2e; 2g needs 2a and 2f.

- **2a, the mirror's footing.** Client: answered calls, the bounded queue with the newest move per block, the slow
  connection and 11 terminal streams, home's tools at each connect, notices. The cheap state and the UI applying only
  what changed. Glue: the one rule (replacing `on_remote`'s blanket refusals), notices as notifications with their
  cap, the undo mark, holds made general, the Share… dialog's words. Home: nothing. Tests: the client against its fake
  home (an answered call; a refused one in its words; the queue's cap; a quick call answered while a slow one runs; a
  home without a tool dims its act; a notice reaches `Update::Notice`); taking the state twice copies no item that
  didn't change; end to end: a watch set at home through the API (`wait_for` without `wait` on a shell) matches, and
  the mirror shows its notice.
- **2b, blocks, wires and agents on the canvas.** Home: `add_block` and `open_agent`'s `x`, `y`; `connect`'s
  `label`. Mirror: adding blocks, the Open dialog with home's agents, closing with the local question, wires (the
  tool, the menus, labels, Delete), Hand a task…, the menus and shortcuts on a remote, the new block's keyboard. Tests:
  home's (`x`, `y` bounds; a label on a wired pair is one Ctrl+Z step); end to end: a terminal added at a point on the
  mirror is at that point at home and gets the keyboard on the mirror; closed on the mirror, it is gone at home; two
  blocks wired and named on the mirror are wired and named at home; a task handed on the mirror to an agent (as
  `delegate`'s tests fake one) is typed at home with `[from API · …]` and its end's notice shows on the mirror;
  Ctrl+Z after an add stops at the mark.
- **2c, terminals: scrolling back and the mouse.** Home: `terminal_input`'s `mouse`, `modes.mouse_moves`. Client: the
  history fed into the viewer's scrollback, then a whole frame; mouse events in the typing queue, moves merged. Mirror:
  the wheel, Shift+PageUp and Find reading history; the mouse as `modes` says. Tests: templum-terminal
  (`mouse_moves`), the registry (`mouse` parses, at most 64, Full), the client (history then whole frame; merged
  moves); end to end (unix): 300 numbered lines printed at home, Find on the mirror finds line 1; a program at home
  that turns mouse reporting on and prints what it reads receives a click's bytes.
- **2d, boards, live and acted on.** Home: `delete_card`, `edit_board`, `edit_column`, `show_board`; `create_card`,
  `edit_card` and `move_card`'s new arguments; `edit_comment` and `delete_comment` into the control server; their
  dangers and the registry's lists. Mirror: the remote's boards and cards, the kanban body drawn from them, every act
  of [Boards](#boards-checklists-notes-ink), card holds. Tests: home's (each tool; a limited token's boards); the mirror
  headless (a board drawn from a state; a drop sends `move_card` with `before`; Done sends only what changed); end to
  end: a card dragged on the mirror lands at home in that column before that card; a card made on the mirror with
  labels and a comment; a column added and renamed; `boards.json` on the mirror unchanged, byte for byte;
  `--bench-mirror`.
- **2e, checklists, notes and ink, live.** Home: the state's `notes`, `checklists`, `drawings` and blocks' `notes`,
  `checklist`; `edit_item` and `delete_item` into the control server. Mirror: the three drawn live, checklist items
  acted on (held), notes read-only with Send to, ink above the blocks; `--bench-mirror` with home's 200 strokes. Tests:
  home's (a limited token sees no note shown only in another workspace; the drawings' caps); end to end: an item
  checked, added, edited and deleted on the mirror is at home; a note typed at home shows on the mirror; a stroke drawn
  at home shows; `notes.json` and `checklists.json` on the mirror unchanged.
- **2f, Git at home.** templum-git: serde on its types; `ops` with the popup's job bodies, which the popup's jobs now
  call (no change a person sees: the Git popup's tests pass unchanged). Home: the `git` tool (operations, scopes,
  confinement, the 8 MiB cut, `details`, the audit's grouping). Tests: templum-git (each operation on a temporary
  repository; a path through `..` refused; the cut drops the largest file's hunks first); the registry (scope per
  operation; an unknown one is `400`); the glue (a limited token's repository outside its folder is `403`; a Control
  token's `discard` is `403`; `look` on the workspace's repository answers its status).
- **2g, Git on the mirror.** The popup on a remote: where it runs (local, or a remote's `git`), the folder from home's
  state and its menu, the slow operations, Generate at home, the trash, Send to the remote's terminals, the dimming by
  scope, the memory-only state, Details. Tests: end to end with home's workspace folder a temporary repository: the
  mirror's popup shows a changed file; staged and committed on the mirror, it is in home's log; a branch made on the
  mirror is home's; on a Control share Discard is dimmed and Send to lists no local terminal. `--bench-git`.
- **2h, agents, automations and the inbox.** Home: the agent's `task` and `role`; `schedules` and `cancel_schedule`;
  the automations' notices. Mirror: the Agents popup with remote agents (Done its own); the Automations popup with
  home's rules and schedules; [the rule](#the-rule-homes-automations-run-only-at-home); the inbox's three fixes; the
  remote chip's effort. Tests: end to end: a task handed at home shows in the mirror's Agents details; a remote agent
  turns Done on the mirror when home's state goes from working to idle, and stops being Done when its block is focused
  on the mirror; a home rule shows in the mirror's popup, switched off there it is off at home, Run now from the mirror
  runs it at home (its Notify me notifies at home and its notice shows on the mirror); with the mirror's automation
  clock moved past a home rule's time nothing runs on the mirror and the mirror's `automations.json` holds no home
  rule; a new local rule's Where lists no remote workspace and defaults to a local one; a rule saved with a remote
  Where is Skipped with the rule's words; a local event rule on "any agent of a kind" doesn't fire when a remote agent
  of that kind ends its turn; a schedule at home is listed and cancelled from the mirror; a remote request is aged from
  home's `since`.

#### Left to the builder (batch 2)

The looks of the new dialogs (Hand a task…, Open on winx-desktop), groups and dimmed controls (theme tokens only); the
words of tooltips not quoted here; how the shared views reach a remote's copies and send their edits, as long as no
remote item ever enters the app's stores (a test proves the files unchanged); how holds are kept; the exact `Op`
names, arguments and result types, from the jobs; how the history reaches the viewer's scrollback; the constants' and
functions' names.

### Later batches

Sketched so batches 1 and 2 close no door, each its own card: editing notes from the mirror (done: kanban card 204); renaming a checklist,
reordering its items and showing another checklist in a block (done: kanban card 205); drawing; an editor's files
through the API; following home's camera; **sealed through tunnels**: Templum's pinned TLS carried inside one
WebSocket the tunnel passes, so not even the tunnel's provider sees the traffic (WebSocket is part 1's non-goal, so it
would be its own card). Batch 2's [Left out](#left-out-of-batch-2) lists the rest.

## Settled after the builder's read

A builder read this design before it was handed over; each answer is *the designer's call* and is written in above:

- **Windows' lazy roots and a proxy from the Start menu:** rustls-platform-verifier for the public way in, and a Proxy
  field beside the variables.
- **Dead connections holding the token's 16 after a laptop sleeps:** the mirror's session header, and `replaced`.
- **A drag as dozens of Ctrl+Z steps and calls at home:** moves are sent on release, one per block.
- **A stopped workspace:** the mirror says so and offers Start, through `show`.
- **Revoke on delete vs "deleted":** deleting doesn't revoke; `GET /v1/me` tells the mirror.
- **Typing filling the audit log:** `terminal_input` and `GET /v1/me` lines grouped per token and minute.

## Settled by the orchestrator

The designer left four questions for the person; the person asked not to be asked, so each is *the orchestrator's
call*, taking the designer's recommendation:

- **The office's proxy:** built as written (a typed or environment proxy, Basic auth, the company-proxy warning);
  NTLM and PAC proxies are out of batch 1 and named in docs/api.md.
- **A share's default scope:** Full, since typing needs it, with the warning that the code runs commands at home;
  Control one click away.
- **A share's default expiry:** 30 days, as tokens; the mirror warns 3 days before.
- **The tunnel's provider sees the traffic** in the public way in: accepted for batch 1, Cloudflare Access's service
  token supported, sealing through tunnels a later card.

## Left to the builder

The looks of the banner, the marks, the cards and the dialogs (theme tokens only); words of tooltips not quoted here;
the threads' and channels' layout inside the rules above; how a workspace is marked remote and the "not mirrored yet"
`Body` arm is named; the SSE reader's shape; how the scaled cells are computed within the font steps; the JSON
names of the index entry; the constants' and functions' names; which local tools need the remote guard, as long as the
end to end test proves none sees a remote block.

## As built

### Batch 1a (home's side)

Plan: [2026-10-25-t2t-batch-1a-home.md](../plans/2026-10-25-t2t-batch-1a-home.md). What the builder chose or changed:

- **Key names** are parsed once, in `templum_terminal::named_key` (`NamedKey`, owned; `.input()` is the `KeyInput`), for
  `send_keys` and `terminal_input`. `ctrl+1` and `ctrl+é` were refused before and are names now (the program gets
  what `encode_key` makes of them: nothing for `ctrl+1`; for `ctrl+é` nothing in legacy mode and `CSI 233;5u` in
  the kitty protocol). `alt+X` and `alt+shift+x` send `ESC X`: only Ctrl drops the typed character.
- **`modes`** ride on `Screen` (`typing` always false) and on every frame; a change of them alone is a frame with no
  rows. The OpenAPI `Frame` schema leaves `modes` optional (older homes send none).
- **A share** is `Token::share` (`Share { workspace, way, address }`) in `api.json`; `Store::tunnel` remembers the
  last tunnel address. `Store::mint_share` makes the token and answers its text once.
- **A limited token's view of the state** now holds its own share, so the event stream no longer shares one
  comparison among limited tokens of the same workspaces: the key is the token as well.
- **`share_workspace`** leaves `expires` optional: null or left out is never (the Share… dialog's default is 30 days).
  A Directly address with a scheme, or none, is refused "Give Directly's address as host:port."; the code is tried
  through `code::decode` before the token is made, so a code the other Templum cannot read is never made.
- **`revoke_share`**'s registry scope is Read (nominally "full, or the share itself"); the glue asks Full of a token
  for all workspaces to revoke another's.
- **The mirror's session:** a replacing event stream is looked for before the token's 16-connection check, so stale
  connections can't keep the new one out. The replaced streams get `event: replaced` as their last frame
  (`Cell::end_with`; `Cell::end` drops what is queued); their connections close with the body, and the connections
  with nothing streaming are aborted. The streaming ones are marked replaced at once, so they no longer count against
  the 16, and are cut after 2 s if their body has not flushed (a sleeping laptop's full send buffer would keep them
  for minutes). A request without `Templum-Mirror` replaces nothing. Each replacement writes a `mirror_replaced`
  line in the audit log.
- **A share revoking itself** gets its answer: the sweep that drops a dead token's connections spares only the
  `revoke_share` call being answered (its next request is `401`; its streams end through the hub). Every other call of
  that token, a waiting `delegate` or `wait_for` included, is cut with the token, and so is an event stream still
  opening when the revoke lands.
- **Replaced connections are capped:** a token's replaced connections that are still alive (a streaming one has 2 s to
  flush `replaced`) are at most 16; past that the oldest are aborted at once, so a mirror in a reconnect loop can't
  fill the 64 connections of the server.
- **The audit log** groups `terminal_input` and `GET /v1/me` as the noise is grouped (key: address, call, token, status), into
  `audit.log`; the test that expected two `terminal_input` lines now expects one. A mirror's grouped line that the full noise half gives up is counted in the `lost` line, like any call.
- **The Edit… dialog** keeps **Share…** below the rows too, so one more computer can be shared with. The Share…
  dialog makes its own footer: Make code is disabled while the API doesn't fit or the name is empty.
- **`Server::mirrored`** says the address of a token's oldest connection with an event stream; the stream's opening
  and its end tell the UI thread (`CLIENTS`), which draws the sidebar's mark from the server.

### Batch 1b (the mirror's client)

Plan: [2026-10-25-t2t-batch-1b-client.md](../plans/2026-10-25-t2t-batch-1b-client.md). UI-free, in `templum-api`'s
`client/`; nothing draws (plan 1c wires it to the canvas). What the builder chose or changed:

- **Modules:** `keys.rs` (`Keys`, `Secrets`, `Kept`: keyring, file or memory), `trust.rs` (`Pinned`, `Checked`, the
  marks), `dial.rs` (TCP, the proxy's `CONNECT`, TLS), `http.rs` (calls, streams, `failure`), `sse.rs` (the event
  reader), `frames.rs` (a frame to a viewer's bytes), `remote.rs` (one remote's life) and `mod.rs` (the types and
  `Mirror`). `templum-terminal` gained `Feeder` (feed and resize a viewer from any thread) and
  `SharedTerm::grid_size`; `State::apply` (in `model.rs`) applies the stream's events to a mirror's copy.
- **The trust marks:** a refusal in the handshake is a `rustls::Error::General` holding `templum:pin`, `:added`,
  `:expired`, `:name`, `:issuer`, `:issuer-alone` or `:other`; `dial` reads the mark back out of the I/O error and
  makes it a `Why`. A signature that is not the key's of the certificate shown (`InvalidCertificate`) is
  `Certificate(Other)` on the public way in only; on the pinned one it is `PinChanged` (see Review fixes to 1b).
- **One coordinator per remote, no `select!`:** the coordinator task never awaits the network. Its sub-tasks (calls,
  events, typing, one per terminal) send it messages tagged with the attempt they belong to; every attempt moves the
  epoch, so what an ended attempt still says is ignored, and dropping the attempt aborts its tasks, which drops their
  connections (a reconnect leaves none behind). The newest state lives in the shared slot and events are applied in
  place (`State::apply` under the lock), so a flood of deltas copies nothing; `Mirror::state` copies it once when the
  UI thread takes it.
- **A stream home ended is looked at once before it is called "reconnecting":** a revoke, a deleted workspace and a
  restarted home all end the stream the same way, so the mirror tries again at once (not twice in a row if the new
  stream ended within a second): `401` is then "revoked" with no "Reconnecting" in between, and a home that is back
  is live without a flicker. A stream that goes silent (45 s) or a failed look is `Reconnecting { attempt: 1 }` and
  the backoff. `attempt` counts consecutive failures; `live_at` is the last time the remote was live.
- **`Limits`** (`silence` 45 s, `me_every` 15 s, `live_resets` 60 s, `typing_patience` 5 s) is `#[doc(hidden)] pub` for the tests
  (`Mirror::start_with`).
- **Frames are fed as they are read:** nothing is held between frames (the spec's "one pending frame" rule holds
  trivially). The first frame of a stream says all three modes (set and reset), so a stream that reopens leaves nothing
  of the old one in the viewer; later frames say only what changed. A title is cut to 256 characters after its control
  characters are dropped. A frame of size 0 is refused like one past 2000 × 1000.
- **Typing:** one call in flight on the typing connection; the block that has waited longest goes next, with up to
  `INPUT_KEYS` of its waiting keys or one paste, in order. A paste past `tools::MAX_TEXT`, or past it in all the
  pastes waiting for one block, is refused or dropped.
- **Calls** have a 30 s limit; one that fails to reach home is "Couldn't reach the other computer." (`Refused`).
- **Forget** runs in the remote's own task (it revokes with a connection of its own, deletes the key on the blocking
  pool, ends); `Mirror::forget` drops what the remote had said and every update after it.
- **A key that can't be saved** (`KeySource::Save` into a file that can't be written) is `Update::Refused { about:
  Show, sentence }` and `Cut(Keyring)`.
- **Public helpers:** `client::{proxy_of, proxy_from_env}` are public for the Connect dialog (plan 1d).

#### Review fixes to 1b

What the review of 1b changed (decisions the builder took while the person was away):

- **Where the key may go** (above): `Secrets` gained `url`, `trust`, `trust_added` and `proxy`; the client compares
  them with the `Remote`'s before it dials (`Remote::binds`, in `prepare`) and refuses the way in that does not fit the
  address (`fits`). The proxy's address is kept in the key too, because the proxy's login goes to it. A Loopback
  remote never uses a proxy.
- **Typing is paced and patient:** `PACE` (60 ms) between the starts of two typing calls, `BUSY_TRIES` (10) of "busy"
  answers retried inside the typing and calls tasks (so the queue is untouched and nothing is sent after the attempt is
  gone: the tasks are aborted with it). `Dropped` is said once per queue until it falls below 1024 and has no paste
  waiting.
- **Typing while not live** (including the look after a stream's end, when the status still says Live) answers
  `Refused { Keys(block), "Not connected: what you type isn't sent." }`, once per block until live again.
- **A stream "home ended"** is a body that finished or a connection closed before its body did (`UnexpectedEof`, which
  is how home cuts a revoked share); a reset or bytes that aren't HTTP are ordinary failures. A terminal stream that
  hears 429 or 503 `busy` reopens after `max(1 s, Retry-After)`. **Try now** also restarts an attempt that is still
  opening, and the body of a non-200 stream answer is read under the 30 s call limit.
- **A proxy's 502, 503 and 504** to `CONNECT` are `Down` (home is unreachable, retried); other statuses stop.
- **Pinned and a forged handshake:** a signature the pinned certificate's key didn't make (the pin matched, the
  signature did not) is "the pin changed", from the verifier itself.
- **The system verifier's verdict is remembered** (8 chains, a name and the hour each) in `Checked`, because
  Windows' and macOS's verifiers may fetch and the client has one thread; when the platform says only "other" (macOS),
  Mozilla's list reads the same chain, so "it expired" and "share Directly instead" still show.
- **Kitty keys:** alacritty keeps one stack per screen and a switch of screens swaps them, so the painter does not push
  and pop: on the first frame, on a change of screen and on a change of kitty, it empties the active stack (`CSI < 99
  u`) and pushes again (`CSI > 1 u`) when kitty is on. The first frame always says the title, even an empty one.
- **Home gone wrong:** a row is clipped at the frame's columns, a cursor is kept on the grid, and each collection of
  the state holds 10 000 items at most (`ApplyError::TooLarge`: the stream drops and reopens). A snapshot is parsed
  before the lock is taken. Home's `frame` sends a whole frame when the screen (`?1049`) switched.

#### Second-round review fixes to 1b

What the second review of 1b changed (decisions the builder took while the person was away):

- **A full row with zero-width marks is no longer cut short.** Home's runs carry each cell's zero-width marks
  (combining accents, `VS16` as in `⚠️`, ZWJ) in the text, so counting characters against the columns cut real
  characters off the end of a full row (Claude Code's border `│` on a line with `✔️`). The painter counts nothing:
  its first frame sends `CSI ?7l` (no autowrap), and the viewer itself clips an over-long row at the right margin
  (alacritty drops the characters past the last column when autowrap is off). Only the painter writes to a viewer,
  and every row it writes starts with a cursor move, so nothing else relied on autowrap; the clamp of sizes (2000 ×
  1000) and of the cursor is unchanged. The round-trip test holds a full row with a combining accent and a row of
  `⚠️✔️`.
- **`Retry-After` is held to 30 s** (`http.rs`): a huge number overflowed `Instant::now() + …` and ended the typing
  task. Typing gives up after ten busy answers (the tenth is the last; it used to be the eleventh) or 5 s of waiting
  in all (`Limits::typing_patience`), and the keys are `Dropped`, never sent late. The calls task gives up on the tenth
  answer too; a move or an answer waits up to 10 × 30 s at most, since it is still wanted when it lands.
- **The public verifier remembers only what was fine,** and not past the leaf's `notAfter` (read from the DER; a
  certificate whose date can't be read is not remembered). A refusal is asked again at the next handshake, so a person
  who installs the authority or sets the clock is heard at once; **Try now** from Idle also forgets every verdict
  (`Checked::forget`). What remains: for up to the clock hour (or until `notAfter`) a chain judged fine stays fine
  although it was revoked, or its authority taken away, in the meantime, unless the person presses Try now; and a
  refused chain costs the system's verifier a call at each handshake, which the reconnect backoff paces. The system's
  roots themselves are read when the verifiers are built (once per start of the remote), as before.
- **The mirrored state is also capped in bytes:** the sum of each kept item's event data length (a put weighs its
  item in place of the old one, a delete gives it back, a snapshot starts over) may not pass 64 MiB (`WEIGHT_MOST`,
  4 × `sse::CAP`); past it is `TooLarge` (the stream drops and reopens). Every event is parsed (`State::parse`) before
  the lock the UI thread takes; under the lock only the quick `Change` it returns is made.
- **A terminal home refused stays refused** when the terminals shown are only put in another order (`blocked` is
  cleared when the set's members change, not on every `Terminals`).
- **Tests that waited on the wall clock hold and release instead:** typing fast sends its 200 keys while the first call
  is held, then releases it and expects the 199 waiting keys in one call; the pace between two calls is checked at half
  of `PACE`. `Dropped` after a queue is only partly drained (one call released, 768 keys waiting) is said again when it
  overflows again.

#### Third-round review fixes to 1b

What the third review of 1b changed (decisions the builder took while the person was away):

- **The event stream is held by its socket, not by memory.** `events_task` used to turn every chunk into thousands of
  `Msg::Event`s on the coordinator's unbounded inbox, faster than the coordinator took them, so a home that wrote
  small valid events (`block` deletes of ids that don't exist pass every cap) grew the mirror by hundreds of MB in
  seconds. Now each event takes a permit of a per-attempt semaphore (`EVENTS_WAITING_MOST`, 64) before it is sent and
  carries it in `Msg::Event`; the permit goes back when the coordinator handles the event or drops it (an ended
  attempt's). With 64 waiting, `events_task` stops reading, the socket's buffers fill and home's writes block. Chosen
  over applying events in the task itself because `last_id`, `Replaced` and the failures stay with the coordinator.
  The other channels need nothing: terminal frames are fed in the stream task itself (`Feeder::feed` is synchronous,
  so the socket is the queue), the typing connection has one batch in flight, and the coordinator's other messages
  come once per call or attempt. The calls channel is unbounded but only the person's commands feed it (a minor).
- **Typing says it when a call fails,** not only when home stayed busy: a `Down` (a timeout, a closed connection) or a
  `TooLarge` answer to a typing call is `Dropped(block)` too, with what waited behind it. The call is not made again
  (`terminal_input` is not idempotent).
- **`Limits::typing_patience`** (5 s) is the "5 s in total" of typing, a field so a test can shorten it.
- **A state's weight counts what it keeps:** an event of a kind `State::parse` doesn't keep by id
  (`State::keeps_items`) weighs nothing and adds nothing to the map, and every kept item costs its data and 64 bytes
  (`ITEM_OVERHEAD`) for its keys and place, so a million tiny items trip the 64 MiB sooner than their data would.
- **A stream follows its viewer:** `sync_terminals` remembers which viewer each stream feeds (`Feeder::same_viewer`)
  and opens the stream again when the terminals shown name another viewer for the block (a block that was closed and
  shown again), so the old viewer is not fed from a stream nobody looks at.

#### Fourth-round review fixes to 1b

What the fourth review of 1b changed (decisions the builder took while the person was away):

- **A put's item must carry the event's id.** The client's weights key on the event's `data.id` while `Edit::run` looks
  items up by their own id, so a hostile home could send `{"op":"put","id":"x","value":{"id":"y",…1 MiB}}` over and
  over: the weight stayed at one item (key `x`), the state pushed a new item each time (200 MiB after 200 puts), and a
  delete of `x` freed weight without removing anything. `Edit::read` now refuses, before any lock, a put whose item's
  id (canonical: a number as it is printed, so `"05"` for 5 is refused; the `usage` event must say `usage`) is not the
  event's: `Malformed`, nothing kept. Tests: `model::tests::a_put_whose_item_has_another_id_is_refused` and
  `client::tests::puts_with_mismatched_ids_cannot_get_past_the_weight_cap` (a bounded loop through `Shared::apply`).
- **The event stream's `event:` and `id:` are short.** `held` counted only the line and the data, so an event could
  hold two more `CAP`s in its `event` and `id` fields (47 MiB seen), and a huge `id` became `last_id` and was sent as
  `Last-Event-ID` on every reconnect. Both are now capped at `sse::NAME_CAP` (256 bytes, home's are short); a longer
  one is `TooLarge`, which ends the stream as `CAP` does. Test: `client::sse::tests::a_long_event_name_or_id_is_too_large`.
- **Forget with revoke stops reading first.** `Life::forget` awaited the connection and the revoke call (up to 3 × 10 s
  plus 30 s) before `drop_attempt()`, while `events_task` kept reading up to 64 events (a hostile home: about 3 GiB). It
  now clones `attempt.net`, calls `drop_attempt()`, then revokes. Test (with a fake home that holds the revoke,
  `Fake::hold_revoke`): `forget_with_revoke_drops_the_streams_before_the_revoke_is_answered`.
- Not done: wrapping the client tests that `results.recv().await` without a timeout in `tokio::time::timeout` (optional
  in the review; they hang only on a regression).

### Batch 1c (the remote workspace on the canvas)

Plan: [2026-10-25-t2t-batch-1c-canvas.md](../plans/2026-10-25-t2t-batch-1c-canvas.md). View-only: connecting, the
blocks, the live terminals, the banner and every state of the connection. What the builder chose or changed:

- **A remote workspace** is a `Space` with `remote: Some(Box<RemoteSpace>)` (`workspace/remote.rs`: the entry, the
  status, `me`, home's workspace, the round trip, `homes`, the camera's state). It is in `workspace/index.json`
  (`session::RemoteEntry`: `url`, `way`, `pin`, `home`, `entry`, `key` as `keyring`/`file`/`none`,
  `trust_added_authority`, `access_id`, `proxy`, `camera`) and never has an `<id>.json`: `snapshot` and
  `save_canvas_now` skip it, `delete_space` sets nothing aside. `save_canvas_now` also writes the index when a remote
  exists (a pan changes only the camera). A value of `remote` that doesn't read is None.
- **Forgetting the last workspace:** when the remote is the only workspace left, Forget makes a new local one first
  (`delete_space_as`), so it always works; a local workspace that is the last one still stays.
- **Ids:** `r<n>`, the smallest unused (`Workspace::remote_id`). Home's block ids live only in `RemoteSpace::homes`;
  `Space::uids` stays empty, and `Space::terminals()` skips remote viewers, so the Agents view, the control server, the
  API's terminals, wires and the spaces screen's counts never see one (`Space::terminal(id)` still finds a pane).
- **The client in the workspace:** `Workspace::mirror` starts `Mirror` at the first need, its wake sends on a futures
  channel, and a task drains it once per wake (`drain_mirror`: statuses, `me`, latency, where the key was kept, then
  each remote's newest state into `apply_remote`). `connect_remotes` runs from a task spawned at the end of
  `Workspace::with`, only when a remote exists, so a test sets `ws.keys` to a mock store first. `keys` is
  `Keys::system(SERVICE or SERVICE_DEV)` outside tests; under `cfg(test)` and without a data folder it is
  `without_keyring`, so no test reaches a real keyring.
- **Connect:** `Connecting { code, access, proxy, file }` (the proxy is a `Key`, so a login in it never prints)
  through `Workspace::connect_code` makes a hidden remote and `KeySource::Save`; the index leaves it out until it is
  live. The dialog (`connect_dialog.rs`) shows a failure in its own words (`failure_words`: the table's for a cut,
  "Couldn't reach winx-desktop." for a remote that can't be reached), so a Connect that can't reach home leaves
  nothing (the remote is forgotten, key included) instead of a remote that keeps reconnecting. After Connect the
  code and the Client Secret leave their fields and the proxy keeps only its address. Only the first status that
  settles it answers (`connect_moved` returns once the remote is no longer pending). `connect_result` returns what
  `drain_mirror` stored (forgetting needs `&mut`). On Live: the
  workspace is shown, the index written, and the clipboard emptied if it still holds the code. The keyring sentence
  names the platform's store. An `AddedAuthority` cut offers **Don't connect** (forget) and **Connect anyway**.
- **Connect anyway needed the client:** `Remote::binds` compares `trust_added` with the key's, so reconnecting with
  `KeySource::Kept` would be cut as a keyring failure. `Command::TrustAdded` makes the client re-save the secrets it
  holds with `trust_added: true` (same place; memory if the keyring refuses) and start again. A new code starts with
  `trust_added_authority` false.
- **Blocks:** `Body::Mirrored(Mirrored { kind, title })` is the one arm for a kind the mirror doesn't draw (a card
  with the kind's icon, `Box` and "kind · title" for a kind it doesn't know); a terminal is a viewer with no
  scrollback and `Pane::remote: Some(RemotePane { block, title, agent, read_only })` (the block's title is home's, as
  the viewer's own follows home's only while it is streamed). A new viewer starts at the rectangle's grid, capped at
  80 × 24 (`DEFAULT_COLS` × `DEFAULT_ROWS`): home's rectangle is not trusted, and its first frame sets the real grid. Blocks are added straight to the
  canvas, with no undo step, focus or pop-in. The first state sets the camera (the state's `view` when it is the
  entry's workspace, else `Camera::fit`); the index keeps the camera only once a state placed it or the person moved
  it. Home's icon and color show on the tile but aren't saved as the person's look. A remote pane's dot and chips
  come from `RemotePane::agent` (`Workspace::doing`); `pane.usage` holds home's usage. Home's rectangle "unless a
  move is held" is plan 1d's.
- **What a mirror takes from home** (`mirror_blocks`, `tidy`): at most `MOST_BLOCKS` (500) blocks, the front ones by z
  (what home no longer sends goes, as any block home dropped), and at most `MOST_WIRES` (200) wires, each once however
  many times home lists it (each is drawn every frame against every block; labels are kept only for those); a rectangle goes through `restorable_rect` (finite,
  size clamped like a restored canvas') and one beyond ±1e7 is left out, so the canvas's union can't overflow and the
  camera stays finite. Home's view is taken as the first camera only within the same ±1e7 (else the canvas fits its
  blocks), and the index neither keeps nor restores a remote's camera beyond it, so the dot grid has an end. Home's
  strings (workspace and block names, kinds, wire labels, the agent's words, `me.computer`, the error sentence of a
  refused call, the type a wrong server answers with) are cleaned once where they enter: one line (`bus::clean_to`), 60 characters, 200 for an agent's
  question and a refusal's toast. Home's workspace color is kept only when it is one of `space_dialog::COLORS`, and
  the id of home's first workspace (written to index.json) only when it is at most 64 bytes with no control
  characters (`tidy_me`). The agent's usage chip keeps home's context percent only within 0 to 100 and its cost only
  within 0 to 1,000,000 dollars (`session_of`). A remote's saved camera beyond ±1e7 doesn't count as placed, so the
  first fit frames the blocks. `RemoteSpace::names` is `homes` the other way round for `home_of`.
- **Terminals:** `streamed` picks at most 12 (`MOST_STREAMS`), the focused first, then the nearest to the canvas's
  center; `Workspace::stream_terminals` runs while the canvas is painted, at most every `STREAM_EVERY` (100 ms), and
  sends `Command::Terminals` only when the set changed; a choice that waits for its time returns when it is due and
  one timer (`stream_at`) repaints then, instead of a frame per frame. A hidden or not-live remote gets an empty set at once; below
  Full detail only the focused terminal streams. `fit_zoom(zoom, content, grid, cell)` is the largest of
  `ZOOM_STEPS` not above `zoom × min(content.w / (cols × cell.w), content.h / (rows × cell.h))`, else the smallest;
  the pointer maps through the same step. `Pane::fit` does nothing for a remote pane and `Pane::grid_size()` reads the
  grid's own size (`Terminal::grid_size` goes stale after a `Feeder::resize`). The wheel says "Only the live screen is
  mirrored for now." once per terminal; "Read-only: a subagent's transcript" is a small overlay at the body's bottom.
- **The banner** (`mirror.rs`, `banner_words`): the spec's sentences and buttons, as plain text (the buttons carry the
  bold actions); "Try now" while reconnecting, "Try again" otherwise. Not live: a veil, grey dots, a dimmed tile. The
  tile has a `Globe` mark; its menu is Open, Disconnect or Connect, Forget…. **Connect** on a remote that never started
  this run (no key) opens the Connect dialog; **Paste a new code…** does too until plan 1d. The "ends in N days" and
  the yellow round trip past 250 ms (its tooltip says the real round trip) are in. The banner's ⓘ
  (`not_mirrored_tip`) lists notes, checklists, editors and boards, ink and shapes, and a terminal's scrollback, and says only the front
  500 blocks (and 200 wires) are shown. `Certificate(Other)` says "it couldn't be checked".
- **Not yet:** `Workspace::on_remote` guards adding blocks, closing, wires, the drawing tools, grid mode, Move to
  workspace (and a mirror is never a place to move a local block to: `Space::takes_blocks`), editors (at
  `add_editor_at`, which Settings' Open file and the Git changes list reach too), notes and checklists opened by id,
  wire labels, the shapes pill, Copy and Paste, the Git
  popup, Ctrl+click on a file path in a mirrored terminal (only its URLs open) (Stop was refused already); the toolbar's buttons are dimmed with
  `NOT_YET`. The menus (canvas, block, wire, ink) don't open on a remote (a notification says `NOT_YET`: menu items have no tooltip) and the
  keyboard shortcuts do nothing silently. A drawing tool picked on the workspace left goes back to Select when a
  remote is shown (`switch_to`), and `begin_ink` does nothing on one.
- **Never a relay:** a tool whose `workspace` argument names a remote (id or name) is refused "API is a mirror of
  winx-desktop: act on it there." in `space_of` (a name that a local workspace also has means the local one) (`show`, `add_block_in` and `share_workspace` too); `overview` and the
  control instructions list it as "API (mirror of winx-desktop)" with no blocks. Home's computer name is cleaned
  (`clean_title`) as it comes in: it is text from another computer, in an agent's instructions.
- **The mirror's API state:** `RemoteView { computer, host, way, status, latency_ms }` as `WorkspaceView::remote`
  (left out for a local workspace; OpenAPI schema `Remote`); its blocks are never in the state. docs/api.md has the
  field's line; plan 1d writes the section for people.
- **Not done:** `--bench-mirror` (a later plan).

### Batch 1d (acting from the mirror)

Plan: [2026-10-25-t2t-batch-1d-act.md](../plans/2026-10-25-t2t-batch-1d-act.md). Typing, moving and resizing, home's
requests, Edit… and Paste a new code…, the end-to-end test, `--bench-mirror` and the docs. What the builder chose or
changed:

- **Typing:** `templum_terminal::key_name` is the inverse of `named_key` (`text_names` for the text path): a printable
  character (or AltGr's) is itself, a space is `space`, any other key is its modifiers (`ctrl+`, `alt+`, `shift+`) and
  its name; an unnamed key (F13, a media key) isn't sent. `on_key_down` of a focused remote terminal sends the name of a
  key that types no text and stops it there; printable text (`templum_terminal::typed_text`: a character with no
  Ctrl or Alt, or AltGr's) is left to the text path as a local terminal's, which sends `text_names` in
  `replace_text_in_range` and ends a dead key's mark there (stopping it would leave "´" drawn); the Paste command
  (`paste_clipboard`, now with the window) sends a paste. Nothing is typed into the viewer itself, `person_keyed` and
  the agent's draft bookkeeping don't run for it. `typing_at` tells once per terminal, with `RemoteSpace::told`: not
  live ("Not connected: what you type isn't sent.", again after it was live in between), a share that doesn't type
  ("Typing needs a Full share: share again on winx-desktop."), and a paste over 256 KiB each time. The client's own
  refusal and `Dropped` go through the same `warn`. Text is a key press at home, so a program that asked the kitty
  protocol for every key as an escape code gets it as a code (locally it would be text).
- **Updates have a door of their own:** `Workspace::heard(id, update)` takes one client update, so tests can say
  what the client would (a refusal, `Dropped`); `drain_mirror` calls it. A `#[cfg(test)] Workspace::said` keeps what
  mirrored workspaces told the person (notifications can't be read back).
- **Moving and resizing:** a release (`end_placing`) and an undo or redo (`place`) call `remote_moved(ids, now)`, which
  sends one `move_block` per block whose rectangle differs from what home has *as far as the mirror knows*: a move sent
  and not shown yet counts as home's, so moving a block back before home's state came still sends. `held` keeps the
  rectangle sent for `HOLD` (2 s); `rect_shown` (in `mirror_blocks`) shows it until home's rectangle is the same (within
  0.5) or the time passes; `expire_holds` (each frame, repainting at the due time through `stream_at`) hands the block
  back to home when no state comes. A block in the person's hand (`placed`) keeps following the pointer when home's
  state arrives, and its hold isn't expired under it (`expire_holds`; its release holds it again). A refusal (`About::Move`) takes the block back to home's last rectangle. Not live: a drag, a resize and
  Ctrl+Z do nothing (`remote_frozen`), and a release that got through sends nothing and goes back.
- **Requests:** `remote_requests` (from `apply_remote`, with the state's requests of home's workspace) makes a wait
  marked `remote` (`RemoteAsk`: the workspace, home's request id, its scope, `sent`, `quiet`) for each new one, through
  `wait_as`, with the viewer of its block as its asker (`Workspace::asker`, since a viewer is no caller of this
  computer's tools); one home no longer lists is withdrawn with no reply. Home's request ids are `r<key>` with the wait key
  counting from the Unix time in milliseconds at home's start (`Workspace::next_wait`), so an id never names another
  request after home restarts, and a wait is home's request when the ids are equal (no clock or text to compare).
  A press in the gap between a reconnect and home's first state answers an id home doesn't know: `not_found`, a
  refusal that shows the request again. A new code (`replace_code`) withdraws every wait of the remote. The person is
  told of a request once per id per connection (`RemoteSpace::requests_told`, reset by a new code), and at most 3
  notifications in 60 s per remote: the rest of a burst is one "N more requests ask you" notification
  (`tell_folded`, "1 more request asks you" for one), and at most one of those in 60 s (`RemoteSpace::folded_at`),
  so a home that makes a request in every state brings at most 4 notices a minute; the requests are all in the inbox
  either way. What home sends is capped (50 requests, 10
  questions, 30 options, lines of 300 characters, descriptions and previews of 4,000, a plan of 64 KiB, buttons of 24)
  and cleaned. `Approved::Remote { line, yes, no }` carries home's summary and its buttons' words (`Approved::buttons`
  answers `String`s). Answering sends `answer_request` (`remote_answer`): the wait is hidden until home drops it, or
  shows again on a refusal. The client refuses every call still in flight when its attempt is dropped (`Attempt::calling`,
  "Couldn't reach the other computer."; also a stream home ended cleanly, which looks again without changing the
  status); if home did hear it, its next state drops the request. The mirror's API (`request_views`) skips remote waits, so no local tool lists or answers
  one, and none of the agent-facing bookkeeping (`needs_you`) runs for them. A request whose block has no viewer here
  is skipped (a minor). A request that can't be answered now (`remote_blocked`: not live, or the share's scope is
  below the request's) has its foot dimmed and covered, with the reason as a tooltip.
- **Edit… and a new code:** a remote's Edit… is `space_dialog`'s (a `remote` flag): home's name, Color and Icon, the
  Connection rows and **Paste a new code…**; Save keeps the look (`edit_remote_look`; none of either means home's look, and Save with the look as it opened, home's,
  leaves it home's).
  The remote tile's menu gains Edit…. Paste a new code… (and Connect on a remote that never started this run) is the
  Connect dialog for that remote (`connect_dialog::open_for`); `replace_code` gives the remote the new code's address,
  way in, pin, Access ID and proxy, an entry of its own and `trust_added_authority` false, connects with
  `KeySource::Save`, and drops the old keys (`Mirror::drop_key`; `RemoteSpace::old_keys`, one per code replaced, even one not yet kept) once
  the newest is `Kept`, or with the remote when it is forgotten. `Keys` remembers the entries it deleted this run and
  refuses to save one: the save of a code replaced again before it finished can't bring its key back. The dialog
  closes at once; how it goes shows in the banner. Its **More…** opens with the remote's Access Client ID and proxy
  address; the secrets are never shown, and what the dialog leaves empty of them (the Client Secret for the same Client
  ID and address, the proxy's login for the same proxy) stays: `KeySource::Save { from: Some(Inherit) }` makes the
  client read them from the old key and save them with the new one, so they don't pass through the UI and the old key
  is deleted only after. The Client Secret is carried over only when the new code's scheme, host and port equal the
  old key's own (`Secrets::url`, bound to it since 1b, not the index entry); otherwise it is left out and the banner
  says "The address changed: type the Client Secret again in More…". If the old key can't be read, the client stops
  before saving (`Why::Keyring`, no `Kept`, so no key is dropped; Try again reads it again), since it may hold the
  only copy of the secret. A code pasted again before the last is kept reads from the same key the last one's Save read from
  (`holder`, `RemoteSpace::inherits`, cleared at `Kept`), the entry that holds the secret, never an entry that may not
  have been written; a connection kept nowhere has none to inherit.
  The Access banner says "Add a service token in Paste a new code… ▸ More…" and offers that button.
- **The banner:** the round trip is yellow past 250 ms as 1c built it, and its tooltip says the real round trip, not
  one rounded to tens (the plan's `Banner::slow` wasn't needed). Under a day the share's end is told in hours or minutes
  ("ends in 5 h: share again on winx-desktop"), else in days rounded up. The ⓘ is `NOT_MIRRORED` plus the 500-block and
  200-wire caps.
- **The end-to-end test** (`workspace/tests/mirror_end_to_end.rs`): one headless workspace is both ends, as the plan
  said. The shell half (`sh` echoes what the mirror types) is `#[cfg(unix)]`.
- **`--bench-mirror`:** one Templum as both ends in a temporary data folder (`templum-bench-mirror-<pid>`, made empty,
  removed at the end or on a failure; the first-run welcome closed). The API on loopback, a Full share of the first
  workspace made with `share_workspace`, connected as `r1` with `Keys::without_keyring(None)` (the key lives in memory),
  `r1` shown at 50% on the middle of its blocks; it waits (30 s) until `mirror_streams_all`, makes home's five
  terminals redraw, and measures with the mixed scenario's zoom and pan (`sweep_and_circle`, shared with
  `--bench-mixed`), printing the same line with `"bench":"mirror"`. A deadline missed prints
  `{"bench":"mirror","error":…}` and exits 1.

#### Batch 1 as built, in short

What differs from the design across the four plans: the tokens and shares are one registry (a share is a limited token
with its details); the client has one coordinator per remote and no `select!`; a frame is fed as it is read, with no
pending frame; the mirror keeps no scrollback and says so once; typing is the person's keyboard as `terminal_input`
(`key_name` is the inverse of `named_key`); the mirror's rectangle for a moved block stands until home shows it or 2 s;
a request of home's is a wait of the mirror's inbox that is answered through the API and listed nowhere else. Not done
in batch 1, by design: notes, checklists, editors and boards as live blocks, ink and shapes, scrolling back and the
mouse in a terminal, and adding, closing and wiring blocks on a mirror.

### Batch 2a (the mirror's footing)

Plan: [2026-10-25-t2t-batch-2a-footing.md](../plans/2026-10-25-t2t-batch-2a-footing.md). The plan's "Open for the
orchestrator" items were settled as it recommended (the rule asks "not yet" first; the undo mark is 2b's; a watch set
through the API names its block and workspace; the state's items are `Arc`s at home too; "only what changed" is per
collection for blocks, wires, requests and workspaces; a slow call never cuts the remote and 524 is "down"; notices have
their own budget; answers and Start are answered calls; the Edit… row keeps batch 1's words until 2h; home's tools
unknown dim nothing). What the builder chose or changed:

- **The state is `Vec<Arc<T>>`** for workspaces, blocks, wires, requests, boards, cards, automations and shares
  (serde's `rc`, already in the tree). `Edit::run` keeps the `Arc` of an item an equal put leaves, so
  `Shared::take` (`Mirror::state`) clones pointers; `model::same` compares two collections by `Arc::ptr_eq`. Home's
  `api_state` wraps each item once. `RemoteSpace::state` is home's state as last taken; `apply_remote` runs
  `mirror_blocks`, `mirror_wires` and `remote_requests` only when workspaces, blocks, wires or requests are not the
  same, and always sets the name, look and first camera. Home's words are cleaned where they are used (`tidy_block`
  in `mirror_blocks`, the workspace's name where `apply_remote` takes it, a wire's label in `mirror_wires`).
- **Calls:** `Call { tool, args, slow }`, `Refusal`, `Answered`, `Mirror::call` (the order is sent before the future
  is polled; a forgotten or unknown remote answers "Couldn't reach the other computer."). `Command::Answer` and
  `Command::Show` and `About::Answer` and `About::Show` are gone (`About::Key` names the key's refusal, which fixes a
  minor). The coordinator keeps `jobs` (64 at most; a move replaces its waiting twin) and feeds one call at a time to
  each of two lanes (`Lane::Quick`, the calls connection with its 30 s; `Lane::Slow`, started at the first slow call,
  600 s, no `GET /v1/me`); `drop_attempt` answers every waiting and in-flight call once: a waiting one "Couldn't reach
  the other computer.", an in-flight one (it was sent) "{computer} may have done it: the connection dropped before it
  answered.". A slow lane's failure other than `Auth`, a refusal, a missing thing or `TooLarge` is "{computer} didn't
  answer in time: it may still be running there." and drops that connection only, unless the connection never opened
  (`Msg::Called.sent` false): that call is "Couldn't reach the other computer." An answer past the cap (`TooLarge`, on
  either lane) is "{computer}'s answer was too large to show.". A refusal that comes after its wait (an answer to a
  request) or its remote (Start it) is gone is not said. `Conn::call_within(limit, most)`; `git` and `read_terminal` answers may weigh 8 MiB.
  Terminal streams are 11 (`STREAMS_MOST`, `MOST_STREAMS`), so a remote holds 15 connections at most.
- **Home's tools and notices:** `Tools` (`has`, `takes`) from `GET /v1/tools`, once per attempt after the first
  `GET /v1/me` (`Update::Tools`; a failure says nothing); `Update::Notice` is a `notice` event's first line, at most
  300 characters, with a block and workspace only when short and plain. At home, a notice for the API's asker names
  the terminal it is about whether or not it is a task's (`api_notice_about(text, terminal)`); `docs/api.md` says so.
- **The rule:** `Deed`, `Nay`, `RemoteSpace::why_not` and `Workspace::why_not` (`workspace/remote.rs`). The guards of
  what a remote must never hold stay `on_remote()` (`spawn_block_with`, `set_wire`, `set_label`, `begin_ink`,
  `switch_to`'s tool reset, `open_menu`). `Deed::Move` replaced `remote_frozen`: a live remote with no `GET /v1/me`
  answered yet is "Needs a Full share", so the tests that made a remote live by hand set `me` too (`live` in
  `tests/remote.rs`). `Start it` asks `Deed::Start` and calls `show` with `entry.home`. `toolbar::gated` answers the
  words and a dim flag.
- **Holds:** `workspace/holds.rs`, `Holds<K, V>` with `hold`, `get`, `end`, `shown`, `expire`; `RemoteSpace::held` is
  `Holds<BlockId, Rect>`. `HOLD` lives there.
- **Notices as notifications:** `Budget` (3 in 60 s, one fold a minute) twice per remote (`requests_budget`,
  `notice_budget`); `remote_notice` ignores a notice whose workspace is not `entry.home`, titles the notification
  "{workspace} on {computer}" and, on a click, reveals the block when `homes` has it, else shows the workspace.
  `fold_notices` runs at each drain.
- **Share… dialog:** its two scopes say the words of the batch-2 design (read in `share_dialog.rs` against the spec;
  the dialog was not looked at on a display).

Departures from the plan: `Update::Notice` is handled in `heard` before it borrows the remote (like `Refused`);
`Deed::Start` is asked by `Act::StartIt`, not only listed; the tests that read `held` ask `held.get(&block)` (`Holds`
has no `is_empty`). Not done: the undo mark (2b's).

### Batch 2b (blocks, wires and agents on the canvas)

Plan: [2026-10-25-t2t-batch-2b-canvas.md](../plans/2026-10-25-t2t-batch-2b-canvas.md). The plan's "Open for the
orchestrator" items were settled as it recommended: the Ctrl+Z that reaches a mark says the sentence and takes the mark
off, and marks one after another are one; only acts that are a canvas step at home leave one (Hand a task… leaves
none); a mark is left when the act is sent; `x` and `y` go together and exclude `near`; `connect` with a label on two
unwired blocks is one step (the wire); `open_agent`'s answer names its block; home's agents are read from
`list_agents`' text, at `Update::Tools` and again when a menu opens a minute on; a shortcut is told once per sentence
per remote per run; the toolbar's Terminal adds at once on a remote; Notes… and Checklists… are dimmed in a remote's
canvas menu; labels aren't held; Hand a task…'s success says nothing; a kanban added from a mirror shows the new board
home makes. What the builder chose or changed:

- **Home:** `add_block` and `open_agent` take `x` and `y` (`AddBlock::at`, `Opening::at`, parsed by `place`, which
  `move_block`'s `number` and `within` share; `Place::At(Point)`, the block's top-left over what is there);
  `open_agent_for_agent` returns its target, the block it opened at once (a shared `Rc<Cell>` the `open` closure fills,
  so a worktree's later open answers none). `connect` takes `label` (`ControlCall::Connect.label`, `LABEL_MOST` 60,
  `wires::tidy_label`): on a wired pair it renames (`rename_wire`, a step, the answer says named or took the name off),
  on an unwired pair it wires and keeps the name by its key. `"label"` is in the registry's `NOT_TARGETS`.
- **The adds:** `remote_add(kind, at)` asks home (`add_block` with `x`, `y` when `RemoteSpace::takes`, a checklist with
  `NAME`), leaves a mark, adds nothing here; `awaiting` keeps the id home answers in `RemoteSpace::awaited` and
  `focus_awaited` (at the answer, and at the end of each `apply_remote`) gives that block the keyboard if the keyboard
  is where it was and 5 s have not passed. `open(rect)` refuses on a remote; `add_checklist_at` got the rule's guard.
- **Home's agents:** `home_agents` parses `list_agents`' lines (at most 100, an id of 64 bytes without control or
  quote characters, a title through `clean_title`, none that ends `  hidden`, which `agent_list` writes for an agent
  the person hid at home); `read_home_agents` runs at `Update::Tools` and when `open_menu`
  finds them `AGENTS_AGAIN` old. The Open dialog (`launcher::ask_home_folder`) lists `home_folders()`, home's workspace
  folder and its terminals' (at most 20 besides it), and a path field; it says "Asking {computer}…" until home
  answers, closes on Ok and shows the refusal in itself. `remote_open_agent` returns `Err` at once when the rule
  says no.
- **Closing:** `RemoteSpace::closed: Holds<BlockId, bool>`; the closed block fades out as a local one and is not
  drawn until home's state loses it, or two seconds pass and it comes back (as a new block, `mirror_blocks`). A refusal
  ends the hold, redraws home's state (`remirror`) and says its sentence once per close. A terminal with an agent asks
  first, as a local one does. `Space::mirror_state` is `apply_remote`'s blocks-and-wires pass, which `remirror` and
  `expire_holds` call when a hold ends with no new state.
- **Wires:** `wires_held: Holds<(BlockId, BlockId), bool>`; `connect`, `disconnect` and `rename_wire` route to
  `remote_wire` and `remote_label` on a remote (so the Wire tool, the menus and Delete all reach them); a name is not
  held. `Deed::needs` makes a home whose `connect` lacks `label` "older" for Rename wire… only. `mirror_wires` leaves
  out a wire whose end is not on the canvas (closed here, not yet gone from home's state). `mark_remote` ends the
  redo stack even when the top of the undo stack is a mark already.
- **The mark:** `Change::Mark` and `mark_remote()`; `undo_canvas` meets it before the "gone" skip, says the sentence
  and takes it off.
- **Menus and shortcuts:** `Gates::of(ws, deeds)` reads the rule's words per item when a menu is built; `gated_item`
  is a disabled element item whose tooltip is the words (a gated submenu — AI agent ▸, Connect to ▸, Disconnect from ▸,
  Move to workspace — is a dimmed item without an arrow). `Items::Mirrored` is a card of home's; a remote terminal's
  menu has Hand a task… when its state shows an agent (`ask_task`, `prompt::ask_lines`: six rows, Ctrl+Enter). `run`
  asks `command_deed` and tells a refused shortcut once (`shortcuts_told`). `NOT_MIRRORED` and the docs say what is
  left.

Review round 1: a dialog opened on a workspace acts only while that one is shown (`Workspace::still_shown`: Hand a
task…, the busy-close question here and on a local workspace, Rename wire…; the Open dialog compares the id in its
`go`), saying "Another workspace is shown now: nothing was done."; `focus_awaited` also gives up when a dialog is open
or the keyboard left the canvas, and the Open dialog closes before the wait; `open_agent` touches the agent's row once;
the block headers' `why_not(Close)` and `Grid` are read once a frame (`block::Gates`).

Review round 2: an agent opened from the Open dialog takes the keyboard. The dialog is reached from a menu item, whose
popup still has the keyboard when the handler runs, so the dialog gives it back to a handle that is gone; when home
answers, `ask_home_folder` closes the dialog and focuses the canvas right after, before `awaiting` looks at
`canvas_has_keys`. The save-or-discard question of a tab (`close_tab`) and Go to line keep the workspace's id and act
only while it is shown (`still_shown`), so a `show` while the question is open cannot remove a tab of another
workspace's editor with the same id.

Departures from the plan: the mirror's hand-over is `remote_hand_task` (`hand_task` is the local delegate's);
`remote_open_agent` returns `Result<(), String>`; `remirror(id, now)` takes the remote's workspace id (a refusal may
come after the person switched); `open_menu`'s refusals (the shells, ink, the agents when home can't open them) are
`warn`s, which also reach the tests' `said`; `Items::Canvas.shells` is `Option<Shells>` (none on a mirror); the
agents-read test hears `Tools` that list `list_agents` and `open_agent` (an empty list makes the rule call home
older, which closes the agents' menu); the end-to-end task test asserts the notice's first line ("task t1 … done in …",
a notice shows one line) and waits a few rounds after `submitted` so home's answer to the call comes before the end;
the add test compares the toolbar's point with `room_around` (`new_terminal_rect` pans the view, so calling it twice
differs). Checked headlessly only, and logged in `docs/minors.md`: the Open dialog, Hand a task…, the dimmed menu
items and tooltips, the dimmed ✕ and fullscreen.

### Batch 2c (terminals: scrolling back and the mouse)

Plan: [2026-10-25-t2t-batch-2c-terminals.md](../plans/2026-10-25-t2t-batch-2c-terminals.md). The plan's "Open for the
orchestrator" items were settled as it recommended: the history keeps the live screen in place (no reopened stream);
scrolling back is a client command (`Command::History`), not `remote_call`, so the UI thread never holds the text;
"changed since the last read" is a frame fed to the viewer (`Feeder::fed`); Shift+PageUp and Shift+PageDown page a
mirrored shell only; the history is read only while neither `full_screen` nor `mouse` is on; a refused press tells only
for `NeedsFull` and `Older`, once per terminal; the wheel in a full-screen program is arrow keys through typing; zero
events taken is not a refusal; drag moves are sent whenever `mouse` is on; typing into a scrolled-back remote terminal
brings it to the bottom; a history line keeps its tabs; a failed read says nothing. What the builder chose or changed:

- **Home:** `modes.mouse_moves` (`Modes::mouse_moves`, `?1003`; `Modes::wants` says whether the program asked for an
  event, and the glue and the viewer's painter read it from one place). `terminal_input` takes `mouse` (1 to
  `INPUT_MOUSE` = 64 events; `registry::{mouse_value, mouse_of}`; `Input::Mouse`), each event through
  `Terminal::mouse` in the program's mode when it arrives; the answer counts those taken (`Sent 0 mouse events to
  b7k2m.` is not an error). `"mouse"` is in `NOT_TARGETS`.
- **The viewer's modes:** the painter writes `mouse` and `mouse_moves` as one pair (`?1000l ?1003l`, then the one that
  is on), because alacritty's mouse modes replace each other; a whole frame clears with `CSI H CSI J`, not `CSI 2J`,
  which on the main screen would push the old screen into a viewer's history.
- **The history:** `Feeder::fill_history(text)` (in `templum-terminal`) takes what `read_terminal` answers, history and
  screen together: it keeps the viewer's screen rows, cursor and place from the bottom, parses `CSI 3 J`, `CSI H`,
  `CSI J` and the cleaned lines (every control character but a tab dropped, the last `VIEWER_HISTORY` + rows lines, a
  long line wrapping again at home's width), then `CSI n S` with `n` = the rows the text takes beyond the screen's used
  rows, which pushes the rest into the history; the screen rows and cursor are put back and the selection goes.
  False, and nothing changed, in the alternate screen. `drop_history`; `Feeder::fed` counts `feed`s (frames), never a
  fill. The client holds one terminal's history at a time (`Life::history`), filled on its thread from the call's
  answer cut to `MAX_TEXT` (`To::History`); the UI hears only `Update::History(block)` and searches Find again.
- **The mouse:** `Command::Mouse` rides the typing queue (`Item::Mouse`: events together up to 64 a call, a waiting move
  of the same button replaces the move before it, `WAITING_MOST` counts keys and mouse events alike). `report` reaches
  home for a remote pane through `remote_mouse` (modes, `Deed::Mouse`: Full, `needs ["mouse"]`); `press_program`
  tells once per terminal (`tell_mouse`) when the share can't, and the press selects.
- **Scrolling:** `Deed::ScrollBack` (read). `remote_history` asks the client; `remote_scroll` (the wheel) and
  `remote_page` (Shift+PageUp/Down) scroll what the viewer has (`scroll_viewer`) and ask for home's when the person
  starts to go back; `open_find` asks too. `typing_into(id)` is `typing_at` for any remote terminal. `remote_wheel` and
  "Only the live screen is mirrored for now." are gone.

Departures from the plan: the fake home answers `read_terminal` from a lent terminal only for a block that is lent
(else its old "did read_terminal", which `tests/server.rs` reads); the client keeps `fed()` as it was when the read was
asked, carried in the job (`To::History(block, fed)`), not after the fill and not set by a later ask that sends no read
(`history_asked: Option<String>` only names the block asked last), so a frame that lands between the ask and the fill
makes the next scroll read again instead of hiding the change; a fill that returns false (home's program went full
screen between the ask and the answer) leaves the history held as it was, so it is still dropped when the stream ends;
an answer already in flight for a block is not asked
again (found in the attempt's waiting jobs and its quick wire, no flag to stick); `remote_history` and `remote_page`
take `&self`/`&mut self` as the borrows needed (`remote_history` is `&self`); `fill_history` puts back the autowrap mode
the viewer had instead of always `?7l`; the typing tests count home's `inputs()`.

### Batch 2d (boards, live and acted on)

Plan: [2026-10-25-t2t-batch-2d-boards.md](../plans/2026-10-25-t2t-batch-2d-boards.md). The plan's "Open for the
orchestrator" items were settled as it recommended: a remote kanban is `Body::Kanban(Kanban { board, remote: true })`
and `Body::kanban()`/`kanban_mut()` answer None for it, so no local lookup by board id meets home's ids; every write
of a kanban goes through one door, `Workspace::act_on_boards(space, BoardAct, …)` (on this computer today's `Boards`
edit, one Ctrl+Z step each; on a mirrored workspace a call of home's, asked of `why_not` first); `before` means "goes
right before" at home and the mirror says it as the card that follows where the local drop puts the card; cards moved,
edited and deleted are held over home's state (2 s), column drags, renames, new cards, comments and a board shown show
a round trip later; card, column and board acts leave an undo mark, showing a board leaves none; a new card's number is
read from home's "Created card 77."; a refused Done reopens the dialog with the person's work; a comment deleted on a
mirror has no Undo; `show_board`'s `new` takes a blank name for "Board n"; the caps the spec doesn't give are 1,000
boards, 100 columns, 500 cards a column and 100 attachments. What the builder chose or changed:

- **Home:** `templum_mcp::tools::LABELS`; `create_card` takes `labels`; `edit_card` takes `title`, `description` (read
  raw: blank clears it), `labels` (`{"red": true, "blue": false}`, applied in the labels' own order) and
  `remove_items`; `move_card` takes `before` (`Boards::move_card_before`). The control server gains `delete_card`,
  `edit_board`, `edit_column` and `show_board` (`control::Before`; `Boards::move_column_before`) and shares
  `edit_comment` and `delete_comment` (`SHARED` is 20). The gate asks a board path (`approvals.rs`, `BoardAsk`,
  `board_ask`) for a delete of a card, a board or a column, since no block is its target; its facts are the words, so a
  card added or gone since the question shows as changed. Through the API the person edits comments by `you` and
  deletes any; the Templum Agent only its own. A delete of a board or a column is the agent's own (it runs
  unasked) only when the agent made the board and every card the delete takes.
- **The mirror's copy:** `RemoteSpace::boards`, made by `mirror_boards` (`workspace/remote_boards.rs`) from home's
  state where it enters: cleaned and capped (`card_of`), a card once and only in a column of its own board, a card
  whose `Arc` didn't change moved over as it was. It is never saved; nothing local is written for a remote workspace.
- **The holds:** `cards_moved`, `cards_deleted`, `cards_edited` (`Holds`), laid over the copy by `lay_holds`; an edit
  leaves out the items it adds (their ids are the mirror's own, from `RemoteSpace::fresh_id`, counting down from
  `u64::MAX`), and a card an edit was laid over is cleaned again at the next copy (`laid`).
- **The Boards dialog** lists the boards of the workspace it was opened on (`kanban::rows` takes that space; the
  dialog's `list_dialog::open` takes `rows` as a closure), and `act_on_boards` says "Another workspace is shown
  now: nothing was done." to Show, New board and Delete board when that workspace isn't the shown one, local or
  mirrored, before anything else: a dialog left open on this computer's workspace can't delete a board of its own
  that has home's id.
- **The dialog:** `Work::space` is where the dialog was opened; its acts go to that workspace by id. A mirror keeps no
  draft (`Work::keep` is skipped, Save draft is gone); attachments show by name and size, Attach is dimmed with
  "Attachments are on {computer}: open them there.", and files dropped, pasted or picked attach nothing; the assignee
  menu lists home's terminals and agents; `finish_refusal` says why Done can't go (the rule, or an `edit_card` home
  lacks an argument of) before anything is sent, and the dialog stays open.
- **Send to ▸** on a remote card lists `remote_terminals()` and pastes with `remote_send_to` (a Full share, 256 KiB).
- **Benchmarks:** `--bench-mirror`'s two kanbans each show a 12-card board, so the mirror draws them live.

Departures from the plan: Delete card in the dialog doesn't ask first on a mirror, as it doesn't locally (the card's
menu does); the dialog's comment pencil and trash are refused with the rule's words when clicked rather than dimmed
(Done, Comment and Delete card are dimmed); the kanban header's All boards button isn't dimmed (its menu item is);
the headless tests drive column moves, renames, deletes and the Boards dialog's rows through `act_on_boards` (the
buttons and prompts call those acts), and the comment field of a mirror's tall dialog through the work, since it
leaves the test window; `Card::text(None)` is `menu/mod.rs`'s choice for a remote card; `Gates` and `gated_item` serve
the kanban's body as the plan says, and `kanban::open_boards` takes the workspace's id as its first argument.

### Batch 2e (checklists, notes and ink, live)

Plan: [2026-10-25-t2t-batch-2e-checklists-notes-ink.md](../plans/2026-10-25-t2t-batch-2e-checklists-notes-ink.md). The
plan's "Open for the orchestrator" items were settled as it recommended: a remote note and checklist are
`Body::Note(Note { remote: true, .. })` and `Body::Checklist(Checklist { remote: true, .. })`, which `Body::note()`,
`note_mut()`, `checklist()` and `checklist_mut()` skip (`note_view()` and `checklist_view()` answer any), so no local
lookup by id meets home's; home's `notes` and `checklists` hold only what a block shows; `checklist` is left out on a
checklist block that shows none and `notes` is on every note block; an older home is told by its tools (no `edit_item`);
a check, an edit and a delete of an item are held over home's state (2 s), added items show a round trip later; + Add
an item opens a field for a new item (Enter sends `add_items` and opens the next, blank closes, blur sends, Esc drops);
a blank edit sends nothing; item acts leave an undo mark; a cut note ends "… The rest is on {computer}."; home's
drawings are the remote `Space::ink`, painted unchanged and never picked (`ink_at` is None on a mirror); a shape counts
2 of the 100,000 points; a note is in one tab at most across the workspace; `DrawingView` mirrors `session::InkItem`
field for field. What the builder chose or changed:

- **Home:** `State.{notes, checklists, drawings}`, `BlockView.{notes, checklist}`, the events `note`, `checklist` and
  `drawing` (`keeps_items`, `parse`, `diff`, the crowded list), `NOTE_TEXT_MOST`, `DRAWINGS_MOST`, `POINTS_MOST`,
  `newest_drawings`, the OpenAPI schemas (`NoteView`, `ChecklistView`, `DrawingsView`, `Drawing`, `NotesShown`); a
  limited token sees the notes and checklists its visible blocks show and the drawings of its workspaces.
  `NOTES_TEXT_MOST` (8 MiB as JSON, counted by `json_len`, a control character taking 6 bytes) is the budget of all the
  notes' texts of `api_state`, in the order of the workspaces and the tabs: the first notes that fit keep their text,
  the rest are `cut` with an empty text (a limited token's share of it is counted before its filter, so a note it sees
  may be among them). `api_scope` is the same state without notes, checklists and drawings, which `api_allows` and the
  other rules over blocks, workspaces and requests read, so a tool call copies none of the content.
  `api_state` builds them from `Body::note()`/`checklist()` and `!is_remote()` spaces only, so the mirror's API never
  relays. The control server shares `edit_item` and `delete_item` (`SHARED` is 22): their bodies are
  `Workspace::item_text` and `item_delete`, which the block server calls after its wire check; `delete_item` is
  Ask own, with its own words (`Delete item 13 of "Release"`, the item's text) and facts holding the text.
- **The mirror's copies:** `workspace/remote_content.rs`: `RemoteSpace::{notes, checklists}` (`mirror_notes`,
  `mirror_checklists`: cleaned and capped, an item whose `Arc` didn't change moved over as it was, never saved;
  `mirror_notes` answers the ids it made anew, and `remote_notes` compares and sets an editor's text only for those,
  so a state that changed home's blocks alone copies no text) and
  `Space::mirror_drawings` (through `InkItem::drawing`, then ±10⁷ and home's caps again). The note editors are
  `RemoteSpace::note_editors`, made in `apply_remote` where a window is (`Workspace::remote_notes`) and laid into the
  blocks' tabs by `Space::lay_note_tabs`, which `remirror` calls too; each is read-only with no subscription.
- **The door:** every write of a checklist's items is `ChecklistAct` through `Workspace::act_on_checklist`; on a
  mirrored workspace `remote_checklist_act` asks the deed (`CheckItem`, `AddItems`, `EditItem`, `DeleteItem`) first,
  lays the hold (`items_checked`, `items_edited`, `items_deleted`, by `lay_item_holds`; a checklist one was laid over is
  cleaned again at the next copy) and calls home's tool. A mirrored field's events carry the workspace it was opened on.
- **The views:** `Note::title`, the strip, the tint, `Checklist::title` and the body read `notes_of` and
  `checklists_of`; the tab strip takes `locked`, `header_button` takes `why` (dimmed, the words its tooltip),
  `block::Gates` gains `boards`, `notes` and `checklists` (the minor about All boards closes), and `mirrored_card` takes
  its kind, title and sentence (and an element id) for an older home's note and checklist.
- **Benchmarks:** `--bench-mirror` gains the ink bench's 200 strokes on home's workspace, which the mirror paints.

Departures from the plan: `Editing` does not carry the workspace (the field's subscription captures it); no
`notes_in` (nothing needed it; `checklists_in` serves `edit_field`); `tab_strip`'s only caller is the note block, so
`editor.rs` passes nothing; `would_type` is platform-aware (the secondary key is Ctrl off macOS); the remote note's
menu is tested through the rule's words and the tabs' clicks and by clicking its All notes… and Rename… items (round
1); `Body` and `Space` both gain the `*_view` accessors the plan names on `Space` only.

Round 1 of the review: the notes' total budget and `api_scope` (above); `mirror_notes`' made ids; `rename_note` refuses on
a mirror itself (home's note id is a note of this computer's too), as `new_note_tab` and `move_item` do, and
`live_content`'s mirror has its own note 7 and list 12 (items 13 and 14), so a write made with home's ids shows in
`untouched`; the Templum Agent's `delete_item` of an item that isn't there (or on a block with no list) runs at once and
says so, with no approval to click. Not done, in `minors.md`: caching the `Arc`s of drawings and notes between API ticks.

### Batch 2f (Git at home)

Plan: [2026-10-25-t2t-batch-2f-git-home.md](../plans/2026-10-25-t2t-batch-2f-git-home.md). Its "Open for the
orchestrator" items were settled as it recommended (the popup calls each job's own function in `ops`, never `ops::run`;
operations follow the jobs; `set_author` writes the repository's config for a limited token; a hunk is accepted only as
`hunk_patch` draws it; every call has a 10-minute deadline; the queue per repository is on the UI thread; `git:diff`
is grouped with `git:look`; `resolve` and `resolve_region` stay Control; a discard through the API keeps a copy when
the trash fits; `message` answers `committed`; `FileDiff` gains `cut`; `templum-api` depends on `templum-git`; shape
refusals are `400`, state refusals `422`, confinement `403`). What the builder chose or changed:

- **templum-git:** serde on every result type (enums in snake_case, the default external representation); `Repo`
  gains `with_deadline` (`run_in` takes a deadline and goes through `until_cancelled` with it, a kill being the
  `CANCELLED` error) and `set_author(&self, name, email, global)`, which runs in the repository's root where the old
  one ran in Templum's own folder; `Pull` is `Default`. `ops/` is `mod.rs` (the shared types: `Failure`, `Look`,
  `Committed`, `Undone`, `Discarding`, `Compared`, `Page`, `CommitShown`), `jobs.rs` (the popup's job bodies), `op.rs`
  (`Op`, `OPS`, `Home`, `Answer`, `Done` and the answer types), `check.rs` (the argument rules and the state checks)
  and `run.rs` (`run`, `fit`, `ANSWER_MOST`). `message::{Writer, generate}` and `Workspace::commit_writer` are
  Generate's, shared with the popup.
- **The rules, as built.** A path: 1–4,096 bytes, no control character, not starting with `/`, `\` or `:`, no drive,
  no empty, `.` or `..` part, and no part that is `.git` (`is_dotgit`, as git's `is_hfs_dotgit` and `is_ntfs_dotgit`
  read it): in any case, without HFS+'s ignored characters (U+200C–200F, U+202A–202E, U+206A–206F, U+FEFF), with
  trailing dots or spaces or a stream after it (`.git::$INDEX_ALLOCATION`), or `git~1`; on Windows no `:` in any part.
  A hash: 4–64 hex digits. A new name (branch, tag): not starting with `-` or `+`, none of `^ ~ : ? * [ \`, no `..`
  or `@{`, no control character or whitespace, at most 255 bytes, and not a hash's digits (4–64 hex: git resolves a
  ref ahead of the commit). An existing branch, and a remote: the same shape without the whitespace rule, then
  listed (`Which::Local` for push, delete, rename, publish and unmerged; a remote branch for `switch` with `remote`).
  A stash is `{index ≤ 10,000, hash}`, checked by `Repo` against the list. A trash entry is 1–100 bytes of
  `[A-Za-z0-9._-]` listed by `ops::discarded` for this repository's root. A patch is exactly one file with one hunk
  and `hunk_patch(&file, 0)` equals it byte for byte, its file listed on the side the operation acts on; a
  `discard_hunk`'s `path` is the patch's. Texts are one line (no control character); `description` takes newlines.
  `inside` checks the folder a path lies in (its deepest existing ancestor, links followed, under the canonical root; a
  link that leads nowhere is outside) and `not_link` the file a write replaces, in that order, so a link inside to
  somewhere outside answers the link's words.
- **Departures from the plan** (the code as built is the truth):
  - `ops` is split into `op.rs`, `check.rs` and `run.rs` besides `mod.rs` and `jobs.rs`; `Home` and the answer types
    live in `op.rs`.
  - `Workspace::api_git` returns `Result<Option<String>, Refusal>` (a line now for a mirrored workspace, one without a
    folder, or a missing folder; Ok(None) for a call that answers later), not `Result<(), Refusal>`.
  - `set_author`'s `name` and `email` may not be empty or start with `-`: `git config --local user.name <value>` reads
    an option in the value's place.
  - **`diff` leaves out an untracked or conflicted file whose path leads out of the repository or is a link:**
    `Repo::working_diff` reads those itself (`std::fs::read` follows a link), so a link to a file outside would show
    the file to a Read token. The popup is unchanged.
  - **`restore` and `ignore` check what they write:** every file a discard kept must lie in a folder inside the
    repository (`Trash::restore` writes through a folder link), and `ignore` refuses a `.gitignore` that is a link.
  - `unstage` also takes a staged rename's old path; `stage` takes a conflicted file (Mark resolved).
  - `delete_branch`'s sentence is "Deleted {branch}." and `init`'s "Made a repository at {root}.", which the plan's
    list leaves out; a shape refusal of a name says "\"{name}\" can't name a {branch|tag|remote}.".
  - `ToolResult.git` and `Error.details` are added in `schemas_with_views` (the schemas' `json!` is at the macro's
    recursion limit); `server.rs` marks a git response with `Audited` (a response extension) and `route` writes it.
- **Review round 1 (the share's token drives git on the person's computer):**
  - **Names that reach git are full refs.** `publish` pushes `refs/heads/{branch}`, so a branch named `+main` is no
    forced push of `main` and one with a `:` is no `src:dst` (`-u` still sets the upstream); `merge`, `compare_log`
    and `range_diff` take `refs/heads/{name}`, else `refs/remotes/{name}` (`Repo::full_ref`), so a tag or hash of the
    same name never stands in for the branch; `push_branch` and `unmerged_count` already built `refs/heads/…`. A name
    git reads as a branch only (`switch`, `rename_branch`, `delete_branch`, `new_branch`) is checked by listing or by
    `check-ref-format`. The popup gets these too (the commands it runs are the same).
  - **`undo_commit`'s `replaced`** must be a commit other than HEAD with HEAD's parents, as an amend leaves it
    (`Repo::amended`); any other is a git failure and the branch stays. An amended root commit has no parents, so
    another root commit passed (closed in round 2 by the reflog).
  - **`fit`** weighs each file's hunks once, sorts and cuts heaviest first: linear, 4,000 files in milliseconds. What
    git prints isn't capped before `fit`; the call's deadline bounds it.
  - **The deadline kills what git started.** A call with a deadline (the API's) runs git in a process group of its
    own (Unix `process_group(0)`) and kills it with `kill -KILL -- -<pid>`; on Windows `taskkill /T /F /PID`. Such a
    call also asks nothing on a terminal: `GIT_TERMINAL_PROMPT=0` (as every call), `SSH_ASKPASS_REQUIRE=force` with
    `SSH_ASKPASS=false` on Unix (ssh's askpass fails at once, as `BatchMode` would, without replacing the user's
    `GIT_SSH_COMMAND` or `core.sshCommand`; a `SSH_ASKPASS_REQUIRE` the user set stays) and `GCM_INTERACTIVE=never`.
    The popup's calls keep their prompts and their plain kill, which a person at the keyboard may rely on. **A prompt
    can still appear at home:** a graphical `GIT_ASKPASS` or `core.askPass` window (acceptable: no operation can change
    a remote's URL), and with `commit.gpgSign` gpg-agent's pinentry for a commit the remote made; with the passphrase
    cached (or `gpg.format=ssh` and an agent) the remote's commits are silently signed with the person's key. Kept as
    it is and listed in `minors.md`.
- **Review round 2:**
  - **A call can't make home read without end.** `until_cancelled` takes `most`: a call with a deadline (the API's)
    reads at most `API_OUTPUT_MOST` = 64 MiB of git's stdout (and of its stderr), one byte past it kills the group and
    the error is `GitError::too_large` ("The output of git is too large to read from <computer>: ask for less."). The
    patch is then parsed and copied (about 4 times its size: 186 MB of patch peaked at 774 MB), so the cap is what bounds
    the memory, not the deadline. The popup has no deadline and reads as before. The files `whole` names in `diff`
    are read up to `ASKED_BYTES` = 32 MiB together (each still under `DIFF_LIMIT`); past it a file follows the ordinary
    budget, else it is listed unread.
  - **A call's discards can't push out a person's.** `Trash::for_api` marks the copies it makes (an `api` file in the
    discard's folder) and `prune` keeps the newest 50 of each repository root, the API's counted apart from the person's;
    60 `resolve` calls in one repository leave another's discards, and the person's own, listed. Not done: skipping a
    copy whose bytes equal a version git keeps (`resolve` can still add 50 copies of a file per repository).
  - **A path is a file, never a pattern.** `Repo::run_paths` prefixes `--literal-pathspecs` for the commands that take
    a caller's paths (`stage`, `unstage`, `discard`, `resolve`, the whole-file diff and a file's history). It is not set
    for every command: `git stash push -u` passes pathspec magic of its own and left untracked files in the tree
    (and in the stash) with `GIT_LITERAL_PATHSPECS=1`. The popup has it too.
  - **`undo_commit`'s "Pushed" guard reads the full HEAD hash**, not the short one a tag or branch can shadow.
  - **A tag and a branch never share a name:** `tag` refuses a name `refs/heads/` has, `new_branch` one `refs/tags/`
    has, and `branches` lists names by `refname:lstrip=2` (and `upstream:lstrip=2`), so a tag that arrived with a fetch
    named like a branch no longer turns it into `heads/main`.
  - **`undo_commit` of an amended root:** when HEAD has no parents, HEAD's reflog must say that its newest "commit
    (amend)" entry replaced `replaced` (the entry before it). The plan said `HEAD@{1}`, but a later commit that was
    undone moves it (the existing test of an amend, a commit and its undo).
- **Review round 3:**
  - **A call's copies are never count-pruned.** Round 2 counted the API's copies apart, but 50 `resolve` calls on a file
    the person had merged by hand pushed the hand copy out (the file stays conflicted, so the call repeats), and a forced
    `restore` made the same kind of copy. Now `prune` deletes an API copy by age (7 days) only, and `Trash::put` of the
    API's trash refuses (`io::ErrorKind::StorageFull`, shown as `Failure::Keep`: "Couldn't keep a copy, so nothing was
    changed: the trash already holds 50 copies made through the API for <root>: restore or clear them on that
    computer") when the root already has `KEEP` of them younger than `MAX_AGE`. Every op that needs a copy goes through
    `put` before it writes (`discard`, `resolve`, `resolve_region`, `restore_version`, a forced `restore`), so the disk
    stays at 50 copies per root. The popup's trash is not held to it. Not done (in `minors.md`): a `resolve` repeated on a file that already holds that side.
  - **One budget for the whole re-reads.** `working_diff` owns `asked_bytes` and `side_diff` takes it: a tracked file
    named in `whole` is re-read only while the budget (32 MiB) has room for its working size, and every re-read adds the
    bytes git wrote; the untracked files named count against the same budget. A file past it keeps its normal diff.
  - **Size before read.** A conflicted file past `DIFF_LIMIT` is skipped by `working_diff` and refused by
    `resolve_region` ("<path> is too large to resolve by region: take one side of it instead.") after `fs::metadata`,
    not read. `stamp` hashes in 64 KiB chunks (the same value `Vec<u8>::hash` gave, so earlier seals stay valid).
- **Review round 4:**
  - **"Couldn't keep a copy" says which step failed.** Round 3 chose `Failure::Keep` by `ErrorKind::StorageFull`, so a
    disk filling during a forced `restore`'s own writes read as "nothing was changed" (earlier files were back), and a
    copy failing with another kind read as "Couldn't write the file". `Trash::restore` now wraps the error of its copy
    (`put`, or the size refusal) in a marker, `trash::kept_nothing(&e)`, and `Op::Restore` answers `Keep` for that and
    `Io` for the rest, whatever the kind.
  - **A forced restore's copy is held to the trash's size too.** For the API's trash, `restore` measures what it would
    write over (`measure`/`fits`: 100 MiB, 500 files, as every other copy) and refuses before writing ("too large to
    keep a copy of what it would write over: restore it on that computer"), so the 50 copies per root can't hold 50 big
    blobs. The popup's trash is not held to it.
  - **A link to a device is never read.** A link to /dev/zero is a character device of size 0 through the link: it
    passed the size check and `fs::read` never ended (memory). The whole-file reads of `templum-git` now look at the
    file itself (`symlink_metadata`): `working_diff`'s conflicted files, `untracked_diff`, `count` and the commit
    message's untracked files read through `repo::working_bytes`, a regular file's bytes or a link's own target text
    (as git diffs a link), and nothing for a device or a pipe; sizes are a link's own (`working_size`), so a changed link
    no longer shows its target's size and lines. `resolve_region` refuses anything but a regular file; `stamp` hashes
    regular files only (a device is `?0`); the trash's copy already refuses a device (`fs::copy`), and `measure` reads
    no content. Not changed: `.gitignore` is read whole by `Repo::ignore` (the API checks it is no link first).
  - **Conflicted files have a total too.** `working_diff` keeps a running total (`CONFLICT_BYTES`, 16 MiB) of the
    conflicted files it reads; a file past it is listed as a conflict with `unread` set, regions not read.
- **Not done, in `minors.md`:** two calls sent back to back on one repository may run in either order (each finds its
  repository first); a planted `.git` file naming a network share is read by git before the root can be checked.

### Batch 2g (the Git popup on the mirror)

Plan: [2026-10-25-t2t-batch-2g-git-mirror.md](../plans/2026-10-25-t2t-batch-2g-git-mirror.md). Its "Open for the
orchestrator" items were settled as it recommended (the seam passes the `Op` and today's closure; a mirror's Git holds
no `Repo` and an empty `folder`; the panel's diff is read when the status changes, after 10 s or when asked; a running
Fetch, Pull, Push, Sync or Generate can't be stopped from the mirror; home's `details` travel in the client's
`Refusal`; a failed command keeps Details and drops Run in a terminal; Full-only acts are refused at their entry; the
folder menu lists folders, not repositories; `measure` has no "has a trash"; Generate never needs this computer's
agent and goes straight to `remote_call`; Recently discarded… is always offered; the popup opens on home's folder as
`home_folders` cleans it; a read the rule refuses sets the error line, an act warns). What the builder chose or
changed:

- **The client:** `Refusal` gains `details: Option<String>` (`Default`, `Refusal::of` is `pub`, `DETAILS_MOST` =
  64 KiB, cut at a character's boundary where `http::templum_error` reads it); `Failure::Refused` carries it;
  `clean_refusal` (glue) cleans the sentence to one line and the details to newlines and tabs, blank ones to None.
- **The seam** (`workspace/git_home.rs`): `git_op` / `act_op(at, op, here, home, done)` are `git_job` / `act_job` on
  this computer, exactly; at home `home_job` keeps the popup's queue and busy rules, asks the rule
  (`Deed::Git(op.name())`: an act warns, a read sets `git.error`; nothing is sent, `acting` untouched) and calls
  `git` through `remote_call` (`slow` for fetch, pull, push, push_branch, publish, sync and message). `heard` turns
  a refusal into a `GitError` whose stderr is home's details (else its sentence), `parsed::<X>` the answer into the
  job's result; `job_done` is `git_job`'s completion, moved out unchanged.
- **A mirror's Git** has `home: Option<String>` (the folder at home) and `Found::{Home(root), Refused(why)}`; its
  `folder` is empty and it never holds a `Repo`, so `poll_git`, `look_up_at` and the rest never stat or run anything
  here. Status every 2 s while shown and live; the diff after a status read only when the status differs from the one
  it was read with or `HOME_DIFF_AGAIN` (10 s) passed, and on opening, Refresh, whitespace and whole-file toggles.
  Nothing is written: `Space::git` of a remote is never saved (a test walks the data folder).
- **The rule:** the Full-only acts (`discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop`,
  `abort`) are refused at their entry, before any question or measure; their buttons and menu items are dimmed with
  the rule's words (`GIT_GATES`). Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a terminal
  are `Deed::GitLeftOut` (`Not on a mirrored workspace yet`) and left out or dimmed.
- **Details** keeps git's whole stderr (a failed command's notification has Details, no Run in a terminal; a login
  failure says "git on {computer} asks for a login: run it in a terminal there."). A `FileDiff` with `cut` draws
  "Too large to show from {computer}.".
- **Generate** runs at home with home's agent (`message`, slow lane, not through the Git's queue): the fields,
  "Left out"/"Kept your message" words and the failure's title use the computer's name.
- **Departures from the plan** (the code as built is the truth):
  - `Workspace::runs` is `git_runs`: `runs` is taken (`content.rs`). `home_send` takes no `window`.
  - The spinning Sync on a mirror reads "Running" (not "Cancel"): it has no click and its tooltip says it can't be
    stopped from here; `git.cancel` is None there (no flag is made).
  - `set_git_folder` returns false on a mirror (a guard), besides the Git never being reached by it.
  - `open_git` sets `home_diff = None` for any Git (only a mirror reads it).
  - Restore this version…'s question says the file goes to Recently discarded on a mirror (home keeps the copy).
  - Tests that draw frames set the mirror live again (`go_live`): a frame lets the mirror's real connection to nowhere
    fail and change its status.
- **Review round 1 (what changed after it):**
  - **Restore acts on the Git it was asked on.** `restore_discarded(at, entry, force)` takes the `GitAt`:
    Recently discarded… carries it from `show_discarded` into its Restore buttons, the "changed since" question and the
    Undo buttons carry it, and the function opens with `show_git(&at)` (as every notification button does), so a
    click or an answer after another workspace was shown acts on the workspace the entry came from. On a mirror
    (`at.home` set) the restore is home's or nothing: this computer's trash is never reached, whatever state the
    mirror's Git is in. `Trash::restore` and `remove` (the root fix) take an id only if it is one name inside the
    trash (`../x`, `a/b` and absolute paths name nothing).
  - **Home's strings that are sent back are not cleaned (see Review round 2).** `Found.root` is every later call's
    `folder`, so `git_home::found` refuses a root that `clean_to(_, LINE_CHARS)` would change ("The repository's
    folder at home has a name this computer can't show.") instead of altering it. A restore's "changed" paths are one
    line each and at most 1,000 (`MOST_CHANGED`); Generate's summary is one line (`LINE_CHARS`) and its description at
    most `TEXT_CHARS`. A serde error (it quotes the value it met) is cut to 200 characters in "The answer couldn't be
    read".
  - **A measure home refuses asks nothing:** `ask_discard`'s job returns a `Result`; an `Err` is told as any failed
    command (`notify_git_error`) and no question is asked.
  - **A Find the rule refuses sets `Found::Refused(why)`** (Try again asks again), instead of leaving "Looking for the
    repository…" for good. `set_home_git_folder` does nothing unless a mirror is shown, and its menu items are
    `still_shown(space)` checked (`Items::HomeGitFolders::space`).
  - **Home's answer is read in the background:** `home_send`'s `Then` hands `home(heard)` (the `serde_json` parse
    and the cleaning, up to 8 MiB) to the background executor before `job_done`, as a local job's result is made
    (`home_job`/`home_send` need `T: Send`). Tests that answer a call read it with `answer_with`/`reply`
    (`run_until_parked`); a mirror's real connection is let to fail once in `on_mirror_with`, before the test starts.
  - **Hooks go on the slow lane:** besides the network and Generate, `commit`, `switch`, `new_branch`, `checkout`,
    `merge`, `revert`, `stash_apply`, `stash_pop` and `continue` (pre-commit, commit-msg, post-checkout, post-merge).
- **Review round 2 (what changed after it):**
  - **Home's answers are read as home gave them.** Round 1 cleaned every string of every answer (`clean_answer`: no
    control characters, at most 10,000 characters). That was a regression: the strings the panel sends back to home
    (a hunk's lines in `hunk_patch`, a file's path in Stage, Unstage, Discard, Resolve, Ignore and Restore this
    version) were no longer home's, so Stage and Discard of a hunk wrote the cleaned lines at home (`git apply` finds
    the cleaned text at another offset, and `patch_file` only checks the patch against its own re-parse), and a path
    with a stripped character named a sibling file. `parsed` now only reads the JSON into the job's type; the local
    panel already draws the same raw git text from a local repository, and the answer's size is bounded by the 8 MiB
    cap. What is still cut or cleaned is what is not sent back: `found`'s root (refused if changed), `shown()`, `Kept.what`,
    Generate's summary and description, and the error text of a serde failure.
  - **Tests:** a hunk with an escape sequence and a 10,001-character line is staged from a mirror and the call's
    `patch` is `hunk_patch` of the raw diff; a path with a control character is sent back as home gave it; a folder
    picked from a menu left open on another workspace changes nothing and says so.

### Batch 2h (agents, automations and the inbox)

Plan: [2026-10-26-t2t-batch-2h-agents-automations-inbox.md](../plans/2026-10-26-t2t-batch-2h-agents-automations-inbox.md).
Its "Open for the orchestrator" items were settled as it recommended: a home rule's popup key is a number in a range of
its own (`HOME_KEYS | n << 32 | id`); no hold for a switched rule or a cancelled schedule; Duplicate sends home's own
`when` and `do` back; home's `save_automation` keeps a new rule's `on`; the foot sentence shows under every home rule's
editor; a new local rule starts on the shown workspace when it is local, else on the first local one; this computer
refuses to save a rule whose Where is a remote workspace; `read_terminal` is grouped in the audit log; a request's age
is home's `since` (never ahead of now, at most 7 days); a request with no viewer is a wait with an empty `from`; the
badge counts home's failed, skipped and missed runs after the popup was last opened; caps of 500 rules and 64 KiB
texts; a notice of a home rule opens the popup at it on a click; "aged from home's since" and "nothing runs on the
mirror" are headless cases, the latter on two workspaces over loopback. What the builder chose or changed:

- **Home:** `AgentView` gains `task` (`TaskView { id, text, by, since }`, `by` the uid of the block that handed it,
  `"api"` for the API) and `role`; `State` gains `schedules` (`ScheduleView`, event `schedule`) and `Notice` gains
  `automation`; a limited token sees the schedules whose `to` block it sees and a `by` it doesn't see as null.
  `cancel_schedule` is the API's tenth own tool (`Cancelled s3.`; "There's no schedule s3: GET /v1/state lists
  them."). Notify me also sends `"{title}: {text}"` as a notice with its rule's number. A new rule saved through the
  API keeps its `on`. `read_terminal` calls are grouped in the audit log.
- **THE RULE, on the mirror's own engine:** `Space::automations_there` ("API is a mirror of winx-desktop: its
  automations run there."); `automation_target` skips a stale rule for a remote workspace with "Target missing: …";
  `automation_problem` refuses to save one; `new_automation` starts on a local workspace; the editor's Where
  (`where_choices`) and its block lists (`blocks`) leave remote workspaces out; `watch_agents` pushes a `Heard` only
  from a local space. Home's rules and schedules are copied into `RemoteSpace::{rules, schedules}`
  (`workspace/remote_rules.rs`), never into `Workspace::automations`, `automation_runs` or `schedules`: a test for
  every way a rule could fire on the mirror (a time, an event, a notice, a stale rule at start, at its time and by
  Run now, a schedule's delivery), and one over loopback that counts home's notices of a rule switched and run from
  the mirror (one).
- **The popup:** a home rule is addressed by its popup key; every function the popup calls routes a home key to a call
  of home's through `home_call` (the rule's `why_not_in` first: a refusal is said and nothing is sent; home's refusal
  is said), and a local key keeps today's code. A group per remote workspace after "Asked by agents": the rules,
  the schedules with Cancel, the foot; dim when the remote isn't live. The editor on a home rule lists home's blocks
  and agents, has the one Where, and saves at home (the red line only while the form still shows that key); `Next`
  and `Last` are home's.
- **The Agents popup:** `Space::watched_terminals` (a remote workspace's viewers too, for the Agents view and its
  watch alone) and `Workspace::watched_caller`; a remote agent's row under "API · winx-desktop" with home's title for
  its agent, its task, card and context; its details (model · effort, role, task, subagents' count, usage with the
  effort line, the screen read through `read_terminal` every 2 s while shown and not streamed); home's own `done`
  reads as idle (this computer's watch makes its Done).
- **The inbox:** a remote request is aged from home's `since`, listed when its block has no viewer here (a wait with
  an empty `from`, no Go to terminal), and the list says "And 3 more on winx-desktop." past 50.
- **The words:** the Edit… row's "The share can" and Share…'s invitation say what a share can do now.
- **Departures from the plan** (the code as built is the truth):
  - `Space::terminals()` leaves a remote workspace's viewers out on purpose, so `watch_agents`, `agent_rows` and
    `done_count` did not see them (the plan said they already did). `watched_terminals` and `watched_caller` are the
    two ways the Agents view reaches them; nothing else does. The existing test `a_remote_agent_s_dot_comes_from_home`
    now expects home's `done` to read `Idle` (the plan's rule).
  - `rule_groups` takes `cx: &App` (it names home's blocks through `uid_title_in`); `home_keys` gives the keys alone
    for `automation_ids`.
  - A Switch, Duplicate or Delete reaches `set_automation_on`, `duplicate_automation` and `delete_automation`, which
    take no window: a refusal of the rule is said once the update under way is over (`say`).
  - `open_automations` marks home's runs seen by remembering each rule's last run (see Review round 1), so no clock of
    home's is compared with this one's.
  - A Save's answer does not reload the form (it would show the old rule): the next state that changes home's rules
    does, when the form shows a home rule with no draft over it (`reload_home_form`).
  - The notice's click is `open_home_notice` (tested), as `open_fired` is for a local Notify me.
  - A home rule's Duplicate leaves `max_runs` and `cooldown` out of its call when home has none: a null is not a
    number to its parser (found by the loopback test).

- **Review round 1:**
  - **A second click sends nothing before home answers.** `Workspace::home_busy` holds the (popup key, tool) of every
    call on a home rule whose answer is to come (`home_rule_call`, `home_rule_call_then`): Save (also of a new rule,
    which would have made a second rule at home), Duplicate, Run now, Delete and the switch go once; the answer frees
    the key (a forgotten remote frees its keys). Save is dimmed and says "Saving at home…" meanwhile
    (`home_saving`). Cancel of a schedule is not guarded (home's second answer is "gone", said once; see
    docs/minors.md).
  - **Open request on a remote agent's row works.** `open_agent_request` takes the workspace from `request_space`,
    not from `caller` (which leaves remote viewers out).
  - **A rule the copy had to cut is not saved from the mirror.** `rule_of` also says whether cleaning changed the
    rule (`HomeRule::cut`: a text over 64 Ki characters, a control character other than a newline or tab, a Who of
    more than 100 blocks, more than 7 or 31 days, a line over its cap, a cooldown past the most); Save of it says
    "This rule is longer than a mirror shows: edit it on {computer}." and sends nothing. Duplicate sends home's own
    `when` and `do`, so it was not affected.
  - **The badge compares no clocks.** `RemoteSpace::rules_seen` keeps, per rule, the `at` of its last run as the person
    last saw it (taken when the rule is first met and when the popup opens); a rule counts when its last run is not
    Ran and its `at` differs. `automations_seen` and `latest_home_run` are gone.
  - **A request listed while its block had no viewer** gets its `from` once a viewer exists (`remote_requests`,
    `viewer_terminal`), so Go to terminal and Open request work for it.
  - **Hostile numbers and a long screen:** `aged` and the Task's "ago" line use `saturating_sub` (a `since` near
    `i64::MIN` overflowed); `screen_of` takes the last lines from the end of home's text instead of collecting every
    line.
  - **A local rule's id is below `HOME_KEYS`.** `automations::load` renumbers an id from `HOME_KEYS` up like a repeated
    one and keeps `next_id` below it, so an edited automations.json can't make a local rule look like home's.

#### Batch 2 as built, in short

What differs from the design across the eight plans: batch 2 is one registry of deeds and one rule (`why_not`) for
everything the mirror can do; every collection of home's (boards, cards, checklists, notes, ink, rules, schedules,
agents) is a cleaned, capped copy made where the state enters and never saved; every act is home's tool through
`remote_call`; the Git popup is the same popup over a seam that calls `git` at home; and home's automations run only
at home, with a test for every way one could fire on the mirror. Left out of batch 2, by design: the 15 items of
"Left out of batch 2" (fewer since: each later card that brings one says so there).

### Fullscreen on a mirror (kanban card 206)

The fullscreen button was dimmed on a mirror ("Not on a mirrored workspace yet") yet worked: it called `fullscreen`,
which never asked the rule, while the shortcut and the menu's item were refused. Grid mode only changes the mirror's
view, so it is no `Deed` any more (`Deed::Grid` is gone, with the header's `Gates::grid`): the button, the item and the
shortcut all act, the button with its usual tooltip. What made it go wrong after going in:

- **Home's state wrote over the tiles.** `mirror_blocks`, `remote_moved`, `expire_holds` and a refused move write a
  block's place through `Space::home_mut`: a tile's place waits in `Grid::homes`, so a move at home is where the block
  goes back to, and Ctrl+Z of a move in the grid sends home the canvas place, not the tile.
- **A tile home closed stayed a hole.** `mirror_blocks` untiles a block home no longer has (`Space::untile`, the
  workspace's half of `untile`, with `Space::calm`); the last one going brings the canvas and its camera back.
- **The tile's menu was a local one.** It now lists home's agents, home's default shell and the rule's gates, as the
  canvas's menu does on a mirror, and the block home adds for it joins the grid (`focus_awaited`, `join_grid`).
- **The index kept grid mode's camera** as the remote's: it keeps `Space::camera()`, the canvas's.

Tests: the four at the end of `workspace/tests/grid.rs` ("On a mirror").

### Home's shells on a mirror (kanban card 201)

Add ▸ Terminal ▸ on a mirror opened home's default shell only (then item 12 of "Left out of batch 2"). Now:

- **Home's API:** a control tool `list_shells` (read; the Templum Agent's too, its reads fold into one row), the
  default's name first with `  default`, then the others' names as Add ▸ Terminal ▸ lists them
  (`Workspace::shell_list`: the list read again when it was read early, as a menu does). `add_block`'s `shell` (a
  terminal's only; with another kind it is refused) opens the shell of `Workspace::shells` with that label exactly
  (`NewBlock::Terminal`'s `shell`, through `spawn_block_with`, so the block keeps it across restarts as a local one).
  `shell` joins the registry's arguments that name nothing.
- **The mirror:** `RemoteSpace::shells` (`HomeShells`, from `home_shells`), `shells_at`, `read_home_shells` (not
  asked of a home without `list_shells`), `shells_stale`; `Deed::AddShell` (`add_block` with `shell`, needing
  `shell`) gates the other shells; `open_home_shell` opens one (`remote_add_in` grew `shell`). The menus take a
  `ShellList` (this computer's, or home's) as they take an `AgentList`; the toolbar's Terminal opens the menu on a
  mirror too, refused with the rule's words when adding a terminal is.
- **Found on the way:** home's lists were read when its tools came, which may be before the remote is live, when
  the call is refused ("Not connected."): home's agents then waited for the menu's next open. They are read once the
  tools are known and the remote is live, whichever comes last.

Tests: `add_block_opens_only_a_shell_list_shells_names` (`templum_agent.rs`), the shells' tests after the agents' in
`remote.rs` (the list read back as home writes it, a hostile list, read at connect and after a minute, waiting for
live, an older home, the toolbar's choice), `a_shell_chosen_on_the_mirror_runs_in_the_terminal_home_adds`
(`mirror_end_to_end.rs`), and `control.rs`'s parse tests.

### Checklists on a mirror (kanban card 205)

The three acts item 2 of "Left out of batch 2" kept on home, renaming a checklist, reordering its items and showing
another checklist in a block, work on a mirror as on a local block ([Checklists](#boards-checklists-notes-ink)):

- **Home's API:** two control tools, Control scope, danger – (the Templum Agent's too): `edit_checklist` (`block`,
  then `name`, or `item` with `before`, or both; `Workspace::checklist_edit`, one `edit_checklist` step) and
  `new_checklist` (`block`, `name`; `Workspace::checklist_new`, refused on a block that shows a checklist). `block`
  is their only target, so the registry's check of a limited token covers them; an item `before` names that is not in
  the block's checklist is "There's no item 99 in …", as for any item. `control::Before` is shared with
  `edit_column` (its `Column` is `Id` now, parsed by `before`).
- **The mirror:** `Deed::EditChecklist` calls `edit_checklist`, `Deed::OpenChecklist` calls `new_checklist`, so an
  older home dims them with "is older". `ChecklistAct` gains `Move` and `Rename`, through `act_on_checklist` (a local
  drop is one now too); `remote_checklist_act` holds them in `RemoteSpace::items_moved` (by the item that follows)
  and `lists_renamed`, laid in `lay_item_holds` and ended by `expire_holds` or a refusal. `ItemDrag` carries its
  block. The name's field sends on Enter or blur (`remote_field`). `open_checklists` takes the workspace it is opened
  on, as `kanban::open_boards` does: `rows` lists the copy's checklists its blocks show, `still_on` refuses a dialog
  whose workspace isn't shown while a mirror is involved, and the trash says "Not on a mirrored workspace yet".
  `remote_new_checklist` is New checklist on a mirror (`new_checklist` into an empty block, else `add_block` beside,
  `checklist_spot`); `open_checklist` on a mirror only reveals the block of the shown workspace that has it. The
  empty block's buttons replace "No checklist open here: open one on winx-desktop." The banner's ⓘ says "deleting a
  checklist" instead of "renaming a checklist and moving its items".
- **Choices** (*the designer's call*): no `checklist` argument to show an existing checklist in a block: home's state
  lists only checklists a block shows, and one is open in one block at most, so the dialog only ever reveals; a new
  checklist on a mirror doesn't open its name's field (home names it "Checklist", as the mirror's Add ▸ Checklist
  does); deleting a checklist from the mirror stays at home.

Tests: `edit_checklist_renames_and_moves_and_new_checklist_fills_an_empty_block` (tests/api.rs: the reply words,
Ctrl+Z, a limited and a read token), `the_checklist_tools_parse_and_say_what_is_missing` (control.rs), the registry's
scope test, and in tests/remote_content.rs the name, the drag, All checklists…, the dialog's trash, a local dialog
once a mirror is shown, an older home, and the two-ends test, which renames, moves and makes a checklist at home.

### Home's recent folders on a mirror (kanban card 203)

The Git popup's folder menu on a mirror listed home's workspace's folder and its terminals' folders only; locally it
also lists the workspace's recent folders (`Space::recent`). Now the mirror's lists them too, from home:

- **Home's API:** an own tool `recent_folders` (`workspace`; Read scope; `Own::RecentFolders`, `api_own`), a small read
  tool rather than a field of the state (*the designer's call*): the state is built at every tick and narrowed per
  token by `State::visible` in `templum-api`, which doesn't touch the disk, while confinement follows links. Its answer
  is the workspace's `Space::recent`, newest first, a folder a line (`recent_lines`); empty when there are none; a
  mirror of another computer is refused with its sentence, a workspace the token doesn't see is one that doesn't exist
  (the registry's `workspace` target). For a limited token it keeps only those inside `confined_roots`, through
  `api_git::inside` (which now returns the real path: a share of another computer is never asked, links are
  followed), each as its real path (a link's own path may lie outside), once; a folder whose path has a control
  character is left out (it isn't one line). It is the footing for kanban card 207 (an editor on a mirror that opens
  home's recent folders).
- **The mirror:** `RemoteSpace::recent` (`home_recent`: at most `MOST_FOLDERS`, each once, a line kept only when it
  already is one clean line, never cleaned into another folder; cleared when the connection is), read by
  `read_home_recent` when the popup opens (`open_git`) and when its folder menu opens (`open_menu`), not asked of a home
  without the tool. `home_git_folders` chains them after `home_folders`, each once, less the one shown and those inside
  its root (as for the others: they are folders, not repositories). The menu's item is the same as the others'
  (`set_home_git_folder`).
- **Choices** (*the designer's call*): the recent folders are read when the popup and the menu open, not at connect or
  with the state: the menu shows the last answer, and the one read at the menu's opening serves its next opening. A
  mirror's first Git, with no terminal and no folder at home, still says "No folder to open Git in" rather than taking
  home's newest recent folder as the local guess does (a minor).

Tests: `recent_folders_are_confined_to_a_limited_token_s_folders` (tests/api.rs: every folder in order for a Read
token, only those inside for a limited one, links both ways, a workspace it doesn't see, a workspace with no folder),
the registry's scope and parse tests, `the_folder_menu_on_a_mirror_lists_home_s_recent_folders` and
`home_s_recent_folders_are_cut_and_nothing_is_cleaned_into_another_folder` (tests/git_mirror.rs: read at the popup's
and the menu's opening, the order and dedup, the item's `find` at home, an older home not asked), and
`the_mirror_s_folder_menu_lists_home_s_recent_folders_inside_the_share` (mirror_end_to_end.rs: over loopback, a
Control share gets only the recent folder inside its workspace's folder).

### Notes on a mirror (kanban card 204)

*Built: plan [2026-10-26-mirror-notes.md](../plans/2026-10-26-mirror-notes.md). What the builder chose or changed
is under **As built** at the end of this section; the [Notes](#boards-checklists-notes-ink) bullets above say what the
mirror does now.*

The person (PT-BR, translated): "Templum Mirror: the notes system isn't integrated, the user must be able to create
notes" and "besides reading, the user must be able to create, edit, rename, change the color of or reopen a note".
Everything a local note block offers works on a mirrored workspace, and runs at home through home's API. The calls
below are *the planner's call* unless the person said them.

**What the person sees on the mirror** (`winx-desktop` is home's name, `Another computer` the share's, as home named
it in Share…):

- **Adding a note:** the toolbar's Note, the canvas menu's **Add ▸ Note** and the shortcut add a note block at home
  where it would go locally (`add_block`, `kind` note, at `x`, `y`). It shows a round trip later and takes the
  keyboard, as a terminal added from the mirror does.
- **Typing** in a note is as local: every key shows at once, nothing waits on the network. The text goes to
  winx-desktop half a second after the person stops typing, at least every 2 s while they keep typing, and when the
  note loses the keyboard. A tab whose text winx-desktop hasn't taken for 2 s shows " •" after its name until it has
  (a note in an editor's sense of unsaved); nothing else shows while it goes.
- **What changes at home comes in live:** an agent's `append_note`, the person at home. The text changes where home
  changed it; the person's caret, selection and scroll stay where they were in their own text. While the person is
  typing, home's change waits for their next pause and is then merged with what they typed.
- **Both changed the same place** (or places that touch): nothing is merged and nothing is lost. The note keeps
  home's text, and what the person typed becomes a new note beside it, in the same block, named "Plan (Another
  computer)"; a notification says "winx-desktop changed "Plan" while you typed: your version is the new note "Plan
  (Another computer)" beside it." Two additions at the very same place are no conflict: home's comes first, then the
  person's.
- **A note closed or deleted at home while the person typed:** their text becomes a new note in that block (in a new
  note block where it was, when the block is gone too): ""Plan" isn't open on winx-desktop any more: your version is
  the new note "Plan (Another computer)"."
- **Not connected:** typing in a note says once "Not connected: what you type isn't sent." and types nothing (batch
  1's rule for keys). Text typed before the connection dropped keeps its " •" and goes when the remote is live again,
  checked against home's newest text and merged as above, never sent blind.
- **Too long for a mirror:** a note longer than 256 KiB at home (the state cuts it, "… The rest is on winx-desktop.")
  stays read-only; a key says once "This note is longer than a mirror edits (256 KiB): edit it on winx-desktop." A
  note the person makes longer than 256 KiB on the mirror says once "This note is past 256 KiB, more than a mirror
  edits: what you add stays here. Shorten it, or edit it on winx-desktop." and sends nothing until it is shorter.
- **Ctrl+Z** in a note's text is its editor's, as locally: it undoes the last change of the text, the person's or one
  that came from home, and the result goes home as typing does. The tab and menu acts below leave the mirror's undo
  mark ("On a mirror, Ctrl+Z undoes moves only: …").
- **The tabs:** **+** makes a new note at home in a new tab of that block (`new_note`); it shows a round trip later,
  shown and with the keyboard. **×** closes the tab at home (`close_note`): it goes at once, and comes back if home
  refuses or still shows it 2 s later; a tab whose text isn't at home yet closes once it is, its " •" showing until
  then (the same for closing the whole block). A **double click** on a tab, or **Rename…**, renames the note
  (`edit_note`'s `name`), and **Color ▸** tints it (`edit_note`'s `color`): both show at once and stand until home's
  state shows them, or 2 s.
- **A block with no note open** offers **New note** and **Open a note…**, as locally.
- **All notes…** (the block's menu and header button; the canvas menu's **Notes…**) lists the mirrored workspace's
  notes: those its blocks show ("Open") and those last closed in it ("Closed"), the most recently changed first, with
  their color's dot, found by their name (an open note's text too). A click on an open note shows its tab (the
  mirror's own view, nothing called); on a closed one, home opens it (`open_note`) in the block the dialog was asked
  from, or, from the canvas menu, in a new note block where a new note goes in the mirror's view. **New note** is the
  tabs' + (or Add ▸ Note without a block). The trash deletes the note for good at home on a second click
  (`delete_note`, a Full share). Notes closed at home before this version aren't listed: open them on winx-desktop. Like
  the Checklists dialog, it acts only while its workspace is shown ("Another workspace is shown now: nothing was
  done.").
- **Dimmed** with the rule's words ([One rule](#one-rule-for-what-the-mirror-can-do)): "Not connected."; an older
  home, "winx-desktop's Templum is older: update it to do this from here." (a key in a note says it once; Add ▸ Note
  too, since home could add a note the mirror can't type in); the trash on a Control share, "Needs a Full share: share
  again on winx-desktop.". The banner's ⓘ no longer lists editing notes.
- Selecting, Copy, Copy all, Find and Send to ▸ (the remote workspace's terminals only) stay as batch 2e made them.

**What home's person sees:** the mirror's text lands in home's editor as one step of its Ctrl+Z, home's caret and
scroll kept, as an agent's `replace_note` does; the audit log names the share. A conflict's new note is an ordinary
note of home's.

**How the text goes home** (the design; the plan has every rule):

- Home gives every note a **`rev`**: a number that grows each time its text changes (and at each `edit_note` of its text),
  starting at the Unix time in milliseconds when Templum starts, so a restarted Templum never reuses one (the request
  ids' rule). The state's `notes` carry it.
- The mirror keeps, for each note the person typed in, the text and `rev` of home's that their typing started from
  (the base). It sends `edit_note` with the **whole text** and the base's `rev`. Home writes it only when the note's
  `rev` is still that one, else refuses: **home never writes an edit made on a text it no longer has**.
- **One send at a time** per note; the next waits until home's state shows the last one (a `rev` newer than the one
  it was made on), so a state that left before the answer can't undo the person's text.
- **When home's text moved on** (a newer `rev`, or a refusal), the mirror merges: its change since the base and home's
  change since the base, each one stretch (the longest common start and end left out), go together when at least one
  character of the base lies between them (two additions at the same place: home's first); else it is a conflict and
  the person's text becomes a new note. Then it sends the merged text on home's newest `rev`.
- A clean note takes home's text as a ranged change of its editor, so the caret and the undo history stay.
- What it gives up: the whole text crosses at each send (at most 256 KiB, every half second to 2 s while typing),
  where a stream of changes would be smaller; a conflict is two notes, never a text with markers; two of the person's
  changes far apart, made within one send, count as one stretch, so a change of home's between them is a conflict
  though it touched neither; text typed in the last half second before Templum quits, or while not connected until
  Templum quits or the remote is forgotten, is lost (it lives only in memory); the first edit from a mirror drops the
  control characters a mirror never shows (a carriage return of a pasted Windows text among them); notes closed before
  this version can't be reopened from the mirror; several carets collapse to one when home's change comes in.

**Home's API** (the API's own tools, like `git`: the person's acts on their notes from another computer, not an
agent's, which writes notes with `append_note`; *the planner's call*):

| Tool | Arguments | Does | Scope |
|---|---|---|---|
| `edit_note` | `block`, `note` (a tab of that block), then any of `name`, `color` (one of six, or `none`), `text` with `rev` | Renames, tints, or replaces the whole text (at most 256 KiB) when the note's `rev` is still `rev`; refused for a note whose text the state cuts | control |
| `new_note` | `block`, then `name`, `text` | A new note as a tab of that note block, shown | control |
| `open_note` | `note`, then `block`, or `workspace` (with `x`, `y`) | Where it is open that tab is shown; else a tab of `block`, or a new note block | control |
| `close_note` | `block`, `note` | Closes that tab as its × does (a blank note goes) | control |
| `delete_note` | `note` | Deletes it for good, from every block, as the Notes dialog's trash | full |

The state's notes gain `rev`, `changed` (Unix seconds) and, for a note no block shows, `workspace`: home now remembers
the workspace whose block last showed a note (`closed_in` in `notes.json`), and the state lists those closed notes
without their text.

**Security:** a share reaches only the notes its workspace's blocks show and those last closed in it, never home's other
notes (they are app-wide); deleting takes Full; every text is capped at 256 KiB at both ends and the mirror's copy is
cleaned as before; a hostile mirror with a Control share can write over the workspace's notes as the person could, one
Ctrl+Z step each at home; a hostile home's `rev`s only decide whether its own edits apply.

#### As built (kanban card 204)

Built as the plan has it, step by step; the code's reality moved these:

- **Home:** `notes.rs` has `Entry::rev` (in memory, seeded at launch with `notes::rev_seed()`, the Unix time in ms,
  before `notes_saved` is cloned), `Notes::stamp`, `touch`, `close_in`, `forget_workspace`, and `splice`, `merge` and
  `shift` (pure, tested headlessly). `closed_in` is marked by `Workspace::note_closed` (a blank note goes; any other is
  remembered) from the tab's ×, a closed block and `close_note`; a deleted workspace forgets its marks. `note::write`
  is the one way a text goes into an editor (home's `edit_note`, and the mirror for every text of home's): one ranged
  change, one Ctrl+Z step, the caret moved by `shift`, the scroll set back (`set_scroll_offset` is deferred by
  gpui-base to the next layout; whether it wins over `move_to`'s own scroll was not looked at by hand, see the minor).
  The five tools are `Own::EditNote…DeleteNote` in the registry (16 own tools; `rev` is a non-target argument),
  dispatched by `api_call` to `Workspace::api_note` (`note_tabs.rs`), which answers with the block as its target.
- **Confinement** (`api_allows`): `open_note` and `delete_note` check the note against the *workspaces' data*
  (`note_seen`: open in a block the token sees, or no block and `closed_in` a workspace it sees), not against
  `api_scope`'s state, which carries no notes (*the builder's call*). A note a token doesn't reach is "There's no note
  9…", as one that isn't there. A block of a mirrored workspace gets `Space::mirror_words` as an `isError` answer; an
  unknown block is the `not_found` error. `edit_note` refuses a text on a stale `rev` and on a note longer than 256 KiB
  before it changes anything, so a call that fails changes nothing; its longer-than-API sentence says "edit it in
  Templum on the computer that holds it" (home doesn't know its own name for it).
- **`Workspace::open_note` does nothing on a mirror** (a guard added: with the rule now open, this computer's note of
  home's id would have been opened in a block of the mirror). A dialog's click goes to `open_note_here`, which shows
  the tab of an open note on a mirror (no call) and else calls home.
- **The mirror's rule:** `Deed::AddNote` is `add_block` (older, too, when home lacks `edit_note`), `EditNote` is
  `edit_note`, `OpenNote` is `open_note`, new `DeleteNote` is `delete_note`; `Nay::NotesReadOnly` is gone and so is
  "editing notes" from the banner's ⓘ (`NOT_MIRRORED`).
- **The copy:** `note_cut_of` keeps `rev`, `changed` and `closed_in` (the state's `workspace`); `notes_cut` names the notes
  whose text is cut; `mirror_notes(state, now)` lays the rename and color holds (`lay_note_holds`) and `remirror` calls
  it; closed notes are in the copy with no editor. `clean_note_text` is the one cleaning (also run on what is typed,
  step 0).
- **`NoteEdit` and `note_steps`** are `workspace/remote_notes.rs`, as the plan's §6.4. Two things differ. A change of
  home's waits for the person's pause *before* it is merged (the plan's order merged at once; the spec says the editor
  waits), so step 5 returns while `now < typed + PAUSE` unless the edit is flushed. And step 7 waits on any rung of the
  rule (not connected, an older home, a share without Control), not only "not connected": nothing can be sent in any of
  them, and the text stays. The `NoteEdit` also keeps the note's last name (for the new note's name when the note is
  gone). A refused copy of the person's text (`stuck`) keeps the text in the editor and says home's sentence; the note
  takes no keys (a key says "Your version of … couldn't be kept on …", once) and its " •" stays. The copy is tried
  again once the remote is live and `HOLD` has passed, until it was refused `TRIES` times in a row; a key starts the
  tries again. A send whose answer was lost (the client's `Refusal::maybe_done`: the call was sent and no answer
  came) leaves the sent text as `doubt` and the base alone: home's next text says whether it landed (it is the
  doubt: the base moves, nothing is merged), didn't (it is the base: the text goes again on the new rev) or
  neither (the person's text is kept as a new note) — a merge on the old base would put the sent text in twice.
  A copy whose answer was lost the same way is not retried by itself (home may have made it: a second would be a
  duplicate note): only a key tries again. The wake is set only for due times still ahead of the step's `now`: a due
  already past has had its step, and what the edit waits on is an event, not time (an answer, home's state, a key),
  so a wake for it would run `note_steps` without end. The remote going live and `Update::Me` run `note_steps`
  themselves, so unsent text and a copy to retry move on at once.
  Past 256 KiB (step 1b only flags it) a note home still shows is neither sent nor copied by a conflict (home
  would refuse both), says its sentence once and stays editable, so the person can shorten it; what only moves the
  base (home's changes, a send whose answer was lost) still runs. A past-cap note home closed, or whose block home
  closed, is kept whatever its size: the copy goes by `add_block` (which takes what the body holds, `new_note` refuses
  256 KiB), so it lands as "a new note where it was" and not as a tab of the same block. A note home shows again
  while it has an edit gets the edit's editor back, not a new one on home's text; once the copy has landed, a
  note shown by a block gets home's text written into that editor, whatever the copy's reason.
  `note::write` commits a composition pending (a dead key, an input method) as plain text before it splices, since a
  ranged write would land over its marked range. Opening a note in a block clears its `closed_in` (`Notes::opened`,
  in `note_tab`), so a workspace deleted with the tab open doesn't hand the note to another workspace's share. When
  home's block is gone too the copy goes in by `add_block`, which has no name: the sentence says "a new note where it
  was". A key in a note before the share is known (`me` not read yet) says "Not connected", and a share below
  Control says the rule's "Needs a Full share…". A note cut at home while typed in becomes a copy with its own sentence ("… is longer
  than a mirror edits (256 KiB): your version is the new note …"; not in the spec's list: *the builder's call*).
- **The door** is `remote_note_act` (`NoteAct::New/Close/Rename/Color/Open/Delete`); holds `notes_renamed`,
  `notes_colored` and `tabs_closed` are laid in `mirror_notes` and `lay_note_tabs` and ended by home's state,
  `expire_holds` or a refusal. `Close` of a tab, and closing a note block (`remote_close`), wait for a `NoteEdit` through
  `close_when_saved` (`RemoteSpace::closing`). `tab_awaited` shows the new tab asked of `new_note` or `open_note`
  and gives it the keyboard when it comes (`show_awaited_tab`, 5 s). The rename field, the Rename… prompt and the Notes
  dialog carry their workspace (`Workspace::still_on`, moved from `checklist.rs`). `open_notes` takes the workspace;
  its rows on a mirror are the notes its blocks show ("Open") and those closed in it ("Closed"); the trash on a Control
  share says "Needs a Full share…" through the door.
- **Not done by hand:** the whole feature was checked headlessly (below); nothing was looked at in two `--dev`
  Templums, the scroll of `note::write` wasn't compared against `append_note`'s, and `--bench-mirror` wasn't run.
  They are in [minors.md](../minors.md).

Tests: `notes.rs` (rev, `closed_in`, `splice`, `merge`, `shift`), the registry's scope and `the_note_tools_parse_and_say_what_is_missing`,
`a_limited_caller_sees_the_closed_notes_of_its_workspaces_only` (model.rs); tests/api.rs
`edit_note_writes_on_its_rev_only_and_keeps_home_s_caret`, `new_close_open_and_delete_a_note_through_the_api`,
`a_limited_token_reaches_only_its_workspace_s_notes`, `a_deleted_workspace_s_closed_notes_are_no_new_workspace_s`, `a_note_opened_again_is_closed_in_no_workspace`;
tests/remote_notes.rs (the plan's 1–17: typing and its pause, `MOST_WAIT`, the caret, the merge, both-in-one-place, a
refused edit, a state older than the answer, a note closed at home, a cut or too long note, not connected, the tabs,
unsaved close, the new tab, the dialog, a local dialog over a mirror, an older home, Add ▸ Note; and, after review, a send whose
answer was lost, a composition pending, the words of a locked key, a refused copy tried again); `remote.rs`'s
`notes_and_adds_are_home_s_calls` (the `Deed` words, an older home); and in mirror_end_to_end.rs the plan's 20–22 over
loopback (the plan's 18, `notes.json` byte for byte, is in the third: it makes, types, renames, colors, closes and
reopens a note and checks the mirror's `notes.json` and `ws.notes`).

