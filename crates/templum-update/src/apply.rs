//! A downloaded version put in place: written beside what it replaces and renamed over it when
//! Templum quits, or an installer or a command left for later (docs/specs/updates.md, How Templum
//! was installed).

use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use semver::Version;
use ureq::Agent;

use crate::fetch::{cant_write, download, own};
use crate::{Install, Release};

/// A version downloaded and ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ready {
    /// `new` takes `target`'s place when Templum quits ([`swap`]); `restart` opens it.
    Swap {
        new: PathBuf,
        target: PathBuf,
        restart: PathBuf,
    },
    /// An installer that replaces Templum once it has quit.
    Installer(Launch),
    /// A command the person runs in a terminal in `folder` (it asks for their password);
    /// `restart` opens the Templum it installs.
    Terminal {
        folder: PathBuf,
        command: String,
        restart: PathBuf,
    },
}

/// A program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl Launch {
    /// Starts it without waiting: it outlives Templum.
    pub fn spawn(&self) -> io::Result<()> {
        Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// Downloads `release`'s files for `install` and gets them ready. A file renamed over its target
/// is written beside it, under a hidden name; installers and packages go to `updates/<version>`.
/// `progress` hears the bytes received and the total.
pub fn prepare(
    agent: &Agent,
    install: &Install,
    release: &Release,
    updates: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Ready, String> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    prepare_on(agent, install, release, updates, os, arch, progress)
}

fn prepare_on(
    agent: &Agent,
    install: &Install,
    release: &Release,
    updates: &Path,
    os: &str,
    arch: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Ready, String> {
    if let Install::Manual(_) = install {
        return Err("This Templum can't replace itself".to_string());
    }
    let files = install.files(release, os, arch)?;
    let folder = updates.join(release.version.to_string());
    let total = files.iter().map(|a| a.size).sum();
    let mut done = 0;
    let mut paths = Vec::new();
    for asset in &files {
        let to = match install {
            Install::AppImage(target) => beside(target, "update"),
            Install::Portable(target) if os == "windows" => beside(target, "update"),
            _ => folder.join(&asset.name),
        };
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(|e| cant_write(dir, &e))?;
        }
        download(agent, asset, &to, &mut |got| progress(done + got, total))?;
        done += asset.size;
        paths.push(to);
    }
    let swap = |new: PathBuf, target: &Path| Ready::Swap {
        new,
        target: target.to_path_buf(),
        restart: target.to_path_buf(),
    };
    let first = paths.swap_remove(0);
    Ok(match install {
        Install::AppImage(target) => {
            executable(&first)?;
            swap(first, target)
        }
        Install::Portable(target) if os == "windows" => swap(first, target),
        Install::Portable(target) => {
            let new = beside(target, "update");
            unpack(&first, &new)?;
            swap(new, target)
        }
        Install::MacApp(bundle) => {
            let new = beside(bundle, "update");
            copy_app(&first, &folder.join("mount"), &new)?;
            swap(new, bundle)
        }
        Install::Nsis(_) => Ready::Installer(Launch {
            program: first,
            // Passive (progress only), Templum opened again, no shortcuts made again.
            args: ["/P", "/R", "/NS"].map(OsString::from).to_vec(),
        }),
        Install::Msi(_) => Ready::Installer(Launch {
            program: "msiexec".into(),
            args: vec!["/i".into(), first.into(), "/passive".into()],
        }),
        Install::Deb(exe) => Ready::Terminal {
            folder,
            command: format!("sudo apt install ./{}", files[0].name),
            restart: exe.clone(),
        },
        Install::Pacman(exe) => Ready::Terminal {
            folder,
            command: "makepkg -si".to_string(),
            restart: exe.clone(),
        },
        Install::Manual(_) => unreachable!("refused above"),
    })
}

/// Puts `new` in `target`'s place. On Unix one rename replaces a file. A running Windows
/// executable can be renamed but not replaced, and a macOS bundle is a folder: they are moved
/// aside first, then deleted (while it runs, a Windows one at the next start: [`cleanup`]).
pub fn swap(new: &Path, target: &Path) -> io::Result<()> {
    if cfg!(unix) && !target.is_dir() {
        return fs::rename(new, target);
    }
    let old = beside(target, "old");
    remove(&old);
    fs::rename(target, &old)?;
    if let Err(e) = fs::rename(new, target) {
        let _ = fs::rename(&old, target);
        return Err(e);
    }
    remove(&old);
    Ok(())
}

/// Deletes what an earlier update left: the old version a swap moved aside, files an interrupted
/// download or unpacking left beside the target, and the downloads of versions up to `current`
/// (installed, or passed by). A ready `.<name>.update` stays: its download isn't done again.
pub fn cleanup(install: &Install, updates: &Path, current: &str) {
    if let Some(target) = install.target() {
        remove(&beside(target, "old"));
        // Another Templum's may be on its way: only an hour old or more.
        let unfinished = beside(target, "update.");
        let unfinished = unfinished.file_name().unwrap_or_default().to_string_lossy();
        let dir = target.parent().unwrap_or(Path::new("."));
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|at| at.elapsed().is_ok_and(|age| age >= STALE));
            if stale
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&*unfinished)
            {
                remove(&entry.path());
            }
        }
    }
    let Ok(current) = Version::parse(current) else {
        return;
    };
    for entry in fs::read_dir(updates).into_iter().flatten().flatten() {
        let passed = entry
            .file_name()
            .to_str()
            .and_then(|name| Version::parse(name).ok())
            .is_some_and(|version| version <= current);
        if passed {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// How old a file left by a download or an unpacking must be to be deleted.
const STALE: std::time::Duration = std::time::Duration::from_secs(3600);

/// `target`'s hidden sibling: `templum.AppImage` and `update` make `.templum.AppImage.update`.
fn beside(target: &Path, what: &str) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.{what}"))
}

fn remove(path: &Path) {
    let _ = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
}

fn executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|e| cant_write(path, &e))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// The portable archive's `templum` (beside its LICENSE, in `templum-<target>/`), written to `to`.
fn unpack(archive: &Path, to: &Path) -> Result<(), String> {
    let broken = |e: io::Error| format!("The archive {} is broken: {e}", archive.display());
    let file = File::open(archive).map_err(broken)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(broken)? {
        let mut entry = entry.map_err(broken)?;
        let path = entry.path().map_err(broken)?;
        if entry.header().entry_type().is_file()
            && path.file_name().is_some_and(|name| name == "templum")
        {
            let part = own(to);
            let mut out = File::create(&part).map_err(|e| cant_write(&part, &e))?;
            io::copy(&mut entry, &mut out).map_err(|e| cant_write(&part, &e))?;
            executable(&part)?;
            return fs::rename(&part, to).map_err(|e| cant_write(to, &e));
        }
    }
    Err(format!("The archive {} has no templum", archive.display()))
}

