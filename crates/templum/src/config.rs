//! `config.toml`. For now it holds only `[theme]`, the light and dark theme names; the theme
//! picker saves them.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TomlError};

use crate::theme::{DEFAULT_DARK, DEFAULT_LIGHT};

pub(crate) const FILE: &str = "config.toml";

/// Templum's config folder: `$XDG_CONFIG_HOME/templum` or `~/.config/templum` on Linux and macOS
/// (the convention of terminal apps), `%APPDATA%\templum` on Windows.
pub(crate) fn dir() -> Option<PathBuf> {
    dir_from(
        cfg!(windows),
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::home_dir(),
    )
}

fn dir_from(
    windows: bool,
    appdata: Option<OsString>,
    xdg: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let base = if windows {
        appdata.map(PathBuf::from)
    } else {
        xdg.filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    base.map(|b| b.join("templum"))
}

/// The theme for light mode and the one for dark mode.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ThemeNames {
    pub light: String,
    pub dark: String,
}

impl Default for ThemeNames {
    fn default() -> Self {
        Self {
            light: DEFAULT_LIGHT.into(),
            dark: DEFAULT_DARK.into(),
        }
    }
}

impl ThemeNames {
    pub fn get(&self, dark: bool) -> &str {
        if dark { &self.dark } else { &self.light }
    }
}

/// `[theme]` from `<dir>/config.toml`. Missing file, missing keys or a broken file: the defaults.
pub(crate) fn read(dir: Option<&Path>) -> ThemeNames {
    let Some(path) = dir.map(|d| d.join(FILE)) else {
        return ThemeNames::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return ThemeNames::default(),
        Err(e) => {
            eprintln!("templum: couldn't read {}: {e}", path.display());
            return ThemeNames::default();
        }
    };
    let doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            let line = line_of(&text, &e);
            eprintln!(
                "templum: {} has an error on line {line}; using the default themes",
                path.display()
            );
            return ThemeNames::default();
        }
    };
    let get = |key: &str| {
        doc.get("theme")
            .and_then(|t| t.get(key))
            .and_then(Item::as_str)
            .map(str::to_string)
    };
    let mut names = ThemeNames::default();
    if let Some(light) = get("light") {
        names.light = light;
    }
    if let Some(dark) = get("dark") {
        names.dark = dark;
    }
    names
}

/// The 1-based line a parse error points at.
pub(crate) fn line_of(text: &str, err: &TomlError) -> usize {
    let at = err.span().map_or(0, |s| s.start.min(text.len()));
    text.as_bytes()[..at]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

#[derive(Debug)]
pub(crate) enum SaveError {
    /// No config folder could be found (no HOME, no APPDATA).
    NoDir,
    /// The file doesn't parse: it is left alone.
    Broken {
        line: usize,
    },
    NotATable,
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDir => write!(f, "no config folder"),
            Self::Broken { line } => write!(f, "{FILE} has an error on line {line}"),
            Self::NotATable => write!(f, "`theme` in {FILE} is not a table"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Sets `theme.dark` or `theme.light` in `<dir>/config.toml`, keeping everything else (comments
/// included). Creates the folder and the file; never writes over a file that doesn't parse.
pub(crate) fn save_theme(dir: &Path, dark: bool, name: &str) -> Result<(), SaveError> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(SaveError::Io(e)),
    };
    let mut doc: DocumentMut = text.parse().map_err(|e| SaveError::Broken {
        line: line_of(&text, &e),
    })?;
    let theme = doc
        .entry("theme")
        .or_insert(toml_edit::table())
        .as_table_like_mut()
        .ok_or(SaveError::NotATable)?;
    theme.insert(if dark { "dark" } else { "light" }, toml_edit::value(name));
    std::fs::create_dir_all(dir).map_err(SaveError::Io)?;
    std::fs::write(&path, doc.to_string()).map_err(SaveError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) {
        std::fs::write(dir.join(FILE), text).unwrap();
    }

    #[test]
    fn defaults_without_a_file_or_a_dir() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(Some(d.path())), ThemeNames::default());
        assert_eq!(read(None), ThemeNames::default());
        assert_eq!(ThemeNames::default().get(false), "Catppuccin Latte");
        assert_eq!(ThemeNames::default().get(true), "Catppuccin Mocha");
    }

    #[test]
    fn reads_theme_names() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "# mine\n[theme]\nlight = \"One Light\"\n");
        let names = read(Some(d.path()));
        assert_eq!(names.light, "One Light");
        assert_eq!(names.dark, "Catppuccin Mocha");
    }

    #[test]
    fn a_broken_file_gives_the_defaults() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[theme\n");
        assert_eq!(read(Some(d.path())), ThemeNames::default());
    }

    #[test]
    fn reads_defaults_when_theme_is_not_a_table() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "theme = \"Dracula\"\n");
        assert_eq!(read(Some(d.path())), ThemeNames::default());
    }

    #[test]
    fn line_of_points_at_the_error() {
        let text = "[theme]\nlight = \"x\"\ndark = @nope\n";
        let err = text.parse::<toml_edit::DocumentMut>().unwrap_err();
        assert_eq!(line_of(text, &err), 3);
    }

    #[test]
    fn dir_follows_the_platform_conventions() {
        let xdg = Some(std::ffi::OsString::from("/x"));
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            dir_from(false, None, xdg, home.clone()),
            Some(PathBuf::from("/x").join("templum"))
        );
        assert_eq!(
            dir_from(false, None, Some("".into()), home.clone()),
            Some(PathBuf::from("/home/u").join(".config").join("templum"))
        );
        assert_eq!(dir_from(false, None, None, None), None);
        let appdata = Some(std::ffi::OsString::from("C:/Users/u/AppData/Roaming"));
        assert_eq!(
            dir_from(true, appdata, None, home),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming").join("templum"))
        );
    }

    #[test]
    fn saving_keeps_comments_and_other_tables() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "# my settings\n[theme]\nlight = \"One Light\" # sunny\n\n[other]\nkey = 1\n",
        );
        save_theme(d.path(), true, "One Dark").unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("light = \"One Light\" # sunny"), "{text}");
        assert!(text.contains("[other]\nkey = 1"), "{text}");
        assert_eq!(
            read(Some(d.path())),
            ThemeNames {
                light: "One Light".into(),
                dark: "One Dark".into()
            }
        );
    }

    #[test]
    fn saving_creates_the_folder_and_the_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("templum");
        save_theme(&dir, false, "Catppuccin Frappé").unwrap();
        assert_eq!(read(Some(&dir)).light, "Catppuccin Frappé");
    }

    #[test]
    fn saving_never_overwrites_a_broken_file() {
        let d = tempfile::tempdir().unwrap();
        let broken = "[theme]\nlight = \"One Light\"\ndark = @nope\n";
        write(d.path(), broken);
        let err = save_theme(d.path(), true, "One Dark").unwrap_err();
        assert_eq!(err.to_string(), "config.toml has an error on line 3");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            broken
        );
    }

    #[test]
    fn saving_refuses_a_non_table_theme() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "theme = \"Dracula\"\n");
        let err = save_theme(d.path(), false, "One Light").unwrap_err();
        assert_eq!(err.to_string(), "`theme` in config.toml is not a table");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            "theme = \"Dracula\"\n"
        );
    }
}
