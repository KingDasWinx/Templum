//! Where a mirror's key is kept (docs/specs/templum-to-templum.md, Where the key is kept): the
//! system's keyring, or a file only the person reads when there is no keyring and they ask for
//! it, or memory. Every call here may wait on the system (an unlocked keyring may prompt): the
//! client calls them on its blocking pool, never on the UI thread.

use crate::code::{Key, Trust};
use crate::store::{private_write, random_text};
use keyring_core::CredentialStore;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// The keyring service Templum's entries live under.
pub const SERVICE: &str = "Templum";
/// The keyring service of `--dev`.
pub const SERVICE_DEV: &str = "Templum (dev)";

const KEYRING_WORDS: &str = "Templum couldn't read this connection's key from the keyring.";
const FILE_WORDS: &str = "Templum couldn't read this connection's key from its file.";

/// Everything secret about one connection, and where it may be sent. `Debug` shows none of it.
///
/// The index that holds the address, the way in and the proxy can be written by anything that
/// writes the data folder; the keyring cannot. So the keyring repeats them, and a remote whose
/// own values differ never sends the key (`Remote::binds`).
#[derive(Clone, PartialEq, Eq)]
pub struct Secrets {
    pub token: Key,
    pub access_secret: Option<String>,
    /// `user:password` for the proxy.
    pub proxy_auth: Option<String>,
    /// The code's address, the way in (with the pin), whether an added authority was accepted
    /// and the proxy: the places the secrets above may go, as they were when the person chose.
    pub url: String,
    pub trust: Trust,
    pub trust_added: bool,
    pub proxy: Option<String>,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secrets { token: tpl_…, .. }")
    }
}

/// `Secrets` as it is kept: one JSON value.
#[derive(Serialize, Deserialize)]
struct Wire {
    token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_auth: Option<String>,
    url: String,
    /// `loopback`, `public` or `pin:` and 64 hex digits.
    trust: String,
    #[serde(default)]
    trust_added: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy: Option<String>,
}

fn trust_text(trust: &Trust) -> String {
    match trust {
        Trust::Loopback => "loopback".into(),
        Trust::Public => "public".into(),
        Trust::Pinned(pin) => format!("pin:{pin}"),
    }
}

fn trust_of(text: &str) -> Option<Trust> {
    match text {
        "loopback" => Some(Trust::Loopback),
        "public" => Some(Trust::Public),
        _ => text
            .strip_prefix("pin:")
            .map(|pin| Trust::Pinned(pin.into())),
    }
}

impl Secrets {
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(&Wire {
            token: self.token.expose().into(),
            access_secret: self.access_secret.clone(),
            proxy_auth: self.proxy_auth.clone(),
            url: self.url.clone(),
            trust: trust_text(&self.trust),
            trust_added: self.trust_added,
            proxy: self.proxy.clone(),
        })
        .expect("strings serialize")
    }

    pub(crate) fn from_json(text: &str) -> Option<Secrets> {
        let kept: Wire = serde_json::from_str(text).ok()?;
        let trust = trust_of(&kept.trust)?;
        kept.token
            .starts_with(crate::store::TOKEN_PREFIX)
            .then(|| Secrets {
                token: Key::new(kept.token),
                access_secret: kept.access_secret,
                proxy_auth: kept.proxy_auth,
                url: kept.url,
                trust,
                trust_added: kept.trust_added,
                proxy: kept.proxy,
            })
    }
}

/// Where `save` put the secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    Keyring,
    File,
    /// Nowhere: the caller holds them until Templum quits.
    Memory,
}

/// The key store of one Templum. Cheap to clone.
#[derive(Clone)]
pub struct Keys(Arc<Inner>);

struct Inner {
    service: String,
    /// The data folder, for the files.
    data: Option<PathBuf>,
    /// The keyring, made at its first use; None: there is none.
    store: OnceLock<Option<Arc<CredentialStore>>>,
    system: bool,
    /// The entries deleted this run, held while any is saved or deleted: a save still running
    /// when its entry is deleted (its connection was replaced) must not write it after. (One
    /// lock for all keys; a keyring that waits for the person holds the others' saves too.)
    gone: Mutex<HashSet<String>>,
}

impl Keys {
    /// The system's keyring, opened at its first use. `data` is the data folder; None: no file
    /// can be kept.
    pub fn system(service: &str, data: Option<&Path>) -> Keys {
        Keys::make(service, data, OnceLock::new(), true)
    }

    /// A given keyring (the tests': `keyring_core::mock::Store`).
    pub fn with_store(store: Arc<CredentialStore>, service: &str, data: Option<&Path>) -> Keys {
        Keys::make(service, data, OnceLock::from(Some(store)), false)
    }

