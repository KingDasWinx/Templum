//! Bytes sent to the shell for key presses, mouse events and pastes (xterm conventions, and the
//! kitty keyboard protocol for programs that turn it on).

use alacritty_terminal::term::TermMode;

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

/// Bytes for a key press, in the terminal's `mode` (cursor keys, the kitty keyboard's flags).
/// `None` means "not a terminal key": printable text, IME and dead keys go through the UI's
/// text-input path instead.
pub(crate) fn encode_key(k: &KeyInput, mode: TermMode) -> Option<Vec<u8>> {
    // Windows reports AltGr as Ctrl+Alt; when it types a printable char, that char is text.
    if k.ctrl
        && k.alt
        && k.key_char
            .is_some_and(|c| c != k.key && !c.is_empty() && !c.chars().any(char::is_control))
    {
        return None;
    }
    // xterm modifier parameter (the kitty protocol's too): 1 + shift + 2*alt + 4*ctrl.
    let m = 1 + k.shift as u8 + 2 * k.alt as u8 + 4 * k.ctrl as u8;
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL)
        && let Some(bytes) = kitty_key(k, m, mode)
    {
        return Some(bytes);
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
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
                b'7' | b'/' | b'-' => 0x1f,
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

/// The keys `send_keys` and `terminal_input` take, as their refusals list them.
pub const KEY_NAMES: &str = "[ctrl+][alt+][shift+] and enter, escape, tab, backspace, delete, \
    insert, up, down, left, right, home, end, page_up, page_down, space, f1–f12, or one character \
    (2, y, A, ç)";

/// A key as `named_key` reads its name: what GPUI would report for it, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedKey {
    pub key: String,
    pub key_char: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl NamedKey {
    /// Whether the key is a lone ESC to a program that asked for nothing special: `escape`, and
    /// the chords that send the same byte (Ctrl+[, Ctrl+3). Alt+key sends ESC and then the key.
    pub fn sends_lone_esc(&self) -> bool {
        encode_key(&self.input(), TermMode::empty()).as_deref() == Some(b"\x1b")
    }

    pub fn input(&self) -> KeyInput<'_> {
        KeyInput {
            key: &self.key,
            key_char: self.key_char.as_deref(),
            ctrl: self.ctrl,
            alt: self.alt,
            shift: self.shift,
        }
    }
}

/// A key as `send_keys` and `terminal_input` name it (`KEY_NAMES`), as GPUI would report it;
/// None for any other name.
pub fn named_key(name: &str) -> Option<NamedKey> {
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut rest = name;
    loop {
        let (flag, tail) = if let Some(tail) = rest.strip_prefix("ctrl+") {
            (&mut ctrl, tail)
        } else if let Some(tail) = rest.strip_prefix("alt+") {
            (&mut alt, tail)
        } else if let Some(tail) = rest.strip_prefix("shift+") {
            (&mut shift, tail)
        } else {
            break;
        };
        if std::mem::replace(flag, true) {
            return None;
        }
        rest = tail;
    }
    let (key, key_char) = match rest {
        "enter" | "escape" | "tab" | "backspace" | "delete" | "insert" | "up" | "down" | "left"
        | "right" | "home" | "end" | "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8"
        | "f9" | "f10" | "f11" | "f12" => (rest.to_owned(), None),
        "page_up" => ("pageup".to_owned(), None),
        "page_down" => ("pagedown".to_owned(), None),
        "space" => ("space".to_owned(), Some(" ".to_owned())),
        _ => {
            let mut chars = rest.chars();
            let c = chars.next()?;
            if chars.next().is_some() || c.is_control() {
                return None;
            }
            let mut lower = c.to_lowercase();
            let key = match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            };
            shift |= c.is_uppercase();
            // Ctrl makes it a chord, not text (so Ctrl+Alt+letter is never AltGr's). Alt keeps the
            // character, so Alt+X sends ESC X.
            let text = match (ctrl, shift) {
                (true, _) => None,
                (false, true) => Some(c.to_uppercase().collect()),
                (false, false) => Some(c.to_string()),
            };
            (key.to_string(), text)
        }
    };
    Some(NamedKey {
        key,
        key_char,
        ctrl,
        alt,
        shift,
    })
}

