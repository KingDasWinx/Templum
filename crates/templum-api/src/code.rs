//! The connection code (docs/specs/templum-to-templum.md, The connection code): one line that
//! holds where a shared workspace is, the share's token and, for a direct share, the pin of
//! Templum's certificate. Home encodes it once, when the person shares; the mirror decodes it.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hyper::Uri;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::store::TOKEN_PREFIX;

/// What every code starts with; the `1` is the format's version.
pub const PREFIX: &str = "tplc1_";

/// A token's text. It shows nowhere: `Debug` and `Display` print `tpl_…`, and it is not
/// `Serialize`; `expose` is the one way to the text, for a header, the keyring or a code.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

impl Key {
    pub fn new(text: String) -> Key {
        Key(text)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tpl_…")
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tpl_…")
    }
}

/// How the mirror checks who answers, read from the code and never guessed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Templum's certificate, by its SHA-256 (64 lowercase hex characters).
    Pinned(String),
    /// A public certificate, checked against the web's authorities.
    Public,
    /// Plain HTTP to this computer itself (an `ssh -L` forward, a test).
    Loopback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    /// Without a trailing `/`: the mirror calls `<url>/v1/…`.
    pub url: String,
    pub key: Key,
    pub trust: Trust,
}

/// Why a text isn't a code the mirror can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrong {
    NotACode,
    CutShort,
    Newer,
    Plain,
}

impl fmt::Display for Wrong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Wrong::NotACode => "This isn't a connection code: one starts with tplc1_.",
            Wrong::CutShort => "The code is cut short: copy it again whole.",
            Wrong::Newer => "This code is from a newer Templum: update this one.",
            Wrong::Plain => {
                "This code's address isn't encrypted: Templum connects in plain HTTP only to this \
                 computer itself."
            }
        })
    }
}

impl std::error::Error for Wrong {}

#[derive(Serialize)]
struct Out<'a> {
    url: &'a str,
    token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pin: Option<&'a str>,
}

/// Unknown fields are ignored; a missing one reads as empty and is refused below.
#[derive(Deserialize)]
struct In {
    #[serde(default)]
    url: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    pin: Option<String>,
}

/// The code for `url` and `key`; `pin` is the certificate's SHA-256 of a pinned share.
pub fn encode(url: &str, key: &Key, pin: Option<&str>) -> String {
    let json = serde_json::to_vec(&Out {
        url,
        token: key.expose(),
        pin,
    })
    .expect("strings serialize");
    format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
}

pub fn decode(text: &str) -> Result<Code, Wrong> {
    let text = text.trim();
    let version = text
        .strip_prefix("tplc")
        .and_then(|rest| rest.split_once('_'))
        .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let body = match version {
        Some(("1", body)) => body,
        // A number too long for u32 is a newer format all the same.
        Some((n, _)) if n.parse::<u32>().is_ok_and(|n| n <= 1) => return Err(Wrong::NotACode),
        Some(_) => return Err(Wrong::Newer),
        None => return Err(Wrong::NotACode),
    };
    let json = URL_SAFE_NO_PAD.decode(body).map_err(|_| Wrong::CutShort)?;
    let wire: In = serde_json::from_slice(&json).map_err(|_| Wrong::CutShort)?;
    if !wire.token.starts_with(TOKEN_PREFIX) {
        return Err(Wrong::CutShort);
    }
    let url = wire.url.trim_end_matches('/').to_string();
    // A login (`https://templum.example.com@evil.com`), a query or a fragment is no address of
    // Templum's: the host would read as one and be another.
    if url.contains(['?', '#']) {
        return Err(Wrong::CutShort);
    }
    let uri: Uri = url.parse().map_err(|_| Wrong::CutShort)?;
    if uri.authority().is_some_and(|a| a.as_str().contains('@')) {
        return Err(Wrong::CutShort);
    }
    let (scheme, host) = match (uri.scheme_str(), uri.host()) {
        (Some(scheme), Some(host)) if !host.is_empty() => (scheme.to_ascii_lowercase(), host),
        _ => return Err(Wrong::CutShort),
    };
    let trust = match (wire.pin, scheme.as_str()) {
        (Some(pin), "https") if pin.len() == 64 && pin.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Trust::Pinned(pin.to_ascii_lowercase())
        }
        (Some(_), _) => return Err(Wrong::CutShort),
        (None, "https") => Trust::Public,
        (None, "http") if matches!(host, "127.0.0.1" | "[::1]" | "::1" | "localhost") => {
            Trust::Loopback
        }
        (None, "http") => return Err(Wrong::Plain),
        (None, _) => return Err(Wrong::CutShort),
    };
    Ok(Code {
        url,
        key: Key(wire.token),
        trust,
    })
}

impl Code {
    /// The host and port as the url has them: `templum.example.com`, `192.168.1.5:7411`.
    pub fn host(&self) -> String {
        match self.url.parse::<Uri>() {
            Ok(uri) => uri
                .authority()
                .map_or(&self.url[..], |a| a.as_str())
                .to_string(),
            Err(_) => self.url.clone(),
        }
    }