/// The disk image's app, copied to `new` (a new bundle: code is never written over, macOS would
/// kill the running app).
fn copy_app(dmg: &Path, mount: &Path, new: &Path) -> Result<(), String> {
    let copy = own(new);
    remove(&copy);
    fs::create_dir_all(mount).map_err(|e| cant_write(mount, &e))?;
    busy_again(Duration::from_millis(500), || {
        run(Command::new("hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(mount)
            .arg(dmg))
    })?;
    let app = fs::read_dir(mount)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "app"));
    let copied = match app {
        Some(app) => run(Command::new("ditto").arg(app).arg(&copy)),
        None => Err(format!("{} has no app", dmg.display())),
    };
    let _ = run(Command::new("hdiutil").arg("detach").arg(mount));
    copied?;
    remove(new);
    fs::rename(&copy, new).map_err(|e| cant_write(new, &e))
}

/// `f`, done again (up to 5 times, `wait` apart) while it fails for a resource that is busy:
/// `hdiutil attach` says "Resource temporarily unavailable" while macOS's disk images are busy.
fn busy_again(wait: Duration, mut f: impl FnMut() -> Result<(), String>) -> Result<(), String> {
    for _ in 1..5 {
        match f() {
            Err(e) if e.contains("Resource temporarily unavailable") => std::thread::sleep(wait),
            done => return done,
        }
    }
    f()
}

fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let out = command
        .output()
        .map_err(|e| format!("Couldn't run {program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program} failed: {}", error.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Asset;
    use crate::fetch::tests::{asset, serve};

    fn release(assets: Vec<Asset>) -> Release {
        Release {
            version: Version::new(0, 2, 0),
            page: String::new(),
            assets,
        }
    }

    /// One file served once, as release 0.2.0's `name`.
    fn one(name: &str, body: &'static [u8]) -> Release {
        release(vec![asset(name, format!("{}/file", serve(body, 1)), body)])
    }

    fn prepared(install: &Install, release: &Release, os: &str, updates: &Path) -> Ready {
        let mut heard = Vec::new();
        let ready = prepare_on(
            &crate::agent(),
            install,
            release,
            updates,
            os,
            "x86_64",
            &mut |got, total| heard.push((got, total)),
        )
        .unwrap();
        let total = release.assets.iter().map(|a| a.size).sum();
        assert_eq!(heard.last(), Some(&(total, total)));
        ready
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn an_appimage_is_written_beside_and_swapped() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Templum.AppImage");
        fs::write(&target, "0.1.1").unwrap();
        let install = Install::AppImage(target.clone());
        let release = one("templum_0.2.0_x86_64.AppImage", b"0.2.0");
        let ready = prepared(&install, &release, "linux", &dir.path().join("updates"));
        let new = dir.path().join(".Templum.AppImage.update");
        assert_eq!(
            ready,
            Ready::Swap {
                new: new.clone(),
                target: target.clone(),
                restart: target.clone()
            }
        );
        assert_eq!(fs::read(&target).unwrap(), b"0.1.1", "untouched until quit");
        #[cfg(unix)]
        assert_eq!(mode(&new), 0o755);
        swap(&new, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"0.2.0");
        assert!(!new.exists());
    }

    fn portable_archive() -> &'static [u8] {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut tar = tar::Builder::new(&mut gz);
            for (name, body) in [
                ("templum-x86_64-unknown-linux-gnu/LICENSE", &b"MIT"[..]),
                ("templum-x86_64-unknown-linux-gnu/templum", b"\x7fELF 0.2.0"),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, name, body).unwrap();
            }
            tar.finish().unwrap();
        }
        Box::leak(gz.finish().unwrap().into_boxed_slice())
    }

    #[test]
    fn a_portable_linux_build_takes_the_archives_templum() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("templum");
        let updates = dir.path().join("updates");
        let release = one(
            "templum-x86_64-unknown-linux-gnu.tar.gz",
            portable_archive(),
        );
        let ready = prepared(
            &Install::Portable(target.clone()),
            &release,
            "linux",
            &updates,
        );
        let new = dir.path().join(".templum.update");
        assert!(matches!(&ready, Ready::Swap { new: n, .. } if *n == new));
        assert_eq!(fs::read(&new).unwrap(), b"\x7fELF 0.2.0");
        #[cfg(unix)]
        assert_eq!(mode(&new), 0o755);
        assert!(
            updates
                .join("0.2.0/templum-x86_64-unknown-linux-gnu.tar.gz")
                .is_file()
        );
    }

    #[test]
    fn a_portable_windows_build_is_written_beside() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("templum.exe");
        let release = one("templum-x86_64-pc-windows-msvc.exe", b"MZ 0.2.0");
        let ready = prepared(
            &Install::Portable(target.clone()),
            &release,
            "windows",
            &dir.path().join("updates"),
        );
        let new = dir.path().join(".templum.exe.update");
        assert_eq!(fs::read(&new).unwrap(), b"MZ 0.2.0");
        assert!(matches!(ready, Ready::Swap { new: n, .. } if n == new));
    }

    #[test]
    fn installers_run_after_quitting() {
        let dir = tempfile::tempdir().unwrap();
        let updates = dir.path().join("updates");
        let exe = dir.path().join("templum.exe");
        let release = one("templum_0.2.0_x64-setup.exe", b"setup");
        let setup = updates.join("0.2.0/templum_0.2.0_x64-setup.exe");
        assert_eq!(
            prepared(&Install::Nsis(exe.clone()), &release, "windows", &updates),
            Ready::Installer(Launch {
                program: setup.clone(),
                args: vec!["/P".into(), "/R".into(), "/NS".into()],
            })
        );
        assert_eq!(fs::read(&setup).unwrap(), b"setup");

        let release = one("templum_0.2.0_x64_en-US.msi", b"msi");
        let msi = updates.join("0.2.0").join("templum_0.2.0_x64_en-US.msi");
        assert_eq!(
            prepared(&Install::Msi(exe), &release, "windows", &updates),
            Ready::Installer(Launch {
                program: "msiexec".into(),
                args: vec!["/i".into(), msi.into(), "/passive".into()],
            })
        );
    }

    #[test]
    fn packages_are_installed_in_a_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let updates = dir.path().join("updates");
        let folder = updates.join("0.2.0");
        let exe = PathBuf::from("/usr/bin/templum");
        let deb = one("templum_0.2.0_amd64.deb", b"deb");
        assert_eq!(
            prepared(&Install::Deb(exe.clone()), &deb, "linux", &updates),
            Ready::Terminal {
                folder: folder.clone(),
                command: "sudo apt install ./templum_0.2.0_amd64.deb".into(),
                restart: exe.clone(),
            }
        );

        let address = serve(b"pacman", 2);
        let pacman = release(vec![
            asset("PKGBUILD", format!("{address}/file"), b"pacman"),
            asset(
                "templum_0.2.0_x86_64.tar.gz",
                format!("{address}/file"),
                b"pacman",
            ),
        ]);
        assert_eq!(
            prepared(&Install::Pacman(exe.clone()), &pacman, "linux", &updates),
            Ready::Terminal {
                folder: folder.clone(),
                command: "makepkg -si".into(),
                restart: exe,
            }
        );
        assert!(folder.join("PKGBUILD").is_file());
        assert!(folder.join("templum_0.2.0_x86_64.tar.gz").is_file());
    }

    #[test]
    fn a_bundle_is_swapped_and_the_old_one_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Templum.app");
        let new = beside(&bundle, "update");
        for (app, version) in [(&bundle, "0.1.1"), (&new, "0.2.0")] {
            fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            fs::write(app.join("Contents/MacOS/templum"), version).unwrap();
        }
        swap(&new, &bundle).unwrap();
        assert_eq!(
            fs::read(bundle.join("Contents/MacOS/templum")).unwrap(),
            b"0.2.0"
        );
        assert!(!new.exists());
        assert!(!beside(&bundle, "old").exists());
    }

    #[test]
    fn cleanup_keeps_only_newer_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("templum.exe");
        fs::write(&target, "").unwrap();
        let long_ago = std::time::SystemTime::now() - STALE;
        for what in [
            "old",
            "update",
            "update.4120.part",
            "update.4121",
            "update.4122.part",
        ] {
            let file = File::create(beside(&target, what)).unwrap();
            if what != "update.4122.part" {
                file.set_modified(long_ago).unwrap();
            }
        }
        let updates = dir.path().join("updates");
        for folder in ["0.1.0", "0.1.1", "0.2.0", "notes"] {
            fs::create_dir_all(updates.join(folder)).unwrap();
        }
        cleanup(&Install::Portable(target.clone()), &updates, "0.1.1");
        let mut left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                ".templum.exe.update",
                ".templum.exe.update.4122.part",
                "templum.exe",
                "updates"
            ],
            "a ready download and another Templum's stay"
        );
        let mut kept: Vec<_> = fs::read_dir(&updates)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, ["0.2.0", "notes"]);
    }

    // macOS CI's `hdiutil attach` once answered EAGAIN; any other failure stays one.
    #[test]
    fn a_busy_resource_is_tried_again_and_nothing_else_is() {
        let mut calls = 0;
        let busy = "hdiutil failed: hdiutil: attach failed - Resource temporarily unavailable";
        let done = busy_again(Duration::ZERO, || {
            calls += 1;
            if calls < 3 { Err(busy.into()) } else { Ok(()) }
        });
        assert_eq!((done, calls), (Ok(()), 3));
        let mut calls = 0;
        let done = busy_again(Duration::ZERO, || {
            calls += 1;
            Err(busy.into())
        });
        assert_eq!((done, calls), (Err(busy.to_string()), 5));
        let mut calls = 0;
        let done = busy_again(Duration::ZERO, || {
            calls += 1;
            Err("hdiutil failed: no such file".into())
        });
        assert_eq!(calls, 1, "{done:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_disk_images_app_is_copied_beside_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(source.join("Templum.app/Contents/MacOS")).unwrap();
        fs::write(source.join("Templum.app/Contents/MacOS/templum"), "0.2.0").unwrap();
        let dmg = dir.path().join("Templum.dmg");
        run(Command::new("hdiutil")
            .args([
                "create",
                "-volname",
                "Templum",
                "-format",
                "UDZO",
                "-srcfolder",
            ])
            .arg(&source)
            .arg(&dmg))
        .unwrap();
        let new = dir.path().join(".Templum.app.update");
        copy_app(&dmg, &dir.path().join("mount"), &new).unwrap();
        assert_eq!(
            fs::read(new.join("Contents/MacOS/templum")).unwrap(),
            b"0.2.0"
        );
    }
}
