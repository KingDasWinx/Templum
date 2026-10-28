//! Automations, the person's cron (docs/specs/automations.md): rules that run at a time and do
//! one thing. This is their pure layer: the saved rules and their file here, the times in
//! `clock`. They run in `workspace/automations.rs` and are drawn by `automations_view`.

use chrono::{NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

pub(crate) mod clock;

/// When an automation runs: a time, on the computer's clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum When {
    Once {
        at: NaiveDateTime,
    },
    /// 1 ..= `clock::EVERY_MOST`, real time from the rule's `since`.
    Every {
        minutes: u32,
    },
    Daily {
        at: NaiveTime,
    },
    /// 1 Monday ..= 7 Sunday.
    Weekly {
        days: Vec<u8>,
        at: NaiveTime,
    },
    /// 1 ..= 31.
    Monthly {
        days: Vec<u8>,
        at: NaiveTime,
    },
    Cron {
        expr: String,
    },
    /// An agent finishes its turn or needs the person.
    Agent {
        event: Heard,
        who: Who,
    },
    /// A command ends in the terminal `terminal` (a uid); `failures`: only with an exit code
    /// other than 0.
    Command {
        terminal: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        failures: bool,
    },
}

/// What an agent does that an event hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Heard {
    /// Its turn ended.
    Idle,
    /// It began to ask for the person.
    NeedsYou,
}

/// Whose events an agent event hears.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Who {
    /// Chosen blocks (uids): any of them, or (`all`) the last of them once every one is idle.
    Blocks {
        blocks: Vec<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Any agent of a kind: an id of Settings ▸ Agents.
    Kind { kind: String },
}

impl When {
    /// An event, not a time.
    pub(crate) fn is_event(&self) -> bool {
        matches!(self, When::Agent { .. } | When::Command { .. })
    }
}

/// The minutes between two runs of an event automation, when it says none.
pub(crate) const COOLDOWN: u32 = 5;
/// The most minutes a cooldown may be.
pub(crate) const COOLDOWN_MOST: u32 = 1440;
/// The block a prompt names for "the agent that fired it": a word no block id can be.
pub(crate) const FIRED: &str = "fired-it";

/// An event as the list and the preview say it: "When Claude Code (abc12) finishes its turn".
/// `block` names a block ("Claude Code (abc12)"), `kind` an agent of Settings ▸ Agents; `space`
/// is the workspace a kind's events come from. None for a time.
pub(crate) fn event_words(
    when: &When,
    block: &dyn Fn(&str) -> String,
    kind: &dyn Fn(&str) -> String,
    space: Option<&str>,
) -> Option<String> {
    Some(match when {
        When::Agent { event, who } => {
            // All of them is for a turn's end only: several that need you fire on any of them.
            let all = *event == Heard::Idle
                && matches!(who, Who::Blocks { blocks, all: true } if blocks.len() > 1);
            let (names, subject) = match who {
                Who::Blocks { blocks, .. } => {
                    let names: Vec<String> = blocks.iter().map(|b| block(b)).collect();
                    let joiner = if all { "and" } else { "or" };
                    (names.len(), clock::list(&names, joiner))
                }
                Who::Kind { kind: k } => {
                    let space = space.map_or(String::new(), |s| format!(" in {s}"));
                    (1, format!("any {}{space}", kind(k)))
                }
            };
            let subject = if names == 0 {
                "an agent".into()
            } else {
                subject
            };
            match (event, all) {
                (Heard::Idle, true) => format!("When {subject} have all finished their turns"),
                (Heard::Idle, false) => format!("When {subject} finishes its turn"),
                (Heard::NeedsYou, _) => format!("When {subject} needs you"),
            }
        }
        When::Command { terminal, failures } => {
            let verb = if *failures { "fails" } else { "ends" };
            format!("When a command {verb} in {}", block(terminal))
        }
        _ => return None,
    })
}