    /// What the Connect dialog says of the code before the person connects.
    pub fn said(&self) -> String {
        let host = self.host();
        match &self.trust {
            Trust::Public => format!("{host} · checks its public certificate"),
            Trust::Pinned(pin) => {
                let pair = |i: usize| pin[i * 2..i * 2 + 2].to_ascii_uppercase();
                format!(
                    "{host} · pins Templum's certificate {}:{}:…:{}",
                    pair(0),
                    pair(1),
                    pair(31)
                )
            }
            Trust::Loopback => format!("{host} · plain HTTP to this computer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code_for(json: &str) -> String {
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    #[test]
    fn a_code_round_trips() {
        let key = Key::new("tpl_abc".into());
        let pin = "ab".repeat(32);
        let text = encode("https://192.168.1.5:7411", &key, Some(&pin));
        assert!(text.starts_with("tplc1_") && !text.contains('='));
        assert_eq!(
            decode(&format!("  {text}\n")),
            Ok(Code {
                url: "https://192.168.1.5:7411".into(),
                key: key.clone(),
                trust: Trust::Pinned(pin.clone())
            })
        );
        let public = decode(&encode("https://templum.example.com/base/", &key, None)).unwrap();
        assert_eq!(
            (public.url.as_str(), &public.trust),
            ("https://templum.example.com/base", &Trust::Public)
        );
        for loopback in [
            "http://127.0.0.1:7411",
            "http://localhost:9",
            "http://[::1]:9",
        ] {
            let code = decode(&encode(loopback, &key, None)).unwrap();
            assert_eq!(code.trust, Trust::Loopback, "{loopback}");
        }
    }

    #[test]
    fn a_wrong_code_says_why() {
        let key = Key::new("tpl_abc".into());
        let text = encode("https://t.example.com", &key, None);
        assert_eq!(decode("hello"), Err(Wrong::NotACode));
        assert_eq!(decode(""), Err(Wrong::NotACode));
        assert_eq!(decode("tplc0_xx"), Err(Wrong::NotACode));
        assert_eq!(decode(&text[..text.len() - 9]), Err(Wrong::CutShort));
        assert_eq!(decode(PREFIX), Err(Wrong::CutShort));
        assert_eq!(decode("tplc2_xxx"), Err(Wrong::Newer));
        assert_eq!(decode("tplc10_xxx"), Err(Wrong::Newer));
        assert_eq!(
            decode(&encode("http://192.168.1.5:7411", &key, None)),
            Err(Wrong::Plain)
        );
        let pin = "ab".repeat(32);
        assert_eq!(
            decode(&encode("http://192.168.1.5:7411", &key, Some(&pin))),
            Err(Wrong::CutShort),
            "a pinned code is https"
        );
        for json in [
            r#"{"url":"https://t.example.com","token":"abc"}"#,
            r#"{"url":"https://t.example.com"}"#,
            r#"{"token":"tpl_abc"}"#,
            r#"{"url":"nonsense","token":"tpl_abc"}"#,
            r#"{"url":"https://t.example.com","token":"tpl_abc","pin":"abcd"}"#,
            r#"{"url":"ftp://t.example.com","token":"tpl_abc"}"#,
            r#"{"url":"https://templum.example.com@evil.com","token":"tpl_abc"}"#,
            r#"{"url":"https://user:pw@t.example.com:7411","token":"tpl_abc"}"#,
            r#"{"url":"https://t.example.com/base?x=1","token":"tpl_abc"}"#,
            r#"{"url":"https://t.example.com/base#top","token":"tpl_abc"}"#,
            "not json",
        ] {
            assert_eq!(decode(&code_for(json)), Err(Wrong::CutShort), "{json}");
        }
        let extra = r#"{"url":"https://t.example.com","token":"tpl_abc","name":"x"}"#;
        assert!(decode(&code_for(extra)).is_ok());
    }

    #[test]
    fn a_code_says_where_it_goes() {
        let key = Key::new("tpl_abc".into());
        let public = decode(&encode("https://templum.example.com", &key, None)).unwrap();
        assert_eq!(public.host(), "templum.example.com");
        assert_eq!(
            public.said(),
            "templum.example.com · checks its public certificate"
        );
        let pin = format!("abcd{}9f", "00".repeat(29));
        let pinned = decode(&encode("https://192.168.1.5:7411", &key, Some(&pin))).unwrap();
        assert_eq!(pinned.host(), "192.168.1.5:7411");
        assert_eq!(
            pinned.said(),
            "192.168.1.5:7411 · pins Templum's certificate AB:CD:…:9F"
        );
        let local = decode(&encode("http://127.0.0.1:7411", &key, None)).unwrap();
        assert_eq!(local.said(), "127.0.0.1:7411 · plain HTTP to this computer");
    }

    #[test]
    fn the_key_never_shows() {
        let code = decode(&encode(
            "https://t.example.com",
            &Key::new("tpl_SECRET".into()),
            None,
        ))
        .unwrap();
        for shown in [
            format!("{code:?}"),
            format!("{}", code.key),
            format!("{:?}", code.key),
        ] {
            assert!(!shown.contains("SECRET"), "{shown}");
        }
        assert_eq!(code.key.expose(), "tpl_SECRET");
    }
}
