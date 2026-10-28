//! How Templum was installed, read from the running executable, and the release files that fit
//! it (docs/specs/updates.md, How Templum was installed).

use std::path::{Path, PathBuf};

use crate::{Asset, Release};

/// How Templum was installed: what an update replaces, and with which file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// The `.AppImage` file.
    AppImage(PathBuf),
    /// A bare executable, on Linux or Windows.
    Portable(PathBuf),
    /// The executable Windows' NSIS installer put.
    Nsis(PathBuf),
    /// The executable Windows' MSI put.
    Msi(PathBuf),
    /// The `.app` bundle.
    MacApp(PathBuf),
    /// The executable dpkg installed.
    Deb(PathBuf),
    /// The executable pacman installed.
    Pacman(PathBuf),
    /// Templum can't replace itself.
    Manual(Why),
}

/// Why Templum can't replace itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Built from source with Cargo.
    Source,
    /// Installed by a package manager Templum doesn't know.
    Package,
    /// macOS: run from the disk image, or from the copy macOS runs an app from Downloads in (App
    /// Translocation).
    NotInstalled,
}

impl Install {
    /// This Templum's.
    pub fn detect() -> Install {
        let Ok(exe) = std::env::current_exe() else {
            return Install::Manual(Why::Package);
        };
        // A link (~/.local/bin/templum) is followed to the file it names. Not on Windows, where
        // that path starts with \\?\, which PowerShell can't start (GPUI's restart).
        let canonical = |path: PathBuf| {
            if cfg!(unix) {
                path.canonicalize().unwrap_or(path)
            } else {
                path
            }
        };
        let exe = canonical(exe);
        let appimage = std::env::var_os("APPIMAGE")
            .zip(std::env::var_os("APPDIR"))
            .map(|(file, dir)| (PathBuf::from(file), canonical(dir.into())));
        detect_in(std::env::consts::OS, &exe, appimage, Path::new("/"))
    }

    /// The release's files it takes, on `os` with an `arch` processor (`std::env::consts`'s).
    pub fn files<'a>(
        &self,
        release: &'a Release,
        os: &str,
        arch: &str,
    ) -> Result<Vec<&'a Asset>, String> {
        let v = &release.version;
        // The packagers' names for the processor: Debian's, Windows' and cargo-packager's dmg.
        let (deb, win, mac) = match arch {
            "x86_64" => ("amd64", "x64", "x64"),
            "aarch64" => ("arm64", "arm64", "aarch64"),
            _ => return Err(format!("Templum isn't built for {arch} processors")),
        };
        let names = match self {
            Install::AppImage(_) => vec![format!("templum_{v}_{arch}.AppImage")],
            Install::Deb(_) => vec![format!("templum_{v}_{deb}.deb")],
            Install::Pacman(_) => {
                vec!["PKGBUILD".to_string(), format!("templum_{v}_{arch}.tar.gz")]
            }
            Install::Nsis(_) => vec![format!("templum_{v}_{win}-setup.exe")],
            Install::Msi(_) => vec![format!("templum_{v}_{win}_en-US.msi")],
            Install::Portable(_) if os == "windows" => {
                vec![format!("templum-{arch}-pc-windows-msvc.exe")]
            }
            Install::Portable(_) => vec![format!("templum-{arch}-unknown-linux-gnu.tar.gz")],
            Install::MacApp(_) => vec![format!("Templum_{v}_{mac}.dmg")],
            Install::Manual(_) => Vec::new(),
        };
        names.iter().map(|name| release.asset(name)).collect()
    }

    /// What an update renames a new file over: the file or bundle the person opens. None when an
    /// installer or a package manager replaces it.
    pub fn target(&self) -> Option<&Path> {
        match self {
            Install::AppImage(path) | Install::Portable(path) | Install::MacApp(path) => Some(path),
            _ => None,
        }
    }
}

