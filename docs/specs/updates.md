# Updates from GitHub Releases

- **Date:** 2026-10-14
- **Status:** Implemented
- **Builds on:** the release workflow (`.github/workflows/release.yml`), [settings.md](settings.md),
  [config.md](config.md) and the notifications of [shell-integration.md](shell-integration.md)

## Goal

A person runs the newest Templum without going to the releases page. Templum finds the newest release on GitHub,
takes the file in it made for their system **and for the way Templum was installed** (an AppImage is replaced by an
AppImage, a `.deb` by a `.deb`), downloads it in the background, checks it, and asks only when to restart.

**Success criteria**

1. A Templum older than the newest release downloads it by itself and offers **Restart to update**.
2. After the restart the new version runs, with the canvas as it was left.
3. A Templum installed by a package manager that needs `sudo` (`.deb`, pacman) gets the command typed in a terminal.
4. A Templum it can't replace (another package manager, run from the disk image) tells that a version is out and
   links to it; a build from source checks only on demand.
5. **Settings ▸ Updates** shows the version, checks on demand, and turns the automatic check off.
6. A file whose checksum doesn't match GitHub's is never run or put in place.

## Out of scope

- Prereleases (`-rc` tags): `releases/latest` lists only stable releases.
- A signature of Templum's own (minisign): the trust is GitHub's, over HTTPS, with the SHA-256 GitHub lists for
  each file.
- Updating while running: the new version takes over on the next start.

## Where the release comes from

`GET https://api.github.com/repos/KingDasWinx/Templum/releases/latest`, with a `User-Agent` (GitHub rejects requests
without one). It is the newest release that is neither a draft nor a prerelease. Without a token GitHub allows 60
requests per hour per address; Templum asks once at start and once a day.

Each asset of the answer has a `digest` (`sha256:…`, GitHub computes it on upload). A file is downloaded to
`<name>.part`, hashed as it arrives, and kept only if the hash matches; an asset without a digest is refused. A file
already downloaded with the right hash isn't downloaded again.

## How Templum was installed