/// What `named_key` reads back as `k`: the name of a key the keyboard pressed, for another
/// Templum to press at home (`terminal_input`). A printable character typed, with no Ctrl or Alt,
/// or with both and a character other than its key (AltGr, the rule `encode_key` has), is that
/// character; any other key is its modifiers (`ctrl+`, `alt+`, `shift+`, in that order) and its
/// name. None for a key `named_key` has no name for (F13, a media key, a lone modifier).
pub fn key_name(k: &KeyInput) -> Option<String> {
    if let Some(text) = typed_text(k) {
        return one_char(text);
    }
    let key = match k.key {
        "pageup" => "page_up".to_owned(),
        "pagedown" => "page_down".to_owned(),
        "enter" | "escape" | "tab" | "backspace" | "delete" | "insert" | "up" | "down" | "left"
        | "right" | "home" | "end" | "space" | "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7"
        | "f8" | "f9" | "f10" | "f11" | "f12" => k.key.to_owned(),
        other => one_char(other)?,
    };
    let mut name = String::new();
    for (held, prefix) in [(k.ctrl, "ctrl+"), (k.alt, "alt+"), (k.shift, "shift+")] {
        if held {
            name += prefix;
        }
    }
    Some(name + &key)
}

/// The text `k` types, when it does: a printable character with no Ctrl or Alt, or with both and a
/// character other than its key (AltGr). The platform's text-input path carries it too (IME,
/// dead keys), so a key handler that leaves such a key alone gets it exactly once.
pub fn typed_text<'a>(k: &KeyInput<'a>) -> Option<&'a str> {
    let typed = k
        .key_char
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
    let altgr = k.ctrl && k.alt && typed.is_some_and(|t| t != k.key);
    typed.filter(|_| altgr || (!k.ctrl && !k.alt && k.key != "space"))
}

/// `text` as a name, when it is one character; a space is `space`.
fn one_char(text: &str) -> Option<String> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(' '), None) => Some("space".into()),
        (Some(c), None) if !c.is_control() => Some(c.to_string()),
        _ => None,
    }
}

/// Each character of `text` that is not a control character, as its own key name: what an IME
/// or a dead key committed.
pub fn text_names(text: &str) -> Vec<String> {
    text.chars()
        .filter_map(|c| one_char(c.encode_utf8(&mut [0; 4])))
        .collect()
}

/// A key in the kitty keyboard protocol (sw.kovidgoyal.net/kitty/keyboard-protocol), as
/// `CSI code[:shifted];modifiers[;text] u`: what legacy bytes can't tell apart (Shift+Enter,
/// Ctrl+Shift+letter, Esc) or, when the program asked for every key, any key. None: the legacy
/// bytes say it (arrows, F-keys, Home... already carry their modifiers; plain text is text).
// ponytail: presses only, no repeat or release events nor lone modifier keys; they need key-up
// events from the UI.
fn kitty_key(k: &KeyInput, m: u8, mode: TermMode) -> Option<Vec<u8>> {
    let every = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);
    let (code, text_key) = match k.key {
        "escape" => (27, false),
        "enter" => (13, false),
        "tab" => (9, false),
        "backspace" => (127, false),
        "space" => (32, true),
        key => {
            let mut chars = key.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            (u32::from(c), true)
        }
    };
    // Enter, Tab and Backspace alone keep their bytes, so a shell left in this mode by a crashed
    // program still runs commands; text is text unless Ctrl or Alt is held.
    let plain = if text_key {
        !k.ctrl && !k.alt
    } else {
        m == 1 && k.key != "escape"
    };
    if plain && !every {
        return None;
    }
    let mut seq = format!("\x1b[{code}");
    let typed = k
        .key_char
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
    if mode.contains(TermMode::REPORT_ALTERNATE_KEYS)
        && k.shift
        && let Some(shifted) = typed.filter(|t| *t != k.key && t.chars().count() == 1)
    {
        seq += &format!(":{}", u32::from(shifted.chars().next()?));
    }
    let text = typed.filter(|_| {
        text_key && every && !k.ctrl && !k.alt && mode.contains(TermMode::REPORT_ASSOCIATED_TEXT)
    });
    if m > 1 || text.is_some() {
        seq += &format!(";{m}");
    }
    if let Some(text) = text {
        let points: Vec<String> = text.chars().map(|c| u32::from(c).to_string()).collect();
        seq += &format!(";{}", points.join(":"));
    }
    seq.push('u');
    Some(seq.into_bytes())
}