/// How the executable at `exe` was installed, on `os` (`std::env::consts::OS`). `appimage` is what
/// the AppImage runtime sets: `APPIMAGE` (the file) and `APPDIR` (where it is mounted); `root` is
/// where `/var/lib` is looked for (`/`, a folder in tests).
pub fn detect_in(
    os: &str,
    exe: &Path,
    appimage: Option<(PathBuf, PathBuf)>,
    root: &Path,
) -> Install {
    // `target/release/templum`, `target/<triple>/debug/templum`: Cargo marks its folder.
    if exe
        .ancestors()
        .skip(1)
        .take(3)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| dir.join("CACHEDIR.TAG").is_file())
    {
        return Install::Manual(Why::Source);
    }
    let exe = exe.to_path_buf();
    match os {
        "linux" => {
            // Every program started from an AppImage's terminal inherits its variables (Cursor's):
            // they are Templum's only when it runs from the mounted image.
            if let Some((file, dir)) = appimage
                && exe.starts_with(&dir)
            {
                return Install::AppImage(file);
            }
            if !exe.starts_with("/usr") {
                return Install::Portable(exe);
            }
            let lib = root.join("var/lib");
            if lib.join("dpkg/info/templum.list").is_file() {
                Install::Deb(exe)
            } else if pacman_has(&lib.join("pacman/local")) {
                Install::Pacman(exe)
            } else {
                Install::Manual(Why::Package)
            }
        }
        "windows" => {
            if exe.with_file_name("uninstall.exe").is_file() {
                Install::Nsis(exe)
            } else if exe
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("program files")
            {
                Install::Msi(exe)
            } else {
                Install::Portable(exe)
            }
        }
        "macos" => {
            let path = exe.to_string_lossy();
            if path.starts_with("/Volumes/") || path.contains("/AppTranslocation/") {
                return Install::Manual(Why::NotInstalled);
            }
            // <bundle>.app/Contents/MacOS/templum
            let bundle = exe
                .parent()
                .filter(|dir| dir.ends_with("Contents/MacOS"))
                .and_then(Path::parent)
                .and_then(Path::parent)
                .filter(|bundle| bundle.extension().is_some_and(|e| e == "app"));
            match bundle {
                Some(bundle) => Install::MacApp(bundle.to_path_buf()),
                None => Install::Manual(Why::Package),
            }
        }
        _ => Install::Manual(Why::Package),
    }
}

