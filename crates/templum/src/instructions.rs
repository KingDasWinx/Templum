//! The instructions the person writes for their agents: each a name, a Markdown text, on or off,
//! for every workspace or one and for every agent or chosen ones. App-wide like the checklists,
//! kept in `instructions.json`; a hook gives the ones that apply to an agent as its session
//! starts (docs/specs/instructions.md).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::{self, Loaded};

/// The most characters of a name.
pub(crate) const NAME_MAX: usize = 60;
/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Instructions {
    pub version: u32,
    /// The last id given out: never reused.
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub instructions: Vec<Instruction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Instruction {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub text: String,
    /// Off keeps it and gives it to no one.
    #[serde(default = "on")]
    pub on: bool,
    /// The id of the workspace it is for; None: every workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The ids (Settings ▸ Agents') of the agents it is for; None: every agent. Some of none
    /// (every one removed from Settings) is for no one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<String>>,
}

fn on() -> bool {
    true
}

/// What the person writes in the editor: an instruction without its id and its switch.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub name: String,
    pub text: String,
    pub workspace: Option<String>,
    pub agents: Option<Vec<String>>,
}

impl Default for Instructions {
    fn default() -> Self {
        Instructions {
            version: VERSION,
            next_id: 0,
            instructions: Vec::new(),
        }
    }
}

impl Instructions {
    /// A new instruction, on, after the others; returns its id.
    pub fn add(&mut self, draft: Draft) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.instructions.push(Instruction {
            id,
            name: draft.name,
            text: draft.text,
            on: true,
            workspace: draft.workspace,
            agents: draft.agents,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Instruction> {
        self.instructions.iter().find(|i| i.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Instruction> {
        self.instructions.iter_mut().find(|i| i.id == id)
    }

    /// Rewrites instruction `id` from `draft`, keeping its place and its switch.
    pub fn set(&mut self, id: u64, draft: Draft) {
        if let Some(i) = self.get_mut(id) {
            i.name = draft.name;
            i.text = draft.text;
            i.workspace = draft.workspace;
            i.agents = draft.agents;
        }
    }

    pub fn set_on(&mut self, id: u64, on: bool) {
        if let Some(i) = self.get_mut(id) {
            i.on = on;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.instructions.retain(|i| i.id != id);
    }

    /// The instructions that apply in workspace `workspace` to the agent `agent` says yes to
    /// (given an agent's id): on, with text, for every workspace or this one, for every agent or
    /// one it accepts. The every-workspace ones first, then the workspace's own, each in the
    /// list's order.
    pub fn applying<'a>(
        &'a self,
        workspace: &str,
        agent: impl Fn(&str) -> bool,
    ) -> Vec<&'a Instruction> {
        let applies = |i: &&Instruction| {
            i.on && !clean_text(&i.text).trim().is_empty()
                && i.agents
                    .as_ref()
                    .is_none_or(|agents| agents.iter().any(|a| agent(a)))
        };
        let own = |i: &&Instruction| i.workspace.as_deref() == Some(workspace);
        let every = self.instructions.iter().filter(|i| i.workspace.is_none());
        let mine = self.instructions.iter().filter(own);
        every.chain(mine).filter(applies).collect()
    }
}

/// A character no editor shows: zero widths, direction overrides, soft hyphens, tags, variation
/// selectors, fillers, which can carry hidden text. (A variation selector after an emoji only
/// picks its look: agents read the text without it.)
pub(crate) fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// A person's Markdown as agents are told it: its lines kept (and tabs), no other control
/// character, no invisible one, no blank lines at the end.
pub(crate) fn clean_text(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|&c| (c == '\n' || c == '\t' || !c.is_control()) && !invisible(c))
        .collect();
    kept.trim_end().to_string()
}

pub(crate) fn path(dir: &Path) -> PathBuf {
    dir.join("instructions.json")
}

pub(crate) fn load(dir: &Path) -> Loaded<Instructions> {
    match session::read::<Instructions>(&path(dir)) {
        Loaded::Saved(mut saved) => {
            let ids = saved.instructions.iter_mut().map(|i| &mut i.id);
            session::unique_ids(ids, &mut saved.next_id);
            Loaded::Saved(saved)
        }
        other => other,
    }
}

/// Writes the instructions; like canvases, a write older than the last one done is dropped.
pub(crate) fn save(dir: &Path, instructions: &Instructions, generation: u64) -> io::Result<()> {
    let text = serde_json::to_string_pretty(instructions).map_err(io::Error::other)?;
    session::write_newest(&path(dir), &text, generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str, text: &str, workspace: Option<&str>) -> Draft {
        Draft {
            name: name.into(),
            text: text.into(),
            workspace: workspace.map(str::to_string),
            agents: None,
        }
    }

    #[test]
    fn the_file_round_trips_and_an_older_one_loads() {
        let dir = tempfile::tempdir().unwrap();
        // A file without `on`, `workspace` and `agents`: on, every workspace, every agent.
        std::fs::write(
            path(dir.path()),
            r#"{ "version": 1, "next_id": 2, "instructions": [
                { "id": 1, "name": "A", "text": "a" },
                { "id": 2, "name": "B", "text": "b", "on": false, "workspace": "w", "agents": ["claude"] } ] }"#,
        )
        .unwrap();
        let Loaded::Saved(mut loaded) = load(dir.path()) else {
            panic!("loaded");
        };
        let a = loaded.get(1).unwrap();
        assert_eq!((a.on, &a.workspace, &a.agents), (true, &None, &None));
        let b = loaded.get(2).unwrap();
        assert_eq!(
            (b.on, b.workspace.as_deref(), b.agents.as_deref()),
            (false, Some("w"), Some(&["claude".to_string()][..]))
        );
        save(dir.path(), &loaded, 1).unwrap();
        assert_eq!(load(dir.path()), Loaded::Saved(loaded.clone()));
        // Ids are never reused after a remove, not even the last one's.
        loaded.remove(2);
        let c = loaded.add(draft("C", "c", None));
        assert_eq!(c, 3);
        loaded.remove(c);
        assert_eq!(loaded.add(draft("D", "d", None)), 4);
    }

