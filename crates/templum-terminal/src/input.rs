//! Bytes sent to the shell for key presses and pastes (xterm conventions).

/// A key press, as the UI toolkit reports it (GPUI key names: "enter", "up", "f5", "a"...).
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyInput<'a> {
    pub key: &'a str,
    /// The text the key would type, if any.
    pub key_char: Option<&'a str>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Bytes for a key press. `None` means "not a terminal key": printable text, IME and dead
/// keys go through the UI's text-input path instead.
pub fn encode_key(k: &KeyInput, app_cursor: bool) -> Option<Vec<u8>> {
    // Windows reports AltGr as Ctrl+Alt; when it types a printable char, that char is text.
    if k.ctrl
        && k.alt
        && k.key_char
            .is_some_and(|c| c != k.key && !c.is_empty() && !c.chars().any(char::is_control))
    {
        return None;
    }
    // xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    let m = 1 + k.shift as u8 + 2 * k.alt as u8 + 4 * k.ctrl as u8;
    let cursor = |f: char| match (m > 1, app_cursor) {
        (true, _) => format!("\x1b[1;{m}{f}").into_bytes(),
        (false, true) => format!("\x1bO{f}").into_bytes(),
        (false, false) => format!("\x1b[{f}").into_bytes(),
    };
    let ss3 = |f: char| match m > 1 {
        true => format!("\x1b[1;{m}{f}").into_bytes(),
        false => format!("\x1bO{f}").into_bytes(),
    };
    let tilde = |code: u8| match m > 1 {
        true => format!("\x1b[{code};{m}~").into_bytes(),
        false => format!("\x1b[{code}~").into_bytes(),
    };
    let meta = |bytes: &[u8]| match k.alt {
        true => [b"\x1b", bytes].concat(),
        false => bytes.to_vec(),
    };
    Some(match k.key {
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "enter" => meta(b"\r"),
        "backspace" if k.ctrl => meta(b"\x08"),
        "backspace" => meta(b"\x7f"),
        "tab" if k.shift => b"\x1b[Z".to_vec(),
        "tab" => meta(b"\t"),
        "escape" => meta(b"\x1b"),
        "space" if k.ctrl => meta(&[0]),
        key if k.ctrl && key.len() == 1 => {
            let c = key.as_bytes()[0].to_ascii_lowercase();
            let byte = match c {
                b'a'..=b'z' | b'[' | b'\\' | b']' | b'^' | b'_' => c & 0x1f,
                b'@' | b'2' => 0,
                b'3' => 0x1b,
                b'4' => 0x1c,
                b'5' => 0x1d,
                b'6' => 0x1e,
                b'7' | b'/' => 0x1f,
                b'8' => 0x7f,
                _ => return None,
            };
            meta(&[byte])
        }
        _ if k.alt => {
            let text = k.key_char.unwrap_or(k.key);
            if text.chars().count() != 1 {
                return None;
            }
            meta(text.as_bytes())
        }
        _ => return None,
    })
}

/// Bytes for pasted text. Bracketed when the app asked for it (and then any ESC is dropped,
/// so the payload can't close the bracket early and run commands); otherwise newlines
/// become carriage returns, like typing Enter.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let body: String = text.chars().filter(|&c| c != '\x1b').collect();
        [b"\x1b[200~".as_slice(), body.as_bytes(), b"\x1b[201~"].concat()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str) -> KeyInput<'_> {
        KeyInput {
            key,
            ..KeyInput::default()
        }
    }

    fn enc(k: KeyInput) -> Option<Vec<u8>> {
        encode_key(&k, false)
    }

    #[test]
    fn plain_keys_encode_like_xterm() {
        assert_eq!(enc(key("enter")).unwrap(), b"\r");
        assert_eq!(enc(key("backspace")).unwrap(), b"\x7f");
        assert_eq!(enc(key("tab")).unwrap(), b"\t");
        assert_eq!(
            enc(KeyInput {
                shift: true,
                ..key("tab")
            })
            .unwrap(),
            b"\x1b[Z"
        );
        assert_eq!(enc(key("escape")).unwrap(), b"\x1b");
        assert_eq!(enc(key("up")).unwrap(), b"\x1b[A");
        assert_eq!(encode_key(&key("up"), true).unwrap(), b"\x1bOA");
        assert_eq!(enc(key("home")).unwrap(), b"\x1b[H");
        assert_eq!(enc(key("pageup")).unwrap(), b"\x1b[5~");
        assert_eq!(enc(key("delete")).unwrap(), b"\x1b[3~");
    }

    #[test]
    fn function_keys_and_modifiers() {
        assert_eq!(enc(key("f1")).unwrap(), b"\x1bOP");
        assert_eq!(enc(key("f5")).unwrap(), b"\x1b[15~");
        assert_eq!(enc(key("f12")).unwrap(), b"\x1b[24~");
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                ..key("right")
            })
            .unwrap(),
            b"\x1b[1;5C"
        );
        assert_eq!(
            encode_key(
                &KeyInput {
                    ctrl: true,
                    ..key("right")
                },
                true
            )
            .unwrap(),
            b"\x1b[1;5C"
        );
        assert_eq!(
            enc(KeyInput {
                shift: true,
                ..key("f1")
            })
            .unwrap(),
            b"\x1b[1;2P"
        );
        assert_eq!(
            enc(KeyInput {
                alt: true,
                ..key("delete")
            })
            .unwrap(),
            b"\x1b[3;3~"
        );
    }

    #[test]
    fn ctrl_and_alt_combinations() {
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                ..key("c")
            })
            .unwrap(),
            [3]
        );
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                shift: true,
                ..key("c")
            })
            .unwrap(),
            [3]
        );
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                ..key("[")
            })
            .unwrap(),
            [0x1b]
        );
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                ..key("space")
            })
            .unwrap(),
            [0]
        );
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                alt: true,
                ..key("a")
            })
            .unwrap(),
            [0x1b, 1]
        );
        assert_eq!(
            enc(KeyInput {
                alt: true,
                key_char: Some("x"),
                ..key("x")
            })
            .unwrap(),
            b"\x1bx"
        );
        assert_eq!(
            enc(KeyInput {
                alt: true,
                key_char: Some("ç"),
                ..key("ç")
            })
            .unwrap(),
            "\x1bç".as_bytes()
        );
        assert_eq!(
            enc(KeyInput {
                alt: true,
                ..key("backspace")
            })
            .unwrap(),
            b"\x1b\x7f"
        );
    }

    #[test]
    fn printable_text_goes_to_the_text_input_path() {
        assert_eq!(
            enc(KeyInput {
                key_char: Some("a"),
                ..key("a")
            }),
            None
        );
        assert_eq!(
            enc(KeyInput {
                key_char: Some(" "),
                ..key("space")
            }),
            None
        );
        assert_eq!(
            enc(KeyInput {
                shift: true,
                key_char: Some("A"),
                ..key("a")
            }),
            None
        );
        // Windows reports AltGr as Ctrl+Alt: AltGr+q on ABNT2 types "/", not ESC ^Q.
        assert_eq!(
            enc(KeyInput {
                ctrl: true,
                alt: true,
                key_char: Some("/"),
                ..key("q")
            }),
            None
        );
    }

    #[test]
    fn paste_is_bracketed_and_sanitized() {
        assert_eq!(paste_bytes("ls\n", true), b"\x1b[200~ls\n\x1b[201~");
        assert_eq!(
            paste_bytes("a\x1b[201~rm -rf ~\n", true),
            b"\x1b[200~a[201~rm -rf ~\n\x1b[201~"
        );
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
    }
}