/// pacman's database has a `templum-<version>-<release>` folder for an installed `templum`.
fn pacman_has(local: &Path) -> bool {
    std::fs::read_dir(local)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("templum-"))
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::fixture;

    fn at(os: &str, exe: &str) -> Install {
        detect_in(os, Path::new(exe), None, Path::new("/nonexistent"))
    }

    #[test]
    fn a_build_from_source_is_told_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("target/x86_64-unknown-linux-gnu/release")).unwrap();
        fs::write(dir.path().join("target/CACHEDIR.TAG"), "").unwrap();
        for exe in [
            "target/release/templum",
            "target/debug/templum.exe",
            "target/x86_64-unknown-linux-gnu/release/templum",
        ] {
            for os in ["linux", "windows", "macos"] {
                assert_eq!(
                    detect_in(os, &dir.path().join(exe), None, dir.path()),
                    Install::Manual(Why::Source),
                    "{os} {exe}"
                );
            }
        }
    }

    #[test]
    fn linux_installations() {
        let appimage = PathBuf::from("/home/ada/Apps/templum.AppImage");
        let mounted = PathBuf::from("/tmp/.mount_templuX");
        assert_eq!(
            detect_in(
                "linux",
                &mounted.join("usr/bin/templum"),
                Some((appimage.clone(), mounted)),
                Path::new("/")
            ),
            Install::AppImage(appimage)
        );
        // Run from another AppImage's terminal: its variables aren't Templum's.
        let cursor = (
            PathBuf::from("/home/ada/Apps/Cursor.AppImage"),
            PathBuf::from("/tmp/.mount_CursorY"),
        );
        assert_eq!(
            detect_in(
                "linux",
                Path::new("/usr/bin/templum"),
                Some(cursor.clone()),
                Path::new("/nonexistent")
            ),
            Install::Manual(Why::Package)
        );
        assert_eq!(
            detect_in(
                "linux",
                Path::new("/home/ada/bin/templum"),
                Some(cursor),
                Path::new("/")
            ),
            Install::Portable("/home/ada/bin/templum".into())
        );
        assert_eq!(
            at("linux", "/home/ada/bin/templum"),
            Install::Portable("/home/ada/bin/templum".into())
        );
        assert_eq!(
            at("linux", "/usr/bin/templum"),
            Install::Manual(Why::Package)
        );

        let root = tempfile::tempdir().unwrap();
        let lib = root.path().join("var/lib");
        fs::create_dir_all(lib.join("pacman/local/templum-extras-1.0-1")).unwrap();
        // The AUR's: its helper updates it, not the release's PKGBUILD (a package that conflicts).
        fs::create_dir_all(lib.join("pacman/local/templum-bin-0.1.4-1")).unwrap();
        let exe = Path::new("/usr/bin/templum");
        assert_eq!(
            detect_in("linux", exe, None, root.path()),
            Install::Manual(Why::Package),
            "another package's folder"
        );
        fs::create_dir_all(lib.join("pacman/local/templum-0.1.1-1")).unwrap();
        assert_eq!(
            detect_in("linux", exe, None, root.path()),
            Install::Pacman(exe.into())
        );
        fs::create_dir_all(lib.join("dpkg/info")).unwrap();
        fs::write(lib.join("dpkg/info/templum.list"), "/usr/bin/templum\n").unwrap();
        assert_eq!(
            detect_in("linux", exe, None, root.path()),
            Install::Deb(exe.into())
        );
    }

    #[test]
    fn windows_installations() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("templum.exe");
        assert_eq!(
            detect_in("windows", &exe, None, dir.path()),
            Install::Portable(exe.clone())
        );
        fs::write(dir.path().join("uninstall.exe"), "").unwrap();
        assert_eq!(
            detect_in("windows", &exe, None, dir.path()),
            Install::Nsis(exe.clone())
        );
        assert_eq!(
            at("windows", r"C:\Program Files\Templum\templum.exe"),
            Install::Msi(r"C:\Program Files\Templum\templum.exe".into())
        );
    }

    #[test]
    fn macos_installations() {
        assert_eq!(
            at("macos", "/Applications/Templum.app/Contents/MacOS/templum"),
            Install::MacApp("/Applications/Templum.app".into())
        );
        for exe in [
            "/Volumes/Templum/Templum.app/Contents/MacOS/templum",
            "/private/var/folders/x/T/AppTranslocation/4A1B/d/Templum.app/Contents/MacOS/templum",
        ] {
            assert_eq!(at("macos", exe), Install::Manual(Why::NotInstalled));
        }
        assert_eq!(
            at("macos", "/usr/local/bin/templum"),
            Install::Manual(Why::Package)
        );
    }

    #[test]
    fn each_installation_takes_its_file() {
        let release = fixture();
        let p = PathBuf::new;
        let cases = [
            (
                Install::AppImage(p()),
                "linux",
                "x86_64",
                &["templum_0.1.1_x86_64.AppImage"][..],
            ),
            (
                Install::AppImage(p()),
                "linux",
                "aarch64",
                &["templum_0.1.1_aarch64.AppImage"],
            ),
            (
                Install::Deb(p()),
                "linux",
                "x86_64",
                &["templum_0.1.1_amd64.deb"],
            ),
            (
                Install::Deb(p()),
                "linux",
                "aarch64",
                &["templum_0.1.1_arm64.deb"],
            ),
            (
                Install::Pacman(p()),
                "linux",
                "x86_64",
                &["PKGBUILD", "templum_0.1.1_x86_64.tar.gz"],
            ),
            (
                Install::Portable(p()),
                "linux",
                "x86_64",
                &["templum-x86_64-unknown-linux-gnu.tar.gz"],
            ),
            (
                Install::Portable(p()),
                "linux",
                "aarch64",
                &["templum-aarch64-unknown-linux-gnu.tar.gz"],
            ),
            (
                Install::Portable(p()),
                "windows",
                "x86_64",
                &["templum-x86_64-pc-windows-msvc.exe"],
            ),
            (
                Install::Nsis(p()),
                "windows",
                "x86_64",
                &["templum_0.1.1_x64-setup.exe"],
            ),
            (
                Install::Msi(p()),
                "windows",
                "x86_64",
                &["templum_0.1.1_x64_en-US.msi"],
            ),
            (
                Install::MacApp(p()),
                "macos",
                "aarch64",
                &["Templum_0.1.1_aarch64.dmg"],
            ),
            (
                Install::MacApp(p()),
                "macos",
                "x86_64",
                &["Templum_0.1.1_x64.dmg"],
            ),
            (Install::Manual(Why::Source), "linux", "x86_64", &[]),
        ];
        for (install, os, arch, names) in cases {
            let files = install.files(&release, os, arch).unwrap();
            let got: Vec<_> = files.iter().map(|a| a.name.as_str()).collect();
            assert_eq!(got, names, "{install:?} {os} {arch}");
        }
    }

    #[test]
    fn a_missing_file_or_processor_is_an_error() {
        let release = fixture();
        let missing = Install::Pacman(PathBuf::new()).files(&release, "linux", "aarch64");
        assert_eq!(
            missing.unwrap_err(),
            "Templum 0.1.1 has no file for this system (templum_0.1.1_aarch64.tar.gz)"
        );
        let windows_arm = Install::Nsis(PathBuf::new()).files(&release, "windows", "aarch64");
        assert!(windows_arm.is_err());
        let x86 = Install::Portable(PathBuf::new()).files(&release, "linux", "x86");
        assert_eq!(x86.unwrap_err(), "Templum isn't built for x86 processors");
    }
}