/// An event as a row's right side says it: "on abc12's turn's end", "when a Codex needs you".
/// `kind` names an agent of Settings ▸ Agents. None for a time.
pub(crate) fn event_short(when: &When, kind: &dyn Fn(&str) -> String) -> Option<String> {
    Some(match when {
        When::Agent { event, who } => match (event, who) {
            (Heard::Idle, Who::Blocks { blocks, all }) => match blocks.as_slice() {
                [one] => format!("on {one}'s turn's end"),
                many if *all => format!("once all {} agents finish", many.len()),
                many => format!("on a turn's end of {} agents", many.len()),
            },
            (Heard::Idle, Who::Kind { kind: k }) => format!("on a {}'s turn's end", kind(k)),
            (Heard::NeedsYou, Who::Blocks { blocks, .. }) => match blocks.as_slice() {
                [one] => format!("when {one} needs you"),
                many => format!("when one of {} agents needs you", many.len()),
            },
            (Heard::NeedsYou, Who::Kind { kind: k }) => format!("when a {} needs you", kind(k)),
        },
        When::Command {
            terminal,
            failures: true,
        } => format!("on a failure in {terminal}"),
        When::Command { terminal, .. } => format!("on a command's end in {terminal}"),
        _ => return None,
    })
}

fn cooldown() -> u32 {
    COOLDOWN
}

fn is_cooldown(minutes: &u32) -> bool {
    *minutes == COOLDOWN
}

use std::io;
use std::path::{Path, PathBuf};

use crate::session::{self, Loaded};
use crate::workspace::remote_rules::HOME_KEYS;