    #[test]
    fn every_workspace_comes_first_then_the_list() {
        let mut all = Instructions::default();
        let a = all.add(draft("A", "a", Some("W")));
        let b = all.add(draft("B", "b", None));
        let c = all.add(draft("C", "c", Some("W")));
        let d = all.add(draft("D", "d", None));
        all.set_on(d, false);
        all.add(draft("E", "  \n", None));
        let ids = |workspace: &str| -> Vec<u64> {
            all.applying(workspace, |_| true)
                .iter()
                .map(|i| i.id)
                .collect()
        };
        assert_eq!(ids("W"), [b, a, c]);
        assert_eq!(ids("X"), [b]);
    }

    #[test]
    fn chosen_agents_are_asked_and_an_empty_choice_is_for_no_one() {
        let mut all = Instructions::default();
        let mut chosen = |agents: Option<Vec<&str>>| {
            all.add(Draft {
                agents: agents.map(|a| a.into_iter().map(str::to_string).collect()),
                ..draft("x", "x", None)
            })
        };
        let (every, codex, none) = (
            chosen(None),
            chosen(Some(vec!["codex"])),
            chosen(Some(vec![])),
        );
        let ids = |agent: &str| -> Vec<u64> {
            all.applying("W", |id| id == agent)
                .iter()
                .map(|i| i.id)
                .collect()
        };
        assert_eq!(ids("claude"), [every]);
        assert_eq!(ids("codex"), [every, codex]);
        assert!(!ids("codex").contains(&none));
    }

    #[test]
    fn editing_one_keeps_its_place_and_its_switch() {
        let mut all = Instructions::default();
        let first = all.add(draft("A", "a", None));
        let second = all.add(draft("B", "b", None));
        all.set_on(first, false);
        all.set(first, draft("A2", "a2", Some("W")));
        let a = all.get(first).unwrap();
        assert_eq!(
            (a.name.as_str(), a.on, a.workspace.as_deref()),
            ("A2", false, Some("W"))
        );
        assert_eq!(all.instructions[1].id, second);
    }
}
