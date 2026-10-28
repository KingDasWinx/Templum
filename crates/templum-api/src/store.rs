//! The API's settings and tokens, and their file (docs/specs/api.md, Authentication and
//! Persistence): `<data>/api/api.json`, written through a temporary file and a rename, private to
//! the person. A token's text is never kept; its SHA-256 is.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The port the API listens on unless the person chooses another: fixed, so a tunnel's config
/// stays right.
pub const PORT: u16 = 7411;
/// What every token's text starts with, so a secret scanner can find a leaked one.
pub const TOKEN_PREFIX: &str = "tpl_";
/// The most characters in a token's name.
pub const NAME_MOST: usize = 60;
/// The newest version of the file this build reads.
const VERSION: u32 = 1;

/// What a token may do, each including the one before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Read,
    Control,
    Full,
}

impl Scope {
    /// The scope as a sentence names it: "send_to_terminal needs a Full token."
    pub fn word(self) -> &'static str {
        match self {
            Scope::Read => "Read",
            Scope::Control => "Control",
            Scope::Full => "Full",
        }
    }
}

/// Who may reach the server.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Listen {
    /// `127.0.0.1`: where a tunnel or a proxy on this computer reaches it.
    #[default]
    Loopback,
    /// `0.0.0.0`.
    All,
    /// One address of this computer.
    Address(String),
}

/// How the server speaks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tls {
    #[default]
    Off,
    /// Templum's own certificate.
    Own,
    /// The person's: a PEM chain and a PEM key.
    Files { cert: PathBuf, key: PathBuf },
}

/// What the person sets in Settings ▸ API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub listen: Listen,
    pub port: u16,
    pub tls: Tls,
    /// The web pages (whole origins) allowed to call the API. Empty: CORS is off.
    pub origins: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            listen: Listen::default(),
            port: PORT,
            tls: Tls::default(),
            origins: Vec::new(),
        }
    }
}

/// A token's newest use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Use {
    pub at: u64,
    pub addr: String,
}

/// How a share's other computer reaches this one (docs/specs/templum-to-templum.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    /// Through a tunnel or proxy with a public certificate.
    #[default]
    Public,
    /// Directly, pinning Templum's certificate.
    Pinned,
}

/// What makes a token a share of one workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub workspace: String,
    pub way: Way,
    /// As the person typed it: `https://templum.example.com` or `192.168.1.5:7411`.
    pub address: String,
}

/// A key the person made. It holds the hash of its text, never the text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub id: u64,
    pub name: String,
    pub hash: String,
    pub scope: Scope,
    /// The workspaces it may see; None: all of them.
    pub workspaces: Option<Vec<String>>,
    pub created: u64,
    pub expires: Option<u64>,
    pub last: Option<Use>,
    /// Set for a token made by sharing a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<Share>,
}

impl Token {
    /// Not expired at `now` (Unix seconds).
    pub fn live(&self, now: u64) -> bool {
        self.expires.is_none_or(|end| now < end)
    }

    /// Whether this is a share that reaches the workspace `space` now. The token's own list is the
    /// truth: `share.workspace` is the workspace it was made for, and an id can be made again.
    pub fn shares(&self, space: &str) -> bool {
        self.share.is_some()
            && self
                .workspaces
                .as_ref()
                .is_some_and(|list| list.iter().any(|w| w == space))
    }

    /// A share's id, `s<token id>`; None for a token Settings made.
    pub fn share_id(&self) -> Option<String> {
        self.share.as_ref().map(|_| format!("s{}", self.id))
    }
}

/// The file's content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub settings: Settings,
    pub next_id: u64,
    pub tokens: Vec<Token>,
    /// The automations a token limited to some workspaces saved, by number: the folders (real
    /// paths) of its workspaces then. What they open must still lie inside them when they run.
    pub confined: std::collections::BTreeMap<u64, Vec<PathBuf>>,
    /// The token (its id) that saved each automation made through the API, by number: when the
    /// token is revoked or expires, its automations are turned off.
    pub saved_by: std::collections::BTreeMap<u64, u64>,
    /// The last tunnel address a share used: the Share… dialog's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            version: VERSION,
            settings: Settings::default(),
            next_id: 1,
            tokens: Vec::new(),
            confined: Default::default(),
            saved_by: Default::default(),
            tunnel: None,
        }
    }
}