/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;
/// The most characters of a name.
pub(crate) const NAME_MAX: usize = 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Automations {
    pub version: u32,
    /// The last id given out: never reused.
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub automations: Vec<Automation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Automation {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    /// Off keeps it and runs nothing.
    #[serde(default = "on")]
    pub on: bool,
    /// The id of the workspace it is for; None: every workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub when: When,
    #[serde(rename = "do")]
    pub act: Do,
    /// Stops (turns off) after this many runs that went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs: Option<u32>,
    #[serde(default)]
    pub runs: u32,
    /// When its When started counting: Unix seconds.
    #[serde(default)]
    pub since: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastRun>,
    /// The uid of the terminal its command last opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened: Option<String>,
    /// Minutes between two runs of an event automation, per agent that fired it.
    #[serde(default = "cooldown", skip_serializing_if = "is_cooldown")]
    pub cooldown: u32,
}

fn on() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Do {
    /// Type `text` into the agent in the block `block` (a uid).
    Prompt {
        block: String,
        text: String,
    },
    /// Run `text` in the terminal `terminal` (a uid), or in a new one in `folder`.
    Command {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        text: String,
    },
    /// Open the agent `agent` (an id of Settings ▸ Agents).
    Agent {
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    Notify {
        text: String,
    },
}

/// How an automation's last run went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LastRun {
    /// Unix seconds.
    pub at: i64,
    pub went: Went,
    pub about: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Went {
    Ran,
    Skipped,
    Failed,
    /// Its time came while Templum was closed.
    Missed,
}

/// The name of a copy of the rule named `name`: "<name> (copy)", cut first so it is no longer than a
/// name may be; an empty name stays empty.
pub(crate) fn copy_name(name: &str) -> String {
    const COPY: &str = " (copy)";
    if name.is_empty() {
        return String::new();
    }
    let kept: String = name.chars().take(NAME_MAX - COPY.len()).collect();
    format!("{kept}{COPY}")
}

impl Default for Automations {
    fn default() -> Self {
        Automations {
            version: VERSION,
            next_id: 0,
            automations: Vec::new(),
        }
    }
}

impl Automations {
    /// Gives `a` the next id and puts it after the others; returns the id.
    pub fn add(&mut self, mut a: Automation) -> u64 {
        self.next_id += 1;
        a.id = self.next_id;
        self.automations.push(a);
        self.next_id
    }

    pub fn get(&self, id: u64) -> Option<&Automation> {
        self.automations.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Automation> {
        self.automations.iter_mut().find(|a| a.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.automations.retain(|a| a.id != id);
    }

    /// A copy after the last: "<name> (copy)" (an empty name stays empty), off, with no runs, last
    /// run nor kept terminal. Returns its id.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let mut copy = self.get(id)?.clone();
        copy.name = copy_name(&copy.name);
        (copy.on, copy.runs, copy.last, copy.opened) = (false, 0, None, None);
        Some(self.add(copy))
    }
}

pub(crate) fn path(dir: &Path) -> PathBuf {
    dir.join("automations.json")
}

pub(crate) fn load(dir: &Path) -> Loaded<Automations> {
    match session::read::<Automations>(&path(dir)) {
        Loaded::Saved(mut saved) => {
            // Ids from HOME_KEYS up are the popup's keys of another computer's rules: an edited
            // file's are renumbered like a repeated one.
            for a in &mut saved.automations {
                if a.id >= HOME_KEYS {
                    a.id = 0;
                }
            }
            if saved.next_id >= HOME_KEYS {
                saved.next_id = saved.automations.iter().map(|a| a.id).max().unwrap_or(0);
            }
            let ids = saved.automations.iter_mut().map(|a| &mut a.id);
            session::unique_ids(ids, &mut saved.next_id);
            Loaded::Saved(saved)
        }
        other => other,
    }
}

/// Writes the automations; like canvases, a write older than the last one done is dropped.
pub(crate) fn save(dir: &Path, automations: &Automations, generation: u64) -> io::Result<()> {
    let text = serde_json::to_string_pretty(automations).map_err(io::Error::other)?;
    session::write_newest(&path(dir), &text, generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example of docs/specs/automations.md#where-they-are-saved.
    const EXAMPLE: &str = r#"{ "version": 1, "next_id": 2, "automations": [
        { "id": 1, "name": "Morning review", "on": true, "workspace": "k2v8q",
          "when": { "weekly": { "days": [1, 2, 3, 4, 5], "at": "09:00:00" } },
          "do": { "prompt": { "block": "abc12", "text": "Review the open pull requests." } },
          "runs": 3, "since": 1761300000,
          "last": { "at": 1761555600, "went": "ran", "about": "Typed into abc12 as task t4." } },
        { "id": 2, "name": "", "on": true,
          "when": { "every": { "minutes": 10 } },
          "do": { "command": { "folder": "~/api", "text": "curl -fsS localhost:8080/health" } },
          "max_runs": 6, "runs": 0, "since": 1761558000, "opened": "q3x9z" } ] }"#;

    fn notify(name: &str) -> Automation {
        Automation {
            id: 0,
            name: name.into(),
            on: true,
            workspace: None,
            when: When::Daily {
                at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            },
            act: Do::Notify { text: "Hi".into() },
            max_runs: None,
            runs: 0,
            since: 0,
            last: None,
            opened: None,
            cooldown: COOLDOWN,
        }
    }

    #[test]
    fn the_specs_example_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), EXAMPLE).unwrap();
        let Loaded::Saved(loaded) = load(dir.path()) else {
            panic!("the example loads");
        };
        let (first, second) = (&loaded.automations[0], &loaded.automations[1]);
        let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        assert_eq!(
            first.when,
            When::Weekly {
                days: vec![1, 2, 3, 4, 5],
                at: nine
            }
        );
        assert_eq!(
            first.act,
            Do::Prompt {
                block: "abc12".into(),
                text: "Review the open pull requests.".into()
            }
        );
        assert_eq!(first.last.as_ref().unwrap().went, Went::Ran);
        assert_eq!(second.max_runs, Some(6));
        assert_eq!(second.opened.as_deref(), Some("q3x9z"));
        assert_eq!(
            second.act,
            Do::Command {
                terminal: None,
                folder: Some("~/api".into()),
                text: "curl -fsS localhost:8080/health".into()
            }
        );
        save(dir.path(), &loaded, 1).unwrap();
        let Loaded::Saved(again) = load(dir.path()) else {
            panic!("the saved file loads");
        };
        assert_eq!(again, loaded);
    }

    const EVENTS: &str = r#"{ "version": 1, "next_id": 3, "automations": [
      { "id": 1, "when": { "agent": { "event": "idle", "who": { "blocks": ["abc12", "k9x1d"], "all": true } } },
        "do": { "prompt": { "block": "fired-it", "text": "Go on." } }, "cooldown": 10 },
      { "id": 2, "when": { "agent": { "event": "needs_you", "who": { "kind": "codex" } } },
        "do": { "notify": { "text": "Look" } } },
      { "id": 3, "when": { "command": { "terminal": "q3x9z", "failures": true } },
        "do": { "notify": { "text": "Broke" } } } ] }"#;

    #[test]
    fn an_event_rule_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), EVENTS).unwrap();
        let Loaded::Saved(loaded) = load(dir.path()) else {
            panic!("the events load");
        };
        let [first, second, third] = &loaded.automations[..] else {
            panic!("three rules");
        };
        assert_eq!(
            first.when,
            When::Agent {
                event: Heard::Idle,
                who: Who::Blocks {
                    blocks: vec!["abc12".into(), "k9x1d".into()],
                    all: true
                }
            }
        );
        assert_eq!(
            (first.cooldown, &first.act),
            (
                10,
                &Do::Prompt {
                    block: FIRED.into(),
                    text: "Go on.".into()
                }
            )
        );
        assert_eq!(
            (second.when.clone(), second.cooldown),
            (
                When::Agent {
                    event: Heard::NeedsYou,
                    who: Who::Kind {
                        kind: "codex".into()
                    }
                },
                COOLDOWN
            )
        );
        assert_eq!(
            third.when,
            When::Command {
                terminal: "q3x9z".into(),
                failures: true
            }
        );
        save(dir.path(), &loaded, 1).unwrap();
        let text = std::fs::read_to_string(path(dir.path())).unwrap();
        assert_eq!(text.matches("\"cooldown\": 10").count(), 1, "{text}");
        assert_eq!(
            text.matches("cooldown").count(),
            1,
            "only the odd one: {text}"
        );
        let mut plain = loaded.clone();
        plain.automations[0].when = When::Agent {
            event: Heard::Idle,
            who: Who::Blocks {
                blocks: vec!["abc12".into()],
                all: false,
            },
        };
        plain.automations[2].when = When::Command {
            terminal: "q3x9z".into(),
            failures: false,
        };
        save(dir.path(), &plain, 2).unwrap();
        let text = std::fs::read_to_string(path(dir.path())).unwrap();
        assert!(
            !text.contains("\"all\"") && !text.contains("failures"),
            "{text}"
        );
        let Loaded::Saved(again) = load(dir.path()) else {
            panic!("the saved file loads");
        };
        assert_eq!(again, plain);
    }

    fn block_name(uid: &str) -> String {
        let title = match uid {
            "abc12" => "Claude Code",
            "k9x1d" => "Codex",
            _ => "zsh",
        };
        format!("{title} ({uid})")
    }

    fn kind_name(kind: &str) -> String {
        match kind {
            "claude" => "Claude Code".into(),
            "codex" => "Codex".into(),
            other => other.into(),
        }
    }

    fn agent(event: Heard, blocks: &[&str], all: bool) -> When {
        let blocks = blocks.iter().map(|b| b.to_string()).collect();
        When::Agent {
            event,
            who: Who::Blocks { blocks, all },
        }
    }

    fn kind_of(event: Heard, kind: &str) -> When {
        When::Agent {
            event,
            who: Who::Kind { kind: kind.into() },
        }
    }

    #[test]
    fn an_events_words() {
        let say = |when: &When, space| event_words(when, &block_name, &kind_name, space);
        let idle = Heard::Idle;
        let cases = [
            (
                agent(idle, &["abc12"], false),
                None,
                "When Claude Code (abc12) finishes its turn",
            ),
            (
                agent(idle, &["abc12"], true),
                None,
                "When Claude Code (abc12) finishes its turn",
            ),
            (
                kind_of(Heard::NeedsYou, "codex"),
                Some("api"),
                "When any Codex in api needs you",
            ),
            (
                kind_of(idle, "claude"),
                None,
                "When any Claude Code finishes its turn",
            ),
            (
                agent(idle, &["abc12", "k9x1d"], false),
                None,
                "When Claude Code (abc12) or Codex (k9x1d) finishes its turn",
            ),
            (
                agent(idle, &["abc12", "k9x1d", "q3x9z"], false),
                None,
                "When Claude Code (abc12), Codex (k9x1d) or zsh (q3x9z) finishes its turn",
            ),
            (
                agent(idle, &["abc12", "k9x1d"], true),
                None,
                "When Claude Code (abc12) and Codex (k9x1d) have all finished their turns",
            ),
            (
                agent(Heard::NeedsYou, &["abc12", "k9x1d"], true),
                None,
                "When Claude Code (abc12) or Codex (k9x1d) needs you",
            ),
            (
                When::Command {
                    terminal: "q3x9z".into(),
                    failures: true,
                },
                None,
                "When a command fails in zsh (q3x9z)",
            ),
            (
                When::Command {
                    terminal: "q3x9z".into(),
                    failures: false,
                },
                None,
                "When a command ends in zsh (q3x9z)",
            ),
        ];
        for (when, space, words) in cases {
            assert_eq!(say(&when, space).as_deref(), Some(words));
        }
        assert_eq!(say(&When::Every { minutes: 10 }, None), None);
    }

    #[test]
    fn an_events_short_words() {
        let say = |when: &When| event_short(when, &kind_name);
        let (idle, needs) = (Heard::Idle, Heard::NeedsYou);
        let cases = [
            (agent(idle, &["abc12"], false), "on abc12's turn's end"),
            (
                agent(idle, &["a", "b", "c"], false),
                "on a turn's end of 3 agents",
            ),
            (
                agent(idle, &["a", "b", "c"], true),
                "once all 3 agents finish",
            ),
            (kind_of(idle, "claude"), "on a Claude Code's turn's end"),
            (agent(needs, &["abc12"], false), "when abc12 needs you"),
            (
                agent(needs, &["a", "b"], false),
                "when one of 2 agents needs you",
            ),
            (kind_of(needs, "codex"), "when a Codex needs you"),
            (
                When::Command {
                    terminal: "q3x9z".into(),
                    failures: false,
                },
                "on a command's end in q3x9z",
            ),
            (
                When::Command {
                    terminal: "q3x9z".into(),
                    failures: true,
                },
                "on a failure in q3x9z",
            ),
        ];
        for (when, words) in cases {
            assert_eq!(say(&when).as_deref(), Some(words));
        }
        assert_eq!(say(&When::Every { minutes: 10 }), None);
    }

    #[test]
    fn ids_are_never_reused_and_a_copy_is_off() {
        let mut list = Automations::default();
        let first = list.add(notify("A"));
        let second = list.add(notify("B"));
        list.remove(second);
        let third = list.add(notify("C"));
        assert!(third > second && second > first);

        let a = list.get_mut(first).unwrap();
        a.runs = 3;
        a.opened = Some("q3x9z".into());
        a.last = Some(LastRun {
            at: 1,
            went: Went::Ran,
            about: "Notified.".into(),
        });
        let copy = list.duplicate(first).unwrap();
        let copy = list.get(copy).unwrap();
        assert_eq!(copy.name, "A (copy)");
        assert!(!copy.on);
        assert_eq!((copy.runs, &copy.last, &copy.opened), (0, &None, &None));
        assert_eq!(list.automations.last().unwrap().id, copy.id, "placed last");
        // An empty name stays empty.
        let unnamed = list.add(notify(""));
        let copy = list.duplicate(unnamed).unwrap();
        assert_eq!(list.get(copy).unwrap().name, "");
    }

    #[test]
    fn a_copy_of_a_long_name_is_no_longer_than_a_name_may_be() {
        let mut list = Automations::default();
        let id = list.add(notify(&"n".repeat(NAME_MAX)));
        let copy = list.duplicate(id).unwrap();
        let name = &list.get(copy).unwrap().name;
        assert_eq!(name.chars().count(), NAME_MAX);
        assert!(name.ends_with(" (copy)"));
    }

    #[test]
    fn a_repeated_or_missing_id_is_given_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = Automations::default();
        list.add(notify("A"));
        list.add(notify("B"));
        list.automations[1].id = 1;
        save(dir.path(), &list, 1).unwrap();
        let Loaded::Saved(loaded) = load(dir.path()) else {
            panic!("it loads");
        };
        let ids: Vec<u64> = loaded.automations.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn an_id_in_the_range_of_home_s_keys_is_given_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = Automations::default();
        list.add(notify("A"));
        list.add(notify("B"));
        list.automations[1].id = HOME_KEYS | 5;
        list.next_id = HOME_KEYS | 5;
        save(dir.path(), &list, 1).unwrap();
        let Loaded::Saved(loaded) = load(dir.path()) else {
            panic!("it loads");
        };
        let ids: Vec<u64> = loaded.automations.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 2]);
        assert!(loaded.next_id < HOME_KEYS);
    }
}