    pub fn without_keyring(data: Option<&Path>) -> Keys {
        Keys::make(SERVICE, data, OnceLock::from(None), false)
    }

    fn make(
        service: &str,
        data: Option<&Path>,
        store: OnceLock<Option<Arc<CredentialStore>>>,
        system: bool,
    ) -> Keys {
        Keys(Arc::new(Inner {
            service: service.into(),
            data: data.map(Path::to_path_buf),
            store,
            system,
            gone: Mutex::default(),
        }))
    }

    fn store(&self) -> Option<&Arc<CredentialStore>> {
        self.0
            .store
            .get_or_init(|| if self.0.system { system_store() } else { None })
            .as_ref()
    }

    /// Whether a keyring is there and answers: reading an entry that doesn't exist says so.
    pub fn answers(&self) -> bool {
        let Some(store) = self.store() else {
            return false;
        };
        let probe = random_text("probe-", 6);
        matches!(
            store
                .build(&self.0.service, &probe, None)
                .and_then(|entry| entry.get_password()),
            Err(keyring_core::Error::NoEntry)
        )
    }

    /// Keeps `secrets` under `entry`: in the file when `file`, else in the keyring; with no
    /// keyring, or one that refuses, they are `Kept::Memory` (not an error: the connection
    /// lasts until Templum quits). An error only for a file that couldn't be written.
    pub fn save(&self, entry: &str, secrets: &Secrets, file: bool) -> Result<Kept, String> {
        let gone = self.0.gone.lock().unwrap_or_else(|e| e.into_inner());
        if gone.contains(entry) {
            return Err("This connection was replaced or forgotten.".into());
        }
        let json = secrets.to_json();
        if file {
            let path = self
                .path(entry)
                .ok_or("Templum has no folder to keep the key in.")?;
            return private_write(&path, json.as_bytes())
                .map(|()| Kept::File)
                .map_err(|e| {
                    format!(
                        "Templum couldn't write the key's file in {}: {e}",
                        dir_of(&path)
                    )
                });
        }
        let kept = self.store().is_some_and(|store| {
            store
                .build(&self.0.service, entry, None)
                .and_then(|e| e.set_password(&json))
                .is_ok()
        });
        Ok(if kept { Kept::Keyring } else { Kept::Memory })
    }

    pub fn load(&self, entry: &str, file: bool) -> Result<Secrets, String> {
        let text = if file {
            self.path(entry)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .ok_or(FILE_WORDS)?
        } else {
            self.store()
                .and_then(|store| store.build(&self.0.service, entry, None).ok())
                .and_then(|e| e.get_password().ok())
                .ok_or(KEYRING_WORDS)?
        };
        Secrets::from_json(&text)
            .ok_or_else(|| if file { FILE_WORDS } else { KEYRING_WORDS }.into())
    }

    /// Removes `entry` from both places; what is not there, or refuses, is ignored.
    pub fn delete(&self, entry: &str, _file: bool) {
        self.0
            .gone
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry.to_string());
        if let Some(path) = self.path(entry) {
            let _ = std::fs::remove_file(path);
        }
        if let Some(store) = self.store() {
            let _ = store
                .build(&self.0.service, entry, None)
                .and_then(|e| e.delete_credential());
        }
    }

    /// `<data>/remotes/<entry>.key`, for an entry name that can't leave the folder.
    fn path(&self, entry: &str) -> Option<PathBuf> {
        let plain = !entry.is_empty()
            && entry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        let data = self.0.data.as_ref().filter(|_| plain)?;
        Some(data.join("remotes").join(format!("{entry}.key")))
    }
}

fn dir_of(path: &Path) -> String {
    path.parent().unwrap_or(path).display().to_string()
}

/// `remote-` and 8 random characters `[a-z0-9]`: a connection's name in the keyring.
pub fn entry_name() -> String {
    let id: String = random_text("", 32)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(8)
        .collect();
    format!("remote-{id}")
}

#[cfg(target_os = "macos")]
fn system_store() -> Option<Arc<CredentialStore>> {
    let store = apple_native_keyring_store::keychain::Store::new().ok()?;
    Some(store)
}