impl Store {
    /// Forgets `confined` and `saved_by` for the automations `exists` doesn't know: the numbers
    /// start again if `automations.json` was set aside, and a stale entry would be put on a new
    /// rule. Whether anything went.
    pub fn forget_automations(&mut self, exists: impl Fn(u64) -> bool) -> bool {
        let before = self.confined.len() + self.saved_by.len();
        self.confined.retain(|id, _| exists(*id));
        self.saved_by.retain(|id, _| exists(*id));
        self.confined.len() + self.saved_by.len() != before
    }

    /// Makes a token and says its text, which exists nowhere else: the store keeps its hash.
    pub fn mint(
        &mut self,
        name: &str,
        scope: Scope,
        expires: Option<u64>,
        workspaces: Option<Vec<String>>,
        now: u64,
    ) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A token needs a name.".into());
        }
        if name.chars().count() > NAME_MOST {
            return Err(format!("A token's name is at most {NAME_MOST} characters."));
        }
        if expires.is_some_and(|at| at <= now) {
            return Err("The expiry is already past: choose a later time.".into());
        }
        let text = random_text(TOKEN_PREFIX, 32);
        self.tokens.push(Token {
            id: self.next_id,
            name: name.to_string(),
            hash: hash(&text),
            scope,
            workspaces,
            created: now,
            expires,
            last: None,
            share: None,
        });
        self.next_id += 1;
        Ok(text)
    }

    /// Makes a token limited to `share.workspace`, kept with the share's details, and says its id
    /// and its text (which exists nowhere else). A public share's address is remembered as the
    /// next one's default.
    pub fn mint_share(
        &mut self,
        share: Share,
        name: &str,
        scope: Scope,
        expires: Option<u64>,
        now: u64,
    ) -> Result<(u64, String), String> {
        if !matches!(scope, Scope::Control | Scope::Full) {
            return Err("A share is Control or Full.".into());
        }
        let text = self.mint(
            name,
            scope,
            expires,
            Some(vec![share.workspace.clone()]),
            now,
        )?;
        if share.way == Way::Public {
            self.tunnel = Some(share.address.clone());
        }
        let token = self.tokens.last_mut().expect("the token just made");
        token.share = Some(share);
        Ok((token.id, text))
    }

    /// Takes a token away; false when there is none with that id.
    pub fn revoke(&mut self, id: u64) -> bool {
        let before = self.tokens.len();
        self.tokens.retain(|token| token.id != id);
        self.tokens.len() != before
    }

    pub fn revoke_all(&mut self) {
        self.tokens.clear();
    }
}

/// What keeps a share of this `way` and `address` from working when its code is pasted on the
/// other computer, given the API's `settings`; None when it fits.
pub fn share_problem(settings: &Settings, way: Way, address: &str) -> Option<&'static str> {
    match way {
        Way::Pinned if settings.tls != Tls::Own => {
            Some("A pinned share needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS.")
        }
        Way::Pinned if settings.listen == Listen::Loopback => {
            Some("Directly needs the API on every network with Templum's certificate.")
        }
        Way::Pinned => None,
        Way::Public => {
            let address = address.trim();
            let loopback = address.strip_prefix("http://").is_some_and(|rest| {
                ["127.0.0.1", "[::1]", "localhost"].iter().any(|host| {
                    rest.strip_prefix(host)
                        .is_some_and(|tail| tail.is_empty() || tail.starts_with([':', '/']))
                })
            });
            let authority = address.split("://").nth(1).unwrap_or_default();
            let authority = authority.split('/').next().unwrap_or_default();
            if !address.starts_with("https://") && !loopback {
                Some("The address must start with https://.")
            } else if authority.contains('@') || address.contains(['?', '#']) {
                // A login would read as the host and be another.
                Some("The address is a host alone: no login, query or fragment.")
            } else {
                None
            }
        }
    }
}

/// `prefix` and `bytes` random bytes from the system's generator, in base64url without padding.
pub fn random_text(prefix: &str, bytes: usize) -> String {
    let mut random = vec![0u8; bytes];
    // A system without a random source can't make a key worth having: stopping is the safe answer.
    SystemRandom::new()
        .fill(&mut random)
        .expect("the system's random source");
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(random))
}

