## Install

**Linux and macOS**, in a terminal:

```sh
curl -fsSL https://github.com/KingDasWinx/Templum/releases/latest/download/install.sh | sh
```

**Windows**, in PowerShell:

```powershell
irm https://github.com/KingDasWinx/Templum/releases/latest/download/install.ps1 | iex
```

They install the newest release. For this one, put `TEMPLUM_VERSION={version}` before `sh`
(`curl … | TEMPLUM_VERSION={version} sh`), or run `$env:TEMPLUM_VERSION = "{version}"` before `irm`.

**Arch Linux**, from the AUR ([`templum-bin`](https://aur.archlinux.org/packages/templum-bin)): `yay -S templum-bin`
(or `paru -S templum-bin`). Your AUR helper updates it.

## Downloads

An **installer** puts Templum in your system, with its shortcuts; a **portable** file runs from wherever you keep it.
Each one updates itself (for a `.deb` or the `PKGBUILD`, Templum types the install command in a terminal, for your
password).

| System | File | Kind | What it's for |
|---|---|---|---|
| Windows x64 | `templum_{version}_x64-setup.exe` | Installer | For you only, no administrator: Start menu and desktop shortcuts. The one `install.ps1` runs. |
| Windows x64 | `templum_{version}_x64_en-US.msi` | Installer | For every user of the computer, in Program Files; asks for an administrator. |
| Windows x64 | `templum-x86_64-pc-windows-msvc.exe` | Portable | The bare executable: run it from any folder, nothing installed. |
| macOS, Apple Silicon | `Templum_{version}_aarch64.dmg` | Installer | Open it and drag Templum to Applications. |
| macOS, Intel | `Templum_{version}_x64.dmg` | Installer | The same, for an Intel Mac. |
| Debian, Ubuntu… | `templum_{version}_amd64.deb`, `templum_{version}_arm64.deb` (ARM64) | Installer | `sudo apt install ./templum_{version}_amd64.deb` |
| Arch Linux | `PKGBUILD` | Installer | `makepkg -si` in its folder: it downloads `templum_{version}_x86_64.tar.gz`, the package's files. |
| Linux | `templum_{version}_x86_64.AppImage`, `templum_{version}_aarch64.AppImage` (ARM64) | Portable | One file for any distribution: `chmod +x` it and open it. |
| Linux | `templum-x86_64-unknown-linux-gnu.tar.gz`, `templum-aarch64-unknown-linux-gnu.tar.gz` (ARM64) | Portable | The bare executable, to put anywhere. The one `install.sh` installs. |

Each file has its SHA-256 in a `.sha256` beside it. The builds aren't signed by an Apple or a Microsoft account yet,
so a download asks once: on macOS, *System Settings ▸ Privacy & Security ▸ Open Anyway*; on Windows, SmartScreen's
*More info ▸ Run anyway*. The one-line install on macOS doesn't ask.