#[cfg(windows)]
fn system_store() -> Option<Arc<CredentialStore>> {
    let store = windows_native_keyring_store::Store::new().ok()?;
    Some(store)
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn system_store() -> Option<Arc<CredentialStore>> {
    let store = zbus_secret_service_keyring_store::Store::new().ok()?;
    Some(store)
}

#[cfg(not(any(
    target_os = "macos",
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn system_store() -> Option<Arc<CredentialStore>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring_core::api::CredentialStoreApi;
    use keyring_core::mock;

    fn secrets() -> Secrets {
        Secrets {
            token: Key::new("tpl_SECRET".into()),
            access_secret: Some("acc".into()),
            proxy_auth: None,
            url: "https://templum.example.com".into(),
            trust: Trust::Public,
            trust_added: false,
            proxy: None,
        }
    }

    fn mock_in(dir: &Path) -> (Keys, Arc<mock::Store>) {
        let store = mock::Store::new().unwrap();
        (
            Keys::with_store(store.clone(), SERVICE_DEV, Some(dir)),
            store,
        )
    }

    #[test]
    fn a_save_that_ran_late_does_not_bring_back_a_deleted_key() {
        let dir = tempfile::tempdir().unwrap();
        let (keys, _) = mock_in(dir.path());
        // A connection replaced while its save was still waiting for the keyring.
        keys.delete("remote-ab12cd34", false);
        assert!(keys.save("remote-ab12cd34", &secrets(), false).is_err());
        assert!(keys.save("remote-ab12cd34", &secrets(), true).is_err());
        assert!(keys.load("remote-ab12cd34", false).is_err());
        // Another entry is not affected.
        assert_eq!(keys.save("remote-ee", &secrets(), false), Ok(Kept::Keyring));
    }

    #[test]
    fn a_key_round_trips_through_the_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let (keys, _) = mock_in(dir.path());
        assert!(keys.answers());
        let s = secrets();
        assert_eq!(keys.save("remote-ab12cd34", &s, false), Ok(Kept::Keyring));
        assert_eq!(keys.load("remote-ab12cd34", false), Ok(s));
        // Where the key may go is kept with it, whatever the way in.
        for trust in [Trust::Loopback, Trust::Pinned("ab".repeat(32))] {
            let bound = Secrets {
                url: "http://127.0.0.1:7411".into(),
                trust,
                trust_added: true,
                proxy: Some("http://proxy.corp:8080".into()),
                proxy_auth: Some("u:p".into()),
                ..secrets()
            };
            assert_eq!(
                keys.save("remote-ab12cd34", &bound, false),
                Ok(Kept::Keyring)
            );
            assert_eq!(keys.load("remote-ab12cd34", false), Ok(bound));
        }
        keys.delete("remote-ab12cd34", false);
        assert_eq!(
            keys.load("remote-ab12cd34", false),
            Err("Templum couldn't read this connection's key from the keyring.".into())
        );
        assert!(
            !dir.path().join("remotes").exists(),
            "nothing went to a file"
        );
    }

    #[test]
    fn without_a_keyring_the_key_stays_in_memory_unless_a_file_is_asked() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::without_keyring(Some(dir.path()));
        let s = secrets();
        assert!(!keys.answers());
        assert_eq!(keys.save("remote-ab12cd34", &s, false), Ok(Kept::Memory));
        assert!(!dir.path().join("remotes").exists());

        assert_eq!(keys.save("remote-ab12cd34", &s, true), Ok(Kept::File));
        let file = dir.path().join("remotes/remote-ab12cd34.key");
        assert!(file.exists());
        assert_eq!(keys.load("remote-ab12cd34", true), Ok(s));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!((mode(&file), mode(file.parent().unwrap())), (0o600, 0o700));
        }
        keys.delete("remote-ab12cd34", true);
        assert!(!file.exists());
        assert_eq!(
            keys.load("remote-ab12cd34", true),
            Err("Templum couldn't read this connection's key from its file.".into())
        );
    }

    #[test]
    fn a_name_that_leaves_the_folder_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keys::without_keyring(Some(dir.path()));
        assert!(keys.save("../escape", &secrets(), true).is_err());
        assert!(keys.load("../escape", true).is_err());
        assert!(
            Keys::without_keyring(None)
                .save("remote-x", &secrets(), true)
                .is_err()
        );
    }

    #[test]
    fn a_refusing_keyring_is_memory_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (keys, store) = mock_in(dir.path());
        let entry = store.build(SERVICE_DEV, "remote-ab12cd34", None).unwrap();
        let cred: &mock::Cred = entry.as_any().downcast_ref().unwrap();
        cred.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
        assert_eq!(
            keys.save("remote-ab12cd34", &secrets(), false),
            Ok(Kept::Memory)
        );
        assert!(
            keys.load("remote-ab12cd34", false).is_err(),
            "it kept nothing"
        );
    }

    #[test]
    fn secrets_never_show() {
        let s = secrets();
        let shown = format!("{s:?} {:?}", s.token);
        assert!(
            !shown.contains("SECRET") && !shown.contains("acc"),
            "{shown}"
        );
        let name = entry_name();
        assert!(
            name.len() == 15
                && name.starts_with("remote-")
                && name[7..]
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
            "{name}"
        );
        assert_ne!(name, entry_name());
    }
}