/// The SHA-256 of `text`, in lowercase hex.
pub fn hash(text: &str) -> String {
    digest(&SHA256, text.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The live token whose text `presented` is. It hashes `presented` once and compares it with every
/// live token's hash, whatever it finds, so the time it takes says nothing about which matched.
pub fn verify<'a>(tokens: &'a [Token], presented: &str, now: u64) -> Option<&'a Token> {
    let wanted = hash(presented);
    let mut found = None;
    for token in tokens.iter().filter(|token| token.live(now)) {
        if same(token.hash.as_bytes(), wanted.as_bytes()) {
            found = Some(token);
        }
    }
    found
}

/// Whether two byte strings are equal, looking at every byte of equal-length ones. (A length is
/// not a secret: both are hashes.)
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a
        .iter()
        .zip(b)
        .fold(0u8, |acc, (x, y)| acc | std::hint::black_box(x ^ y));
    std::hint::black_box(diff) == 0
}

/// Where the API keeps its files: `<data>/api`.
pub fn folder(data: &Path) -> PathBuf {
    data.join("api")
}

/// Reads the file. No file: the defaults. One that can't be read or parsed, or is of a newer
/// version, is set aside as `api.json.bak` and the Err says so, so the API stays off.
pub fn load(data: &Path) -> Result<Store, String> {
    let path = folder(data).join("api.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(set_aside(&path, &error.to_string())),
    };
    match serde_json::from_slice::<Store>(&bytes) {
        Ok(store) if store.version <= VERSION => Ok(store),
        Ok(store) => Err(set_aside(
            &path,
            &format!(
                "it is version {}, newer than this Templum reads",
                store.version
            ),
        )),
        Err(error) => Err(set_aside(&path, &error.to_string())),
    }
}

/// Renames the file to `.bak` and says what was wrong.
fn set_aside(path: &Path, why: &str) -> String {
    let bak = path.with_extension("json.bak");
    match std::fs::rename(path, &bak) {
        Ok(()) => format!(
            "The API's settings file couldn't be used ({why}); it is kept as {}.",
            bak.display()
        ),
        Err(error) => {
            format!("The API's settings file couldn't be used ({why}) nor set aside: {error}.")
        }
    }
}

pub fn save(data: &Path, store: &Store) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(store).map_err(io::Error::other)?;
    private_write(&folder(data).join("api.json"), &json)
}