/// A mouse button, or a step of the wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    /// The pointer moved to another cell.
    Move,
}

/// A mouse event over a terminal, for the program in it that asked for the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseInput {
    /// None: a move with no button down.
    pub button: Option<Button>,
    pub kind: MouseKind,
    /// The viewport cell (row 0 = top visible line).
    pub row: usize,
    pub col: usize,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Bytes for a mouse event, as the program asked for them (xterm's 1000/1002/1003 and their SGR
/// and UTF-8 forms). None: it didn't ask for this event, or a cell too far for the old form.
pub(crate) fn encode_mouse(e: &MouseInput, mode: TermMode) -> Option<Vec<u8>> {
    let wanted = match (e.kind, e.button) {
        (MouseKind::Move, None) => mode.contains(TermMode::MOUSE_MOTION),
        (MouseKind::Move, Some(_)) => {
            mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG)
        }
        _ => mode.intersects(TermMode::MOUSE_MODE),
    };
    if !wanted {
        return None;
    }
    let button = match e.button {
        None => 3,
        Some(Button::Left) => 0,
        Some(Button::Middle) => 1,
        Some(Button::Right) => 2,
        Some(Button::WheelUp) => 64,
        Some(Button::WheelDown) => 65,
    };
    let extra = 4 * e.shift as u32
        + 8 * e.alt as u32
        + 16 * e.ctrl as u32
        + if e.kind == MouseKind::Move { 32 } else { 0 };
    let (col, row) = (e.col as u32 + 1, e.row as u32 + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if e.kind == MouseKind::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{};{col};{row}{end}", button + extra).into_bytes());
    }
    // The old form doesn't say which button was let go.
    let button = if e.kind == MouseKind::Release {
        3
    } else {
        button
    };
    let mut bytes = b"\x1b[M".to_vec();
    bytes.push(u8::try_from(32 + button + extra).ok()?);
    for v in [col, row] {
        if mode.contains(TermMode::UTF8_MOUSE) {
            let c = char::from_u32(32 + v).filter(|_| 32 + v < 0x800)?;
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        } else {
            bytes.push(u8::try_from(32 + v).ok()?);
        }
    }
    Some(bytes)
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

/// Bytes for text typed in for the person to send (a note, a card): never an Enter. Bracketed
/// like a paste when the app asked for it; otherwise its lines are joined with spaces and every
/// other control character becomes a space. Trailing newlines are dropped.
pub fn typed_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.trim_end_matches(['\r', '\n']);
    if bracketed {
        paste_bytes(text, true)
    } else {
        text.replace("\r\n", "\n")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_never_presses_enter() {
        assert_eq!(typed_bytes("ls\r\nrm -rf x\n", false), b"ls rm -rf x");
        assert_eq!(typed_bytes("a\tb\x1b[A", false), b"a b [A");
        assert_eq!(typed_bytes("a\nb\n", true), b"\x1b[200~a\nb\x1b[201~");
    }

    /// A delegated task: its header, its lines and a paste end of its own stay inside one paste,
    /// whose end (and the Enter after it) only Templum writes.
    #[test]
    fn a_task_cant_end_its_paste_early() {
        let task = "[from api (b7k2m)]\nfix it\x1b[201~\rrm -rf ~\n";
        assert_eq!(
            typed_bytes(task, true),
            b"\x1b[200~[from api (b7k2m)]\nfix it[201~\rrm -rf ~\x1b[201~"
        );
        assert_eq!(
            typed_bytes(task, false),
            b"[from api (b7k2m)] fix it [201~ rm -rf ~"
        );
    }

    fn key(key: &str) -> KeyInput<'_> {
        KeyInput {
            key,
            ..KeyInput::default()
        }
    }

    fn enc(k: KeyInput) -> Option<Vec<u8>> {
        encode_key(&k, TermMode::empty())
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
        assert_eq!(
            encode_key(&key("up"), TermMode::APP_CURSOR).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(enc(key("home")).unwrap(), b"\x1b[H");
        assert_eq!(enc(key("pageup")).unwrap(), b"\x1b[5~");
        assert_eq!(enc(key("delete")).unwrap(), b"\x1b[3~");
    }

    /// The keys an agent names (`send_keys`) send what the person's keyboard would, in the
    /// program's mode; a character is text, typed as it is.
    #[test]
    fn every_key_that_sends_a_lone_esc_says_so() {
        let lone = |name: &str| named_key(name).unwrap().sends_lone_esc();
        for name in ["escape", "ctrl+[", "ctrl+3", "shift+escape"] {
            assert!(lone(name), "{name}");
        }
        for name in [
            "alt+escape",
            "alt+[",
            "ctrl+alt+[",
            "[",
            "j",
            "ctrl+j",
            "enter",
            "f1",
        ] {
            assert!(!lone(name), "{name}");
        }
    }

    fn pressed<'a>(
        key: &'a str,
        key_char: Option<&'a str>,
        (ctrl, alt, shift): (bool, bool, bool),
    ) -> KeyInput<'a> {
        KeyInput {
            key,
            key_char,
            ctrl,
            alt,
            shift,
        }
    }

    #[test]
    fn printable_text_is_left_to_the_text_input_path() {
        let text =
            |key, key_char, ctrl, alt| typed_text(&pressed(key, key_char, (ctrl, alt, false)));
        // A letter, a composed dead key, and AltGr type; a shortcut, a space and Enter don't.
        assert_eq!(text("a", Some("a"), false, false), Some("a"));
        assert_eq!(text("a", Some("á"), false, false), Some("á"));
        assert_eq!(text("q", Some("@"), true, true), Some("@"));
        assert_eq!(text("c", Some("c"), true, false), None);
        assert_eq!(text("b", Some("b"), false, true), None);
        assert_eq!(text("space", Some(" "), false, false), None);
        assert_eq!(text("enter", Some("\n"), false, false), None);
        assert_eq!(text("left", None, false, false), None);
    }

    #[test]
    fn names_are_what_the_keyboard_typed() {
        let k = |key, key_char, ctrl, alt, shift| pressed(key, key_char, (ctrl, alt, shift));
        let name = |k: KeyInput| key_name(&k);
        assert_eq!(
            name(k("a", Some("A"), false, false, true)).as_deref(),
            Some("A")
        );
        // AltGr on Windows is Ctrl+Alt.
        assert_eq!(
            name(k("q", Some("@"), true, true, false)).as_deref(),
            Some("@")
        );
        assert_eq!(
            name(k("p", None, true, false, true)).as_deref(),
            Some("ctrl+shift+p")
        );
        assert_eq!(
            name(k("c", Some("c"), true, false, false)).as_deref(),
            Some("ctrl+c")
        );
        assert_eq!(
            name(k("pageup", None, false, false, false)).as_deref(),
            Some("page_up")
        );
        assert_eq!(
            name(k("f5", None, false, false, true)).as_deref(),
            Some("shift+f5")
        );
        assert_eq!(
            name(k("left", None, false, true, false)).as_deref(),
            Some("alt+left")
        );
        assert_eq!(
            name(k("space", Some(" "), false, false, false)).as_deref(),
            Some("space")
        );
        assert_eq!(
            name(k("x", Some("x"), false, true, false)).as_deref(),
            Some("alt+x"),
            "Alt keeps the key a chord"
        );
        assert_eq!(name(k("f13", None, false, false, false)), None);
        assert_eq!(name(k("shift", None, false, false, true)), None);
        assert_eq!(text_names("çã\u{7}! "), vec!["ç", "ã", "!", "space"]);
    }

    #[test]
    fn a_name_reads_back_as_the_same_key() {
        let k = |key, key_char, ctrl, alt, shift| pressed(key, key_char, (ctrl, alt, shift));
        let keys = [
            k("a", Some("A"), false, false, true),
            k("a", Some("a"), false, false, false),
            k("p", None, true, false, true),
            k("c", None, true, false, false),
            k("pageup", None, false, false, false),
            k("pagedown", None, false, false, true),
            k("f5", None, false, false, true),
            k("left", None, false, true, false),
            k("enter", None, false, false, false),
            k("tab", None, false, false, true),
            k("escape", None, false, false, false),
            k("space", Some(" "), false, false, false),
            k("delete", None, true, true, false),
            k("ç", Some("ç"), false, false, false),
            k("x", Some("x"), false, true, false),
        ];
        let modes = [
            TermMode::empty(),
            TermMode::APP_CURSOR,
            TermMode::DISAMBIGUATE_ESC_CODES,
            TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALL_KEYS_AS_ESC,
        ];
        for key in keys {
            let name = key_name(&key).expect("a name");
            let back = named_key(&name).unwrap_or_else(|| panic!("{name} reads back"));
            for mode in modes {
                assert_eq!(
                    encode_key(&back.input(), mode),
                    encode_key(&key, mode),
                    "{name} in {mode:?}"
                );
            }
        }
        // AltGr's character is text locally; home presses it as a key, which a program that asked
        // for every key as an escape code (kitty) gets as a code.
        let altgr = k("q", Some("@"), true, true, false);
        let back = named_key(&key_name(&altgr).unwrap()).unwrap();
        assert_eq!(encode_key(&back.input(), TermMode::empty()), None);
    }

    #[test]
    fn named_keys_send_what_a_keyboard_would() {
        let bytes = |name: &str, mode| encode_key(&named_key(name).unwrap().input(), mode);
        let (plain, kitty) = (TermMode::empty(), TermMode::DISAMBIGUATE_ESC_CODES);
        assert_eq!(bytes("escape", plain).unwrap(), b"\x1b");
        assert_eq!(
            bytes("escape", kitty).unwrap(),
            b"\x1b[27u",
            "Claude Code, Codex"
        );
        assert_eq!(bytes("up", plain).unwrap(), b"\x1b[A");
        assert_eq!(bytes("up", TermMode::APP_CURSOR).unwrap(), b"\x1bOA");
        assert_eq!(bytes("page_up", plain).unwrap(), b"\x1b[5~");
        assert_eq!(bytes("page_down", plain).unwrap(), b"\x1b[6~");
        assert_eq!(bytes("shift+tab", plain).unwrap(), b"\x1b[Z");
        assert_eq!(bytes("ctrl+c", plain).unwrap(), [3]);
        assert_eq!(bytes("ctrl+c", kitty).unwrap(), b"\x1b[99;5u");
        assert_eq!(bytes("enter", kitty).unwrap(), b"\r");
        assert_eq!(bytes("backspace", plain).unwrap(), b"\x7f");
        assert_eq!(bytes("delete", plain).unwrap(), b"\x1b[3~");
        for text in ["2", "y", "space"] {
            assert_eq!(bytes(text, kitty), None, "{text} is text");
        }
        assert_eq!(named_key("2").unwrap().key_char.as_deref(), Some("2"));
        assert_eq!(named_key("space").unwrap().key_char.as_deref(), Some(" "));
        for bad in ["foo", "ab", "", "ctrl+", "Enter", "\u{1b}", "\n"] {
            assert!(named_key(bad).is_none(), "{bad:?}");
        }
    }

    fn fields(name: &str) -> (String, Option<String>, bool, bool, bool) {
        let k = named_key(name).unwrap();
        (k.key, k.key_char, k.ctrl, k.alt, k.shift)
    }

    fn bytes(name: &str, mode: TermMode) -> Option<Vec<u8>> {
        encode_key(&named_key(name).unwrap().input(), mode)
    }

    #[test]
    fn names_carry_every_modifier_in_any_order() {
        assert_eq!(
            fields("ctrl+shift+p"),
            ("p".into(), None, true, false, true)
        );
        assert_eq!(named_key("shift+ctrl+p"), named_key("ctrl+shift+p"));
        assert_eq!(
            fields("A"),
            ("a".into(), Some("A".into()), false, false, true)
        );
        assert_eq!(
            fields("Ç"),
            ("ç".into(), Some("Ç".into()), false, false, true)
        );
        assert_eq!(
            fields("@"),
            ("@".into(), Some("@".into()), false, false, false)
        );
        assert_eq!(fields("ctrl+alt+a"), ("a".into(), None, true, true, false));
        assert_eq!(
            fields("+"),
            ("+".into(), Some("+".into()), false, false, false)
        );
        assert_eq!(fields("ctrl++"), ("+".into(), None, true, false, false));
        assert_eq!(fields("page_up").0, "pageup");
        for bad in [
            "ctrl+ctrl+p",
            "ctrl+",
            "f13",
            "F5",
            "\u{7}",
            "hyper+a",
            "ab",
            "",
        ] {
            assert_eq!(named_key(bad), None, "{bad:?}");
        }
        // Today's names mean what they meant.
        assert_eq!(
            fields("shift+tab"),
            ("tab".into(), None, false, false, true)
        );
        assert_eq!(fields("ctrl+c"), ("c".into(), None, true, false, false));
        assert_eq!(fields("space").1.as_deref(), Some(" "));
    }

    #[test]
    fn new_names_reach_the_program_as_keys_pressed_at_home() {
        let plain = TermMode::empty();
        assert_eq!(bytes("f5", plain), Some(b"\x1b[15~".to_vec()));
        assert_eq!(bytes("shift+f5", plain), Some(b"\x1b[15;2~".to_vec()));
        assert_eq!(bytes("f1", plain), Some(b"\x1bOP".to_vec()));
        assert_eq!(bytes("alt+left", plain), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(
            bytes("ctrl+up", TermMode::APP_CURSOR),
            Some(b"\x1b[1;5A".to_vec())
        );
        assert_eq!(bytes("ctrl+alt+delete", plain), Some(b"\x1b[3;7~".to_vec()));
        assert_eq!(bytes("insert", plain), Some(b"\x1b[2~".to_vec()));
        assert_eq!(bytes("page_down", plain), Some(b"\x1b[6~".to_vec()));
        assert_eq!(bytes("alt+x", plain), Some(b"\x1bx".to_vec()));
        // Alt keeps the case: ESC and the capital, not the lowercase letter.
        assert_eq!(bytes("alt+X", plain), Some(b"\x1bX".to_vec()));
        assert_eq!(bytes("alt+shift+x", plain), Some(b"\x1bX".to_vec()));
        assert_eq!(bytes("alt+ç", plain), Some("\u{1b}ç".as_bytes().to_vec()));
        assert_eq!(fields("alt+shift+x").1.as_deref(), Some("X"));
        // Claude Code's Shift+Enter and Ctrl+Shift+letter, in the kitty keyboard protocol.
        let kitty = TermMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(bytes("shift+enter", kitty), Some(b"\x1b[13;2u".to_vec()));
        assert_eq!(bytes("ctrl+shift+p", kitty), Some(b"\x1b[112;6u".to_vec()));
        // A character with no modifier is text: the press path types it.
        assert_eq!(bytes("ç", plain), None);
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
                TermMode::APP_CURSOR
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
    fn ctrl_minus_is_ctrl_underscore() {
        let ctrl_minus = KeyInput {
            ctrl: true,
            ..key("-")
        };
        assert_eq!(enc(ctrl_minus).unwrap(), [0x1f]);
    }

    fn kitty(k: KeyInput, mode: TermMode) -> String {
        String::from_utf8(encode_key(&k, mode).unwrap()).unwrap()
    }

    #[test]
    fn the_kitty_keyboard_tells_apart_what_legacy_bytes_cant() {
        let mode = TermMode::DISAMBIGUATE_ESC_CODES;
        let shift = |k| KeyInput { shift: true, ..k };
        let ctrl = |k| KeyInput { ctrl: true, ..k };
        assert_eq!(
            kitty(shift(key("enter")), mode),
            "\x1b[13;2u",
            "Shift+Enter"
        );
        assert_eq!(kitty(ctrl(key("enter")), mode), "\x1b[13;5u");
        assert_eq!(kitty(shift(key("tab")), mode), "\x1b[9;2u");
        assert_eq!(kitty(key("escape"), mode), "\x1b[27u");
        assert_eq!(kitty(ctrl(key("c")), mode), "\x1b[99;5u");
        assert_eq!(kitty(shift(ctrl(key("a"))), mode), "\x1b[97;6u");
        assert_eq!(
            kitty(ctrl(key("-")), mode),
            "\x1b[45;5u",
            "Claude Code's undo"
        );
        let alt_x = KeyInput {
            alt: true,
            key_char: Some("x"),
            ..key("x")
        };
        assert_eq!(kitty(alt_x, mode), "\x1b[120;3u");
        // A shell left in this mode still runs commands; text and arrows keep their bytes.
        assert_eq!(kitty(key("enter"), mode), "\r");
        assert_eq!(kitty(key("backspace"), mode), "\x7f");
        assert_eq!(kitty(ctrl(key("up")), mode), "\x1b[1;5A");
        let a = KeyInput {
            key_char: Some("a"),
            ..key("a")
        };
        assert_eq!(encode_key(&a, mode), None);
        let upper = KeyInput {
            key_char: Some("A"),
            ..shift(key("a"))
        };
        assert_eq!(encode_key(&upper, mode), None);
    }

    #[test]
    fn the_kitty_keyboard_can_report_every_key() {
        let every = TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALL_KEYS_AS_ESC;
        let a = KeyInput {
            key_char: Some("a"),
            ..key("a")
        };
        assert_eq!(kitty(a, every), "\x1b[97u");
        assert_eq!(kitty(key("enter"), every), "\x1b[13u");
        let upper = KeyInput {
            shift: true,
            key_char: Some("A"),
            ..key("a")
        };
        let all = every | TermMode::REPORT_ALTERNATE_KEYS | TermMode::REPORT_ASSOCIATED_TEXT;
        assert_eq!(kitty(upper, every), "\x1b[97;2u");
        assert_eq!(kitty(upper, all), "\x1b[97:65;2;65u");
    }

    fn mouse(button: Option<Button>, kind: MouseKind) -> MouseInput {
        MouseInput {
            button,
            kind,
            row: 2,
            col: 4,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    fn report(e: MouseInput, mode: TermMode) -> Option<String> {
        encode_mouse(&e, mode).map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn mouse_events_reach_only_programs_that_asked() {
        use MouseKind::*;
        let left = Some(Button::Left);
        assert_eq!(report(mouse(left, Press), TermMode::empty()), None);
        let clicks = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(report(mouse(left, Press), clicks).unwrap(), "\x1b[<0;5;3M");
        assert_eq!(
            report(mouse(left, Release), clicks).unwrap(),
            "\x1b[<0;5;3m"
        );
        let wheel = mouse(Some(Button::WheelDown), Press);
        assert_eq!(report(wheel, clicks).unwrap(), "\x1b[<65;5;3M");
        let ctrl_right = MouseInput {
            ctrl: true,
            ..mouse(Some(Button::Right), Press)
        };
        assert_eq!(report(ctrl_right, clicks).unwrap(), "\x1b[<18;5;3M");
        // Moves: while a button is held for 1002, any for 1003.
        assert_eq!(report(mouse(left, Move), clicks), None);
        let drags = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(report(mouse(left, Move), drags).unwrap(), "\x1b[<32;5;3M");
        assert_eq!(report(mouse(None, Move), drags), None);
        let moves = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        assert_eq!(report(mouse(None, Move), moves).unwrap(), "\x1b[<35;5;3M");
    }

    #[test]
    fn mouse_events_in_the_old_forms() {
        let clicks = TermMode::MOUSE_REPORT_CLICK;
        let press = mouse(Some(Button::Left), MouseKind::Press);
        assert_eq!(report(press, clicks).unwrap(), "\x1b[M %#");
        let release = mouse(Some(Button::Left), MouseKind::Release);
        assert_eq!(
            report(release, clicks).unwrap(),
            "\x1b[M#%#",
            "no button said"
        );
        let far = MouseInput { col: 300, ..press };
        assert_eq!(report(far, clicks), None, "past what a byte holds");
        let utf8 = clicks | TermMode::UTF8_MOUSE;
        assert_eq!(report(far, utf8).unwrap(), "\x1b[M \u{14d}#");
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