Read once at start from the running executable (`templum-update`'s `install.rs`):

| Installed as | Recognized by | File taken | Put in place |
|---|---|---|---|
| Built from source | a `CACHEDIR.TAG` (Cargo's) up to 3 folders above the executable | — | told only |
| AppImage | `APPIMAGE` and `APPDIR` (set by the AppImage runtime), the executable inside `APPDIR` | `templum_<v>_<arch>.AppImage` | renamed over the AppImage |
| `.deb` | an executable under `/usr` and `/var/lib/dpkg/info/templum.list` | `templum_<v>_<amd64\|arm64>.deb` | `sudo apt install ./<deb>` typed in a terminal |
| pacman | an executable under `/usr` and `/var/lib/pacman/local/templum-<version>` | `PKGBUILD` and `templum_<v>_x86_64.tar.gz` | `makepkg -si` typed in a terminal |
| Another package manager | an executable under `/usr` without either (the AUR's `templum-bin`: its folder is `templum-bin-<version>`) | — | told only |
| Linux, portable | anything else on Linux | `templum-<arch>-unknown-linux-gnu.tar.gz` (its `templum`) | renamed over the executable |
| Windows installer (NSIS) | `uninstall.exe` beside the executable | `templum_<v>_x64-setup.exe` | run with `/P /R /NS` after Templum quits |
| Windows installer (MSI) | an executable under `Program Files` | `templum_<v>_x64_en-US.msi` | `msiexec /i <msi> /passive` after Templum quits |
| Windows, portable | anything else on Windows | `templum-x86_64-pc-windows-msvc.exe` | the running one moved aside, the new one in its place |
| macOS app | an executable in `<name>.app/Contents/MacOS` | `Templum_<v>_<aarch64\|x64>.dmg` | the dmg's app copied beside the bundle, then swapped |
| macOS, not installed | a path under `/Volumes/` or in `AppTranslocation` | — | told only: move Templum to Applications |

- **Another AppImage's variables.** A program started from an AppImage's terminal (Cursor's) inherits its
  `APPIMAGE`: Templum trusts it only when its own executable is inside `APPDIR`, the image mounted, so it never
  renames itself over another app.
- **Renames, never writes over.** A running AppImage reads through the file it opened, a running Windows executable
  can be renamed but not deleted, and macOS kills an app whose signed code is written over. So the new file is
  written beside the old one under a hidden name (`.<name>.update`) and renamed into place. On Unix one rename
  replaces a file; a running Windows executable and a macOS bundle (a folder) are moved aside first (`.<name>.old`),
  deleted at the next start.
- **When.** Files to rename over are swapped when Templum quits, for any reason: closing the window after a download
  is enough (the workspace is released before GPUI quits, so the swap runs from both). Installers run only after
  **Restart**, once GPUI's loop has ended (starting a process while it shuts down can re-enter it on Windows): the
  NSIS installer in passive mode closes every running Templum and opens the new one (`/R`), without making
  shortcuts again (`/NS`). `msiexec` doesn't open Templum again.
- **Two Templums.** Each writes its downloads and unpackings under a name with its process id, then renames: one
  never truncates the other's file.
- **macOS:** `hdiutil attach -nobrowse -readonly -noautoopen` the dmg in the download folder, `ditto` its app to
  `.<bundle>.update` beside the bundle, `hdiutil detach`. A file Templum downloads itself gets no quarantine flag, so
  Gatekeeper doesn't ask again.
- **Downloads** for installers and packages go to `<data>/updates/<version>/`; folders of versions up to the running
  one are deleted at start, with a swap's `.<name>.old` and the unfinished files beside the target an hour old or
  more. A ready `.<name>.update` stays: its download is skipped next time.

## What a person sees

- **Automatically** (`[updates] check = true`): at start and every 24 hours, and when the switch is turned on. A
  newer release is downloaded at once, in the background; a failed automatic check (no network) says nothing and
  keeps a version known to be out. A build from source checks only with **Check now**: every `cargo run` would ask
  GitHub, which allows 60 requests an hour.
- **The title bar** shows a pill left of the usage meter while there is news:
  - downloading: "0.2.0 · 45%";
  - ready: **Restart to update** (Restart), or **Install 0.2.0** for `.deb` and pacman (then Restart);
  - told only, or the download failed: "0.2.0 available", which opens the release page.
- **A notification**, once per version (the last news told is kept in `<data>/updates/told`, so a restart doesn't
  tell it again): "Templum 0.2.0 is ready" with **Restart** and **What's new** (the release
  page); "Templum 0.2.0 is downloaded" with **Install** for `.deb` and pacman; "Templum 0.2.0 is out" with **What's
  new** when Templum can't replace itself, saying why; "Couldn't update to 0.2.0" with the error.
- **Restart** asks first when a terminal is busy (an agent or a command running in it: Templum reopens the
  terminals, not the programs in them), and about unsaved files as quitting does. Then Templum saves, quits, and the
  new version opens (GPUI's restart: a script waits for the process to end, then starts it).
- **Install** opens a terminal in the download folder with the command typed, not run: the person reads it, presses
  Enter and types their password.

### Settings ▸ Updates

| Row | Control | Key |
|---|---|---|
| Version | the running version; below, the state ("Up to date · checked 14:02", "Downloading 0.2.0… 45%", "0.2.0 is ready", the error); **Check now**, or **Restart** / **Install** when ready | — |
| Check for updates automatically | switch | `updates.check` |

```toml
[updates]
check = true    # look for a new version at start and once a day, and download it
```

**Check now** works with the switch off and always answers: "Templum is up to date" or the error.

## Architecture

| File | Change |
|---|---|
| `templum-update` (new crate, no UI) | `lib.rs`: the release (`parse`, `latest`, `newer_than`), the HTTP agent (`ureq`); `install.rs`: how Templum was installed and the files that fit; `fetch.rs`: download and checksum; `apply.rs`: `prepare`, `swap`, `cleanup`, the installers' command lines |
| `templum/src/workspace/update.rs` (new) | The state, checks, downloads, notifications, Restart and Install, the swap on quit |
| `templum/src/update.rs` (new) | The title bar's pill |
| `templum/src/config.rs`, `settings/` | `[updates]`; Settings ▸ Updates |
| `deny.toml` | `CDLA-Permissive-2.0`: the license of `webpki-roots`, Mozilla's list of certificate authorities that `ureq` trusts |

HTTP is `ureq` (blocking, rustls with ring, the proxy from `HTTPS_PROXY`), run on GPUI's background executor.
Without a data folder (`--bench`) and in tests nothing is checked.

## Testing

**Unit (`templum-update`):** a real `releases/latest` answer parses into the version, every asset and its digest;
version order (`0.2.0-rc.1` before `0.2.0`); each installation is recognized from its layout; each installation and
architecture takes its file from the v0.1.1 names, and a missing one is an error; a download from a local HTTP server
follows a redirect, reports progress, keeps a file with the right digest, leaves nothing for a wrong one and skips a
file already there; the portable archive gives only `templum`, executable; a swap replaces a file and a folder and
keeps the old one aside where needed; cleanup keeps newer versions' downloads, a ready download and another
Templum's unfinished one; another AppImage's variables aren't Templum's; the installers' and terminals' command
lines.

**Headless UI:** Settings ▸ Updates writes `check = false`; a ready update shows the pill and one notification per
version; news told is remembered by the next start; a release Templum can't install shows "available"; Install opens
a terminal and the pill says Restart; closing the window swaps the files.

**Manual:** a release build with its version set to `0.1.0`, copied out of `target/` and run, downloads v0.1.1 and
restarts into it; the same with `APPIMAGE` pointing at a file; on Windows, the portable executable and the NSIS
installer.