/// Makes `dir` if it is missing, `0700` on Unix: only its person opens it.
pub(crate) fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `bytes` to `path` for its person alone: the folder `0700` on Unix, a temporary file in
/// it `0600`, written, then renamed over `path`.
pub fn private_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    private_dir(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(".{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&temporary).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(error) = written.and_then(|()| std::fs::rename(&temporary, path)) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_token_is_shown_once_and_kept_as_its_hash() {
        let mut store = Store::default();
        let text = store
            .mint("phone", Scope::Control, Some(100), None, 10)
            .unwrap();
        assert!(text.starts_with("tpl_") && text.len() == 47);
        let token = &store.tokens[0];
        assert_eq!(
            (token.id, token.hash.clone(), token.created),
            (1, hash(&text), 10)
        );
        let saved = serde_json::to_string(&store).unwrap();
        assert!(
            !saved.contains(&text[4..]),
            "the file never holds the token"
        );
        assert_eq!(verify(&store.tokens, &text, 99).map(|t| t.id), Some(1));
        assert_ne!(store.mint("b", Scope::Read, None, None, 10).unwrap(), text);
    }

    #[test]
    fn a_wrong_expired_or_revoked_token_verifies_nothing() {
        let mut store = Store::default();
        let text = store.mint("a", Scope::Read, Some(100), None, 0).unwrap();
        let other = store.mint("b", Scope::Full, None, None, 0).unwrap();
        let mut changed = text.clone();
        changed.pop();
        changed.push(if text.ends_with('A') { 'B' } else { 'A' });
        for wrong in ["", "tpl_", &changed, &format!("Bearer {text}")] {
            assert!(verify(&store.tokens, wrong, 1).is_none(), "{}", wrong.len());
        }
        assert_eq!(verify(&store.tokens, &text, 99).map(|t| t.id), Some(1));
        assert!(
            verify(&store.tokens, &text, 100).is_none(),
            "expired at its expiry"
        );
        assert_eq!(verify(&store.tokens, &other, 100).map(|t| t.id), Some(2));
        assert!(store.revoke(1) && !store.revoke(1));
        assert!(verify(&store.tokens, &text, 1).is_none());
        store.revoke_all();
        assert!(store.tokens.is_empty() && verify(&store.tokens, &other, 1).is_none());
        // Revoking doesn't hand an id out again.
        assert_eq!(store.next_id, 3);
    }

    #[test]
    fn a_name_is_required_and_short() {
        let mut store = Store::default();
        assert_eq!(
            store.mint("  ", Scope::Read, None, None, 0),
            Err("A token needs a name.".into())
        );
        assert_eq!(
            store.mint(&"x".repeat(61), Scope::Read, None, None, 0),
            Err("A token's name is at most 60 characters.".into())
        );
        assert!(store.tokens.is_empty() && store.next_id == 1);
        store
            .mint(&"é".repeat(60), Scope::Read, None, None, 0)
            .unwrap();
        store.mint(" phone ", Scope::Read, None, None, 0).unwrap();
        assert_eq!(store.tokens[1].name, "phone");
    }

    #[test]
    fn a_share_is_kept_with_its_token() {
        let mut store = Store::default();
        let share = Share {
            workspace: "w3".into(),
            way: Way::Pinned,
            address: "192.168.1.5:7411".into(),
        };
        let (id, text) = store
            .mint_share(share.clone(), "Another computer", Scope::Full, Some(100), 1)
            .unwrap();
        let token = &store.tokens[0];
        assert_eq!(id, 1);
        assert_eq!(token.workspaces, Some(vec!["w3".to_string()]));
        assert_eq!(token.share, Some(share));
        assert_eq!(token.share_id().as_deref(), Some("s1"));
        assert!(text.starts_with("tpl_") && verify(&store.tokens, &text, 2).is_some());
        assert_eq!(store.tunnel, None, "a pinned address isn't a tunnel's");
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &store).unwrap();
        assert_eq!(load(dir.path()), Ok(store.clone()));
        // A file written before has neither a share nor a tunnel.
        std::fs::write(
            folder(dir.path()).join("api.json"),
            r#"{"tokens":[{"id":1,"name":"a","hash":"h","scope":"read","workspaces":null,
                "created":1,"expires":null,"last":null}]}"#,
        )
        .unwrap();
        let old = load(dir.path()).unwrap();
        assert_eq!(
            (old.tokens[0].share.clone(), old.tunnel.clone()),
            (None, None)
        );
        assert_eq!(old.tokens[0].share_id(), None);
        let mine = Share {
            workspace: "w3".into(),
            way: Way::Public,
            address: "https://t.example.com".into(),
        };
        assert_eq!(
            store.mint_share(mine.clone(), "x", Scope::Read, None, 1),
            Err("A share is Control or Full.".into())
        );
        store
            .mint_share(mine, "x", Scope::Control, None, 1)
            .unwrap();
        assert_eq!(store.tunnel.as_deref(), Some("https://t.example.com"));
    }

    #[test]
    fn a_share_must_fit_the_api() {
        let settings = |listen, tls| Settings {
            listen,
            tls,
            ..Settings::default()
        };
        let https =
            "A pinned share needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS.";
        let directly = "Directly needs the API on every network with Templum's certificate.";
        assert_eq!(
            share_problem(&settings(Listen::Loopback, Tls::Off), Way::Pinned, "x"),
            Some(https)
        );
        assert_eq!(
            share_problem(&settings(Listen::All, Tls::Off), Way::Pinned, "x"),
            Some(https)
        );
        assert_eq!(
            share_problem(&settings(Listen::Loopback, Tls::Own), Way::Pinned, "x"),
            Some(directly)
        );
        assert_eq!(
            share_problem(&settings(Listen::All, Tls::Own), Way::Pinned, "x"),
            None
        );
        let public = |address| share_problem(&Settings::default(), Way::Public, address);
        for fits in [
            "https://t.example.com/p",
            "http://127.0.0.1:9000",
            "http://localhost",
            "http://[::1]:1",
            "  https://t.example.com ",
        ] {
            assert_eq!(public(fits), None, "{fits}");
        }
        for bad in [
            "http://10.0.0.2:7411",
            "templum.example.com",
            "http://localhost.evil.com",
            "",
        ] {
            assert_eq!(
                public(bad),
                Some("The address must start with https://."),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_share_address_is_a_host_alone() {
        let public = |address| share_problem(&Settings::default(), Way::Public, address);
        for bad in [
            "https://templum.example.com@evil.com",
            "https://user:pw@t.example.com",
            "https://t.example.com/p?x=1",
            "https://t.example.com/#top",
        ] {
            assert_eq!(
                public(bad),
                Some("The address is a host alone: no login, query or fragment."),
                "{bad}"
            );
        }
        // An @ further down the path is no login.
        assert_eq!(public("https://t.example.com/@me"), None);
    }

    #[test]
    fn a_token_cannot_expire_in_the_past() {
        let mut store = Store::default();
        for at in [0, 99, 100] {
            assert_eq!(
                store.mint("x", Scope::Read, Some(at), None, 100),
                Err("The expiry is already past: choose a later time.".into()),
                "{at}"
            );
        }
        assert!(store.tokens.is_empty());
        assert!(store.mint("x", Scope::Read, Some(101), None, 100).is_ok());
        assert!(store.mint("x", Scope::Read, None, None, 100).is_ok());
    }

    #[test]
    fn same_compares_every_byte() {
        assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab"));
        assert!(!same(b"xbc", b"abc") && same(b"", b""));
    }

    #[test]
    fn random_text_is_base64url_of_that_many_bytes() {
        let text = random_text("tkt_", 24);
        assert!(text.starts_with("tkt_") && text.len() == 36);
        assert!(
            text[4..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(text, random_text("tkt_", 24));
    }

    #[test]
    fn the_file_round_trips_and_defaults_fill_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Ok(Store::default()));
        let mut store = Store::default();
        store.settings.enabled = true;
        store.settings.origins = vec!["https://dash.example".into()];
        store
            .mint("phone", Scope::Full, Some(5), Some(vec!["w3".into()]), 1)
            .unwrap();
        store.tokens[0].last = Some(Use {
            at: 4,
            addr: "1.2.3.4".into(),
        });
        save(dir.path(), &store).unwrap();
        assert_eq!(load(dir.path()), Ok(store));

        let path = folder(dir.path()).join("api.json");
        std::fs::write(&path, r#"{ "version": 1 }"#).unwrap();
        let bare = load(dir.path()).unwrap();
        assert_eq!(bare, Store::default());
        assert_eq!(
            (bare.settings.enabled, bare.settings.port, bare.next_id),
            (false, 7411, 1)
        );
        std::fs::write(
            &path,
            r#"{"settings":{"listen":{"address":"10.0.0.2"},
                "tls":{"files":{"cert":"c","key":"k"}}}}"#,
        )
        .unwrap();
        let settings = load(dir.path()).unwrap().settings;
        assert_eq!(settings.listen, Listen::Address("10.0.0.2".into()));
        assert_eq!(
            settings.tls,
            Tls::Files {
                cert: "c".into(),
                key: "k".into()
            }
        );
        assert_eq!(settings.port, 7411);
        std::fs::write(&path, r#"{"settings":{"listen":"all"}}"#).unwrap();
        assert_eq!(load(dir.path()).unwrap().settings.listen, Listen::All);
    }

    #[test]
    fn a_bad_or_newer_file_is_set_aside() {
        for bytes in ["not json", r#"{ "version": 2 }"#] {
            let dir = tempfile::tempdir().unwrap();
            let path = folder(dir.path()).join("api.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            let error = load(dir.path()).unwrap_err();
            assert!(error.contains("api.json.bak"), "{error}");
            assert_eq!(
                std::fs::read(path.with_extension("json.bak")).unwrap(),
                bytes.as_bytes()
            );
            assert!(!path.exists());
            assert_eq!(
                load(dir.path()),
                Ok(Store::default()),
                "set aside, then fresh"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_file_and_its_folder_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &Store::default()).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder(dir.path())), 0o700);
        assert_eq!(mode(&folder(dir.path()).join("api.json")), 0o600);
        let leftovers: Vec<_> = std::fs::read_dir(folder(dir.path())).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "the temporary file is gone");
    }
}
