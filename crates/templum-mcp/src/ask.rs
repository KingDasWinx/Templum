//! `ask_user`'s questions (docs/specs/ask-user.md): parsed from either shape, and the answer's
//! text. No UI.

use serde_json::{Map, Value, json};

use crate::tools::Reply;

/// The most characters of a header shown; a longer one is cut with "…".
pub const HEADER_MAX: usize = 12;
/// The most characters of the question a notice names.
const NOTICE_MAX: usize = 60;

/// An `ask_user` call's questions, or `propose_plan`'s plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub questions: Vec<Question>,
    /// Asked with `questions`: the answer is JSON. Today's `question` gets plain text.
    pub json: bool,
    /// `propose_plan`'s: its one question is Approve or Reject, with a comment.
    pub plan: Option<Plan>,
    /// Said with `why` (trimmed): what the agent is doing and what the answer decides.
    pub why: Option<String>,
}

/// A plan the person approves or rejects (docs/specs/agent-tools.md, propose_plan).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub title: Option<String>,
    /// Markdown.
    pub text: String,
}

/// The longest plan `propose_plan` takes, in bytes.
pub const PLAN_MAX: usize = 256 * 1024;

/// `propose_plan`'s dialog: one single-select question, Approve or Reject, and the comment.
pub fn plan(title: Option<String>, text: String) -> Ask {
    Ask {
        questions: vec![Question {
            text: "Approve the plan?".into(),
            header: None,
            options: vec![label("Approve"), label("Reject")],
            multi: false,
        }],
        json: false,
        plan: Some(Plan { title, text }),
        why: None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub text: String,
    pub header: Option<String>,
    pub options: Vec<Choice>,
    pub multi: bool,
}

/// One of a question's options.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub description: Option<String>,
    pub preview: Option<String>,
}

/// `ask_user`'s arguments, in either shape; Err names the place that is wrong.
pub fn parse(args: &Value) -> Result<Ask, String> {
    let why = match &args["why"] {
        Value::Null => None,
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::String(_) => {
            return Err("why is blank: say why you ask, or leave it out.".into());
        }
        _ => return Err("\"why\" is a text".into()),
    };
    let ask = match (&args["question"], &args["questions"]) {
        (Value::Null, Value::Null) => {
            Err("ask_user needs \"questions\" (or \"question\", a string)".into())
        }
        (Value::Null, questions) => questions_of(questions),
        (question, Value::Null) => {
            let text = question
                .as_str()
                .ok_or("ask_user needs \"question\", a string")?;
            let options = match &args["options"] {
                Value::Null => Vec::new(),
                Value::Array(items) => items
                    .iter()
                    .map(|o| o.as_str().map(label))
                    .collect::<Option<_>>()
                    .ok_or("\"options\" is a list of strings")?,
                _ => return Err("\"options\" is a list of strings".into()),
            };
            let question = Question {
                text: text.to_string(),
                header: None,
                options,
                multi: false,
            };
            Ok(Ask {
                questions: vec![question],
                json: false,
                plan: None,
                why: None,
            })
        }
        _ => Err("Give question or questions, not both.".into()),
    }?;
    Ok(Ask { why, ..ask })
}

fn questions_of(value: &Value) -> Result<Ask, String> {
    let items = value
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("\"questions\" is a list of one or more questions")?;
    let mut questions: Vec<Question> = Vec::new();
    for (q, item) in items.iter().enumerate() {
        let place = format!("questions[{q}]");
        let text = item["question"]
            .as_str()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| format!("{place} needs \"question\", a text that isn't blank"))?;
        if let Some(before) = questions.iter().position(|o| o.text == text) {
            return Err(format!(
                "questions[{before}] and {place} have the same text: the answers are keyed by it."
            ));
        }
        let options = match &item["options"] {
            Value::Null => Vec::new(),
            Value::Array(options) => {
                let mut choices: Vec<Choice> = Vec::new();
                for (i, option) in options.iter().enumerate() {
                    let at = format!("{place}.options[{i}]");
                    let choice = choice(option, &at)?;
                    if let Some(before) = choices
                        .iter()
                        .position(|c| c.label.trim() == choice.label.trim())
                    {
                        return Err(format!(
                            "{at} has the same label as {place}.options[{before}]."
                        ));
                    }
                    choices.push(choice);
                }
                choices
            }
            _ => return Err(format!("{place}.options is a list of options")),
        };
        let multi = match &item["multiSelect"] {
            Value::Null => false,
            value => value
                .as_bool()
                .ok_or_else(|| format!("{place}.multiSelect is true or false"))?,
        };
        questions.push(Question {
            text: text.to_string(),
            header: text_at(&item["header"], &format!("{place}.header"))?,
            options,
            multi,
        });
    }
    Ok(Ask {
        questions,
        json: true,
        plan: None,
        why: None,
    })
}

/// An option: a string, its label, or `{ "label", "description", "preview" }`.
fn choice(option: &Value, at: &str) -> Result<Choice, String> {
    if let Some(text) = option.as_str() {
        return (!text.trim().is_empty())
            .then(|| label(text))
            .ok_or_else(|| format!("{at} needs a label"));
    }
    let text = option["label"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| format!("{at} needs a label"))?;
    Ok(Choice {
        label: text.to_string(),
        description: text_at(&option["description"], &format!("{at}.description"))?,
        preview: text_at(&option["preview"], &format!("{at}.preview"))?,
    })
}

fn label(text: &str) -> Choice {
    Choice {
        label: text.to_string(),
        description: None,
        preview: None,
    }
}

/// An optional text: a blank one is none.
fn text_at(value: &Value, at: &str) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone()).filter(|s| !s.trim().is_empty())),
        _ => Err(format!("{at} is a text")),
    }
}

/// `text`, at most `max` characters: a longer one is cut, "…" last.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

impl Ask {
    /// The first question's first line, or a plan's title: the dot's tooltip, the notification's
    /// text.
    pub fn summary(&self) -> String {
        if let Some(plan) = &self.plan {
            return plan.title.clone().unwrap_or_else(|| "A plan".into());
        }
        let text = self.questions.first().map_or("", |q| q.text.as_str());
        text.lines().next().unwrap_or_default().to_string()
    }

    /// A plan's Approve and Reject answer on a click: they are the decision itself. Every other
    /// option only picks; Send answers.
    pub fn quick(&self) -> bool {
        self.plan.is_some()
    }

    /// Asked with `questions`, more than one: the dialog shows one at a time, under a tab each.
    pub fn tabs(&self) -> bool {
        self.json && self.questions.len() > 1
    }

    /// Question `q`'s tab: its header as its chip shows it, or "Question 2" without one.
    pub fn tab(&self, q: usize) -> String {
        self.questions[q]
            .header_shown()
            .unwrap_or_else(|| format!("Question {}", q + 1))
    }

    /// The result's text: `answers[q]` holds the labels picked in question `q`, in option order,
    /// then Other's text. JSON for `questions`, with `note` when written; else the answer alone.
    pub fn answer(&self, answers: &[Vec<String>], note: &str) -> String {
        if !self.json {
            return answers[0].concat();
        }
        let answered: Map<String, Value> = self
            .questions
            .iter()
            .zip(answers)
            .map(|(q, picked)| {
                let value = if q.multi {
                    json!(picked)
                } else {
                    json!(picked.concat())
                };
                (q.text.clone(), value)
            })
            .collect();
        let mut answer = json!({ "answers": answered });
        if !note.trim().is_empty() {
            answer["note"] = json!(note.trim());
        }
        answer.to_string()
    }

    /// Whether `answers` (one list per question, as [`Ask::answer`] takes them) fit these
    /// questions: what the inbox's Send can't send. Err says what to fix. The API checks an answer
    /// with it, since its caller isn't a form.
    pub fn checked(&self, answers: &[Vec<String>]) -> Result<(), String> {
        if answers.len() != self.questions.len() {
            return Err(format!(
                "Answer the {} questions: answers has {}.",
                self.questions.len(),
                answers.len()
            ));
        }
        for (i, (question, picked)) in self.questions.iter().zip(answers).enumerate() {
            let n = i + 1;
            if picked.iter().any(|a| a.trim().is_empty()) {
                return Err("An answer is blank.".into());
            }
            if question.multi && picked.is_empty() {
                return Err(format!("Question {n} needs an answer."));
            }
            if !question.multi && picked.len() != 1 {
                return Err(format!("Question {n} takes one answer."));
            }
            let others: Vec<&String> = picked
                .iter()
                .filter(|a| !question.options.iter().any(|o| &o.label == *a))
                .collect();
            if self.plan.is_some() && !others.is_empty() {
                return Err("A plan is Approve or Reject.".into());
            }
            if others.len() > 1 {
                return Err(format!(
                    "Question {n} takes one other answer at most: \"{}\" and \"{}\" aren't its options.",
                    others[0], others[1]
                ));
            }
        }
        Ok(())
    }

    /// The tool's result: the answer as `answer` gives it; for a plan, `Approved.` or the error
    /// `Rejected.`, with the person's comment when they wrote one.
    pub fn reply(&self, answers: &[Vec<String>], note: &str) -> Reply {
        if self.plan.is_none() {
            return Reply::ok(self.answer(answers, note));
        }
        let comment = match note.trim() {
            "" => String::new(),
            note => format!("\nThe person's comment: {note}"),
        };
        match answers.first().and_then(|a| a.first()).map(String::as_str) {
            Some("Approve") => Reply::ok(format!("Approved.{comment}")),
            _ => Reply::err(format!("Rejected.{comment}")),
        }
    }

    /// Question `q`'s preview: None when no option of it has one; else the highlighted option's,
    /// or the picked one's, or the first one with a preview; `Some(None)`: "No preview".
    pub fn preview_of(&self, q: usize, form: &Form) -> Option<Option<&str>> {
        let options = &self.questions[q].options;
        let first = options.iter().find_map(|c| c.preview.as_deref())?;
        let at = match form.highlight {
            Some(Row::Option(on, i)) if on == q => Some(i),
            _ => (0..options.len()).find(|i| form.picked(q, *i)),
        };
        Some(match at {
            Some(i) => options[i].preview.as_deref(),
            None => Some(first),
        })
    }

    /// What is typed into the agent's terminal when its client stopped waiting for `answer`: a
    /// line naming the question, then the answer on the next (a long one is a paste the first
    /// line comes with, as delegate's notices).
    pub fn notice(&self, answer: &str) -> String {
        let what = if self.plan.is_some() {
            "plan"
        } else {
            "question"
        };
        format!(
            "[answer to your {what} \"{}\"] The answer is in the pasted text.\n{answer}",
            cut(&self.summary(), NOTICE_MAX)
        )
    }
}

impl Question {
    /// The header as its chip shows it: HEADER_MAX characters at most.
    pub fn header_shown(&self) -> Option<String> {
        self.header.as_deref().map(|h| cut(h, HEADER_MAX))
    }
}

/// An agent's preview as clean text: its escape sequences dropped, `\r\n` and `\r` as new lines,
/// tabs as 4 spaces, other control characters dropped.
pub fn clean_preview(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: parameters, then a final byte.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ESC \.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                // A two-character escape.
                _ => {}
            },
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\n' => clean.push('\n'),
            '\t' => clean.push_str("    "),
            c if c.is_control() => {}
            c => clean.push(c),
        }
    }
    clean
}

/// A row of the dialog the highlight can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Question `.0`'s option `.1`.
    Option(usize, usize),
    /// Question `.0`'s Other field (a question without options: its field).
    Other(usize),
    Note,
    Send,
}

/// What the person picked in a dialog, the row highlighted and the question shown. The Other
/// fields' and the note's texts live in the app's fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub highlight: Option<Row>,
    /// The question on screen, its tab lit (`Ask::tabs`); 0 for a call of one.
    pub tab: usize,
    picked: Vec<Vec<bool>>,
    other: Vec<bool>,
}

impl Form {
    /// Nothing picked, no row highlighted, the first question shown.
    pub fn new(ask: &Ask) -> Form {
        Form {
            highlight: None,
            tab: 0,
            picked: ask
                .questions
                .iter()
                .map(|q| vec![false; q.options.len()])
                .collect(),
            other: vec![false; ask.questions.len()],
        }
    }

    /// The dialog's rows in order: for `questions`, the shown question's options then its Other,
    /// then the note and Send; today's shape, its options and Send, or its field and Send.
    pub fn rows(&self, ask: &Ask) -> Vec<Row> {
        // A plan's: Comment, then Reject on the left and Approve on the right.
        if ask.plan.is_some() {
            return vec![Row::Note, Row::Option(0, 1), Row::Option(0, 0)];
        }
        if !ask.json {
            let options = ask.questions[0].options.len();
            let mut rows = match options {
                0 => vec![Row::Other(0)],
                n => (0..n).map(|i| Row::Option(0, i)).collect(),
            };
            rows.push(Row::Send);
            return rows;
        }
        let q = self.tab;
        let mut rows: Vec<Row> = (0..ask.questions[q].options.len())
            .map(|i| Row::Option(q, i))
            .collect();
        rows.extend([Row::Other(q), Row::Note, Row::Send]);
        rows
    }

    pub fn picked(&self, q: usize, i: usize) -> bool {
        self.picked[q][i]
    }

    pub fn other_picked(&self, q: usize) -> bool {
        self.other[q]
    }

    /// Picks option `i` of question `q`: alone in a single-select question (Other unpicked, its
    /// text kept), checked or unchecked in a multiSelect.
    pub fn pick(&mut self, ask: &Ask, q: usize, i: usize) {
        if ask.questions[q].multi {
            self.picked[q][i] = !self.picked[q][i];
        } else {
            self.picked[q].iter_mut().for_each(|p| *p = false);
            self.picked[q][i] = true;
            self.other[q] = false;
        }
    }

    /// Question `q`'s Other field holds `text`: picked unless it is blank, which in a
    /// single-select question unpicks its option.
    pub fn typed(&mut self, ask: &Ask, q: usize, text: &str) {
        self.other[q] = !text.trim().is_empty();
        if self.other[q] && !ask.questions[q].multi {
            self.picked[q].iter_mut().for_each(|p| *p = false);
        }
    }

    /// Moves the highlight `by` rows, wrapping; from none, onto the first row going down, the
    /// last going up.
    pub fn step(&mut self, ask: &Ask, by: isize) {
        let rows = self.rows(ask);
        let at = self
            .highlight
            .and_then(|row| rows.iter().position(|r| *r == row));
        let next = match at {
            Some(at) => (at as isize + by).rem_euclid(rows.len() as isize) as usize,
            None if by > 0 => 0,
            None => rows.len() - 1,
        };
        self.highlight = Some(rows[next]);
    }

    /// Each question's answer (the labels picked in option order, then Other's text trimmed), once
    /// every question has one. `others[q]`: question `q`'s Other text.
    pub fn answers(&self, ask: &Ask, others: &[String]) -> Option<Vec<Vec<String>>> {
        (0..ask.questions.len())
            .map(|q| Some(self.answer_of(ask, q, others)).filter(|a| !a.is_empty()))
            .collect()
    }

    /// How many questions have an answer so far.
    pub fn answered(&self, ask: &Ask, others: &[String]) -> usize {
        (0..ask.questions.len())
            .filter(|q| self.has_answer(ask, *q, others))
            .count()
    }

    /// Whether question `q` has an answer: its tab says so.
    pub fn has_answer(&self, ask: &Ask, q: usize, others: &[String]) -> bool {
        !self.answer_of(ask, q, others).is_empty()
    }

    fn answer_of(&self, ask: &Ask, q: usize, others: &[String]) -> Vec<String> {
        let mut answer: Vec<String> = ask.questions[q]
            .options
            .iter()
            .zip(&self.picked[q])
            .filter(|(_, picked)| **picked)
            .map(|(choice, _)| choice.label.clone())
            .collect();
        let other = others.get(q).map_or("", |t| t.trim());
        if self.other[q] && !other.is_empty() {
            answer.push(other.to_string());
        }
        answer
    }

    /// The next question without an answer, wrapping: after the shown one while the highlight is
    /// on one of its rows, else from it. None when every question has one.
    pub fn unanswered(&self, ask: &Ask, others: &[String]) -> Option<usize> {
        let n = ask.questions.len();
        let from = match self.highlight {
            Some(Row::Option(..) | Row::Other(_)) => self.tab + 1,
            _ => self.tab,
        };
        (from..from + n)
            .map(|q| q % n)
            .find(|q| !self.has_answer(ask, *q, others))
    }

    /// Shows the next question without an answer (`unanswered`) and highlights its first row;
    /// stays when every question has one.
    pub fn next_unanswered(&mut self, ask: &Ask, others: &[String]) {
        if let Some(q) = self.unanswered(ask, others) {
            self.show(ask, q);
            self.highlight = Some(first_row(ask, q));
        }
    }

    /// Shows question `q`: a highlight on another question's row goes to its first row; on the
    /// note or Send, or on none, it stays.
    pub fn show(&mut self, ask: &Ask, q: usize) {
        self.tab = q;
        if let Some(Row::Option(on, _) | Row::Other(on)) = self.highlight
            && on != q
        {
            self.highlight = Some(first_row(ask, q));
        }
    }

    /// Shows the question `by` tabs away from the shown one, wrapping.
    pub fn turn(&mut self, ask: &Ask, by: isize) {
        let n = ask.questions.len() as isize;
        self.show(ask, (self.tab as isize + by).rem_euclid(n) as usize);
    }
}

/// Question `q`'s first row: its first option, or its field when it has none.
fn first_row(ask: &Ask, q: usize) -> Row {
    if ask.questions[q].options.is_empty() {
        Row::Other(q)
    } else {
        Row::Option(q, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's example call.
    fn example() -> Value {
        json!({
            "questions": [
                {
                    "question": "Which database should the service use?",
                    "header": "Database",
                    "options": [
                        { "label": "Postgres", "description": "What production runs",
                          "preview": "DATABASE_URL=postgres://…\npool = 10" },
                        { "label": "SQLite", "description": "One file, no server" }
                    ],
                    "multiSelect": false
                },
                {
                    "question": "Which features go in the first release?",
                    "header": "Features",
                    "options": [{ "label": "Auth" }, { "label": "Search" }, { "label": "Export" }],
                    "multiSelect": true
                }
            ],
            "timeout": 600
        })
    }

    #[test]
    fn both_shapes_parse() {
        let today = parse(&json!({ "question": "Ship it?", "options": ["Yes", "No"] })).unwrap();
        assert!(!today.json);
        assert_eq!(today.questions.len(), 1);
        let q = &today.questions[0];
        assert_eq!(q.text, "Ship it?");
        assert!(!q.multi);
        assert_eq!(q.header, None);
        assert_eq!(
            q.options,
            ["Yes", "No"].map(|l| Choice {
                label: l.into(),
                description: None,
                preview: None
            })
        );
        let ask = parse(&example()).unwrap();
        assert!(ask.json);
        assert_eq!(ask.questions.len(), 2);
        assert!(!ask.questions[0].multi && ask.questions[1].multi);
        assert_eq!(ask.questions[0].header.as_deref(), Some("Database"));
        let first = &ask.questions[0].options[0];
        assert_eq!(first.description.as_deref(), Some("What production runs"));
        assert_eq!(
            first.preview.as_deref(),
            Some("DATABASE_URL=postgres://…\npool = 10")
        );
        assert_eq!(ask.questions[0].options[1].preview, None);
        assert_eq!(ask.questions[1].options.len(), 3);
    }

    #[test]
    fn options_may_be_strings_or_objects() {
        let ask = parse(&json!({ "questions": [
            { "question": "Which?", "options": ["A", { "label": "B", "description": " " }] }
        ] }))
        .unwrap();
        let options = &ask.questions[0].options;
        assert_eq!(options[0].label, "A");
        assert_eq!(options[1].label, "B");
        assert_eq!(options[1].description, None, "a blank description is none");
    }

    #[test]
    fn bad_questions_are_refused_by_place() {
        let q = |text: &str| json!({ "question": text, "options": ["A"] });
        let cases = [
            (json!({}), "question"),
            (
                json!({ "question": "Why?", "questions": [q("Why?")] }),
                "Give question or questions, not both.",
            ),
            (json!({ "questions": [] }), "questions"),
            (json!({ "questions": [q("Why?"), q("  ")] }), "questions[1]"),
            (
                json!({ "questions": [{ "question": "Why?", "options": [{ "description": "no label" }] }] }),
                "questions[0].options[0] needs a label",
            ),
            (
                json!({ "questions": [q("Why?"), q("Why?")] }),
                "questions[0] and questions[1] have the same text",
            ),
            (
                json!({ "questions": [{ "question": "Why?", "options": ["A", { "label": "A" }] }] }),
                "the same label",
            ),
            (
                json!({ "questions": [{ "question": "Why?", "multiSelect": "yes" }] }),
                "multiSelect",
            ),
        ];
        for (args, says) in cases {
            let err = parse(&args).unwrap_err();
            assert!(err.contains(says), "{args}: {err}");
        }
    }

    #[test]
    fn a_long_header_is_cut() {
        let header = |h: Option<&str>| Question {
            text: "Why?".into(),
            header: h.map(str::to_string),
            options: Vec::new(),
            multi: false,
        };
        let cut = header(Some("Infrastructure")).header_shown().unwrap();
        assert_eq!(cut, "Infrastruct…");
        assert_eq!(cut.chars().count(), HEADER_MAX);
        assert_eq!(
            header(Some("Database")).header_shown().as_deref(),
            Some("Database")
        );
        assert_eq!(header(None).header_shown(), None);
    }

    #[test]
    fn the_answer_is_json_for_questions() {
        let ask = parse(&example()).unwrap();
        let answers = vec![
            vec!["SQLite".to_string()],
            vec!["Auth".into(), "Export".into()],
        ];
        let answer: Value = serde_json::from_str(&ask.answer(&answers, " keep it small ")).unwrap();
        assert_eq!(
            answer,
            json!({
                "answers": {
                    "Which database should the service use?": "SQLite",
                    "Which features go in the first release?": ["Auth", "Export"],
                },
                "note": "keep it small",
            })
        );
        let one = vec![vec!["Postgres".to_string()], vec!["Search".to_string()]];
        let answer: Value = serde_json::from_str(&ask.answer(&one, "  ")).unwrap();
        assert_eq!(
            answer,
            json!({ "answers": {
                "Which database should the service use?": "Postgres",
                "Which features go in the first release?": ["Search"],
            } }),
            "no note when blank; a multiSelect with one pick is still a list"
        );
    }

    #[test]
    fn why_is_kept_trimmed() {
        let ask = parse(
            &json!({ "question": "Ship it?", "options": ["Yes"], "why": "  Releasing 1.2. " }),
        )
        .unwrap();
        assert_eq!(ask.why, Some("Releasing 1.2.".into()));
        let ask =
            parse(&json!({ "questions": [{ "question": "Ship it?" }], "why": "Now" })).unwrap();
        assert_eq!(ask.why, Some("Now".into()), "in either shape");
        assert_eq!(parse(&json!({ "question": "Ship it?" })).unwrap().why, None);
    }

    #[test]
    fn a_blank_why_is_refused() {
        let err = parse(&json!({ "question": "Ship it?", "why": "  " })).unwrap_err();
        assert!(err.contains("why"), "{err}");
        let err = parse(&json!({ "question": "Ship it?", "why": 3 })).unwrap_err();
        assert!(err.contains("why"), "{err}");
    }

    #[test]
    fn the_answer_is_text_for_question() {
        let ask = parse(&json!({ "question": "Ship it?", "options": ["Yes", "No"] })).unwrap();
        assert_eq!(ask.answer(&[vec!["Yes".into()]], ""), "Yes");
        assert_eq!(ask.answer(&[vec!["Yes".into()]], "a note"), "Yes");
        let when = parse(&json!({ "question": "When?" })).unwrap();
        assert_eq!(when.answer(&[vec!["Friday".into()]], ""), "Friday");
    }

    /// A plan is one implicit question, Approve or Reject, with a comment: approved is a result,
    /// rejected an error; its notice names the plan (docs/specs/agent-tools.md, propose_plan).
    #[test]
    fn a_plans_form_answers_approved_or_rejected_with_the_comment() {
        let ask = plan(Some("Move the bus".into()), "## Steps\n1. a".into());
        assert_eq!(
            Form::new(&ask).rows(&ask),
            [Row::Note, Row::Option(0, 1), Row::Option(0, 0)],
            "Comment, Reject, Approve"
        );
        let mut form = Form::new(&ask);
        assert_eq!(form.answers(&ask, &[]), None);
        form.pick(&ask, 0, 0);
        let answers = form.answers(&ask, &[]).unwrap();
        assert_eq!(ask.reply(&answers, " "), Reply::ok("Approved."));
        form.pick(&ask, 0, 1);
        let answers = form.answers(&ask, &[]).unwrap();
        assert_eq!(
            ask.reply(&answers, "do b first"),
            Reply::err("Rejected.\nThe person's comment: do b first")
        );
        assert_eq!(
            ask.notice("Rejected."),
            "[answer to your plan \"Move the bus\"] The answer is in the pasted text.\nRejected."
        );
        assert_eq!(ask.summary(), "Move the bus");
        let untitled = plan(None, "x".into());
        assert!(
            untitled
                .notice("Approved.")
                .starts_with("[answer to your plan \"A plan\"]")
        );
        // A question's reply is its answer, as before.
        let one = parse(&json!({ "question": "Why?" })).unwrap();
        assert_eq!(
            one.reply(&[vec!["Because".into()]], ""),
            Reply::ok("Because")
        );
    }

    #[test]
    fn the_notice_names_the_question() {
        let ask = parse(&example()).unwrap();
        assert_eq!(ask.summary(), "Which database should the service use?");
        let notice = ask.notice("SQLite");
        assert!(
            notice.starts_with("[answer to your question \"Which database"),
            "{notice}"
        );
        // The answer on a line of its own: Claude Code shows a long paste as "[Pasted text]",
        // which it takes for data unless its first line comes with it.
        assert!(
            notice.ends_with("\"] The answer is in the pasted text.\nSQLite"),
            "{notice}"
        );
        let long = "x".repeat(80);
        let ask = parse(&json!({ "question": format!("{long}\nsecond line") })).unwrap();
        assert_eq!(ask.summary(), long);
        assert_eq!(
            ask.notice("Yes"),
            format!(
                "[answer to your question \"{}…\"] The answer is in the pasted text.\nYes",
                "x".repeat(59)
            )
        );
    }

    /// A `questions` call of `n` single-select questions ("Q0", …) of two options each.
    fn singles(n: usize) -> Ask {
        let questions: Vec<Value> = (0..n)
            .map(|q| json!({ "question": format!("Q{q}"), "options": ["A", "B"] }))
            .collect();
        parse(&json!({ "questions": questions })).unwrap()
    }

    /// A call of a single-select question ("Which?", A or B) and one without options ("When?").
    fn one_and_free() -> Ask {
        let questions = json!([
            { "question": "Which?", "options": ["A", "B"] },
            { "question": "When?" }
        ]);
        parse(&json!({ "questions": questions })).unwrap()
    }

    fn one(question: Value) -> Ask {
        parse(&json!({ "questions": [question] })).unwrap()
    }

    #[test]
    fn a_single_select_question_keeps_one_pick() {
        let ask = one(json!({ "question": "Which?", "options": ["Postgres", "SQLite"] }));
        let mut form = Form::new(&ask);
        form.pick(&ask, 0, 0);
        form.pick(&ask, 0, 1);
        assert!(!form.picked(0, 0) && form.picked(0, 1));
        form.typed(&ask, 0, "MySQL");
        assert!(!form.picked(0, 1), "typing in Other unpicks the option");
        assert!(form.other_picked(0));
        form.pick(&ask, 0, 0);
        assert!(!form.other_picked(0), "an option unpicks Other");
        assert_eq!(
            form.answers(&ask, &["MySQL".into()]),
            Some(vec![vec!["Postgres".to_string()]])
        );
    }

    #[test]
    fn a_multi_select_question_toggles() {
        let ask = one(
            json!({ "question": "Which?", "options": ["Auth", "Search", "Export"],
                              "multiSelect": true }),
        );
        let mut form = Form::new(&ask);
        form.pick(&ask, 0, 0);
        form.pick(&ask, 0, 2);
        form.pick(&ask, 0, 0);
        assert!(!form.picked(0, 0) && form.picked(0, 2));
        form.typed(&ask, 0, "Docs");
        assert!(form.picked(0, 2) && form.other_picked(0));
        assert_eq!(
            form.answers(&ask, &["Docs".into()]),
            Some(vec![vec!["Export".to_string(), "Docs".to_string()]])
        );
        form.typed(&ask, 0, "");
        assert!(!form.other_picked(0), "emptying the field unpicks Other");
    }

    #[test]
    fn other_with_only_spaces_is_no_answer() {
        let ask = one(json!({ "question": "Which?", "options": ["Postgres"] }));
        let mut form = Form::new(&ask);
        form.typed(&ask, 0, "   ");
        assert!(!form.other_picked(0));
        assert_eq!(form.answers(&ask, &["   ".into()]), None);
        form.typed(&ask, 0, "  MySQL ");
        assert_eq!(
            form.answers(&ask, &["  MySQL ".into()]),
            Some(vec![vec!["MySQL".to_string()]])
        );
    }

    #[test]
    fn a_question_without_options_is_its_field() {
        let ask = one(json!({ "question": "When?" }));
        assert_eq!(
            Form::new(&ask).rows(&ask),
            [Row::Other(0), Row::Note, Row::Send]
        );
        let today = parse(&json!({ "question": "When?" })).unwrap();
        assert_eq!(Form::new(&today).rows(&today), [Row::Other(0), Row::Send]);
        for ask in [ask, today] {
            let mut form = Form::new(&ask);
            assert_eq!(form.answers(&ask, &[String::new()]), None);
            form.typed(&ask, 0, "Friday");
            assert_eq!(
                form.answers(&ask, &["Friday".into()]),
                Some(vec![vec!["Friday".to_string()]])
            );
            form.typed(&ask, 0, " ");
            assert_eq!(form.answers(&ask, &[" ".into()]), None);
        }
    }

    #[test]
    fn todays_shape_has_no_other_nor_note() {
        let ask = parse(&json!({ "question": "Ship it?", "options": ["Yes", "No"] })).unwrap();
        assert_eq!(
            Form::new(&ask).rows(&ask),
            [Row::Option(0, 0), Row::Option(0, 1), Row::Send]
        );
    }

    #[test]
    fn the_highlight_starts_on_none_then_steps_and_wraps() {
        let ask = singles(2);
        let mut form = Form::new(&ask);
        assert_eq!(form.highlight, None);
        form.step(&ask, 1);
        assert_eq!(form.highlight, Some(Row::Option(0, 0)));
        form.step(&ask, 1);
        form.step(&ask, 1);
        assert_eq!(form.highlight, Some(Row::Other(0)));
        let mut back = Form::new(&ask);
        back.step(&ask, -1);
        assert_eq!(back.highlight, Some(Row::Send));
        back.step(&ask, -1);
        assert_eq!(back.highlight, Some(Row::Note));
        back.step(&ask, 1);
        back.step(&ask, 1);
        assert_eq!(
            back.highlight,
            Some(Row::Option(0, 0)),
            "from Send it wraps"
        );
    }

    /// One question shows at a time: the keys move through its rows, the note and Send only.
    #[test]
    fn the_rows_are_the_shown_questions() {
        let ask = singles(2);
        let mut form = Form::new(&ask);
        let shown = |q| vec![Row::Option(q, 0), Row::Option(q, 1), Row::Other(q)];
        assert_eq!(
            form.rows(&ask),
            [shown(0), vec![Row::Note, Row::Send]].concat()
        );
        form.show(&ask, 1);
        assert_eq!(
            form.rows(&ask),
            [shown(1), vec![Row::Note, Row::Send]].concat()
        );
        form.highlight = Some(Row::Other(1));
        form.step(&ask, 1);
        form.step(&ask, 1);
        form.step(&ask, 1);
        assert_eq!(
            form.highlight,
            Some(Row::Option(1, 0)),
            "Send wraps to its first"
        );
    }

    #[test]
    fn showing_a_question_moves_a_highlight_off_the_hidden_one() {
        let ask = one_and_free();
        let mut form = Form::new(&ask);
        form.show(&ask, 1);
        assert_eq!((form.tab, form.highlight), (1, None), "none stays none");
        form.highlight = Some(Row::Note);
        form.show(&ask, 0);
        assert_eq!(form.highlight, Some(Row::Note), "the note shows under each");
        form.highlight = Some(Row::Option(0, 1));
        form.show(&ask, 1);
        assert_eq!(
            form.highlight,
            Some(Row::Other(1)),
            "a free question's field"
        );
        form.show(&ask, 0);
        assert_eq!(form.highlight, Some(Row::Option(0, 0)));
    }

    #[test]
    fn turning_wraps_both_ways() {
        let ask = singles(3);
        let mut form = Form::new(&ask);
        form.turn(&ask, -1);
        assert_eq!(form.tab, 2);
        form.turn(&ask, 1);
        assert_eq!(form.tab, 0);
        form.turn(&ask, 1);
        assert_eq!(form.tab, 1);
    }

    #[test]
    fn an_answer_is_checked_against_its_questions() {
        let ask = parse(&json!({"questions": [
            {"question": "Which database?", "header": "DB", "options": [{"label": "Postgres"}, {"label": "SQLite"}]},
            {"question": "Extras?", "header": "Extras", "multiSelect": true, "options": [{"label": "A"}, {"label": "B"}]},
        ]}))
        .unwrap();
        let v = |answers: &[&[&str]]| -> Vec<Vec<String>> {
            answers
                .iter()
                .map(|a| a.iter().map(|s| s.to_string()).collect())
                .collect()
        };
        assert_eq!(ask.checked(&v(&[&["Postgres"], &["A", "B"]])), Ok(()));
        assert_eq!(
            ask.checked(&v(&[&["Postgres"], &["A", "my own"]])),
            Ok(()),
            "one Other"
        );
        let err = |answers: &[&[&str]]| ask.checked(&v(answers)).unwrap_err();
        assert_eq!(
            err(&[&["Postgres"]]),
            "Answer the 2 questions: answers has 1."
        );
        assert_eq!(
            err(&[&["Postgres", "SQLite"], &["A"]]),
            "Question 1 takes one answer."
        );
        assert_eq!(err(&[&["Postgres"], &[]]), "Question 2 needs an answer.");
        assert_eq!(err(&[&["  "], &["A"]]), "An answer is blank.");
        assert_eq!(
            err(&[&["Postgres"], &["x", "y"]]),
            "Question 2 takes one other answer at most: \"x\" and \"y\" aren't its options."
        );
        let plan = plan(Some("t".into()), "body".into());
        assert_eq!(plan.checked(&v(&[&["Approve"]])), Ok(()));
        assert_eq!(
            plan.checked(&v(&[&["maybe"]])).unwrap_err(),
            "A plan is Approve or Reject."
        );
    }

    /// A call with several questions shows tabs; one question, today's shape and a plan don't.
    #[test]
    fn only_several_questions_have_tabs() {
        assert!(singles(2).tabs());
        assert!(!singles(1).tabs());
        let today = parse(&json!({ "question": "Ship it?", "options": ["Yes"] })).unwrap();
        assert!(!today.tabs());
        assert!(!plan(None, "ok".into()).tabs());
    }

    #[test]
    fn a_tab_is_named_by_its_header_or_its_place() {
        let ask = parse(&json!({ "questions": [
            { "question": "Which?", "header": "A rather long header" },
            { "question": "When?" }
        ] }))
        .unwrap();
        assert_eq!(ask.tab(0), ask.questions[0].header_shown().unwrap());
        assert_eq!(ask.tab(1), "Question 2");
    }

    #[test]
    fn a_tab_knows_its_question_is_answered() {
        let ask = one_and_free();
        let mut form = Form::new(&ask);
        let mut others = vec![String::new(); 2];
        assert!(!form.has_answer(&ask, 0, &others) && !form.has_answer(&ask, 1, &others));
        form.pick(&ask, 0, 1);
        others[1] = "Friday".into();
        form.typed(&ask, 1, "Friday");
        assert!(form.has_answer(&ask, 0, &others) && form.has_answer(&ask, 1, &others));
    }

    /// The next question without an answer starts after the shown one while the highlight is on
    /// it, and from it otherwise (the note, Send, none).
    #[test]
    fn the_next_unanswered_starts_from_the_shown_question() {
        let ask = singles(3);
        let others = vec![String::new(); 3];
        let mut form = Form::new(&ask);
        form.show(&ask, 1);
        assert_eq!(form.unanswered(&ask, &others), Some(1), "from it");
        form.highlight = Some(Row::Option(1, 1));
        assert_eq!(form.unanswered(&ask, &others), Some(2), "after it");
        form.pick(&ask, 2, 0);
        assert_eq!(form.unanswered(&ask, &others), Some(0), "wrapping");
        form.pick(&ask, 0, 0);
        form.pick(&ask, 1, 0);
        assert_eq!(form.unanswered(&ask, &others), None);
    }

    #[test]
    fn next_unanswered_skips_answered_questions() {
        let ask = singles(3);
        let others = vec![String::new(); 3];
        let mut form = Form::new(&ask);
        form.pick(&ask, 1, 0);
        form.highlight = Some(Row::Option(0, 1));
        form.next_unanswered(&ask, &others);
        assert_eq!(form.highlight, Some(Row::Option(2, 0)));
        assert_eq!(form.tab, 2, "its question shows");
        form.next_unanswered(&ask, &others);
        assert_eq!(form.highlight, Some(Row::Option(0, 0)), "it wraps");
        form.pick(&ask, 0, 0);
        form.pick(&ask, 2, 1);
        form.next_unanswered(&ask, &others);
        assert_eq!(
            form.highlight,
            Some(Row::Option(0, 0)),
            "all answered: it stays"
        );
    }

    #[test]
    fn the_foot_counts_the_questions_answered() {
        let ask = singles(3);
        let mut others = vec![String::new(); 3];
        let mut form = Form::new(&ask);
        assert_eq!(form.answered(&ask, &others), 0);
        form.pick(&ask, 1, 0);
        assert_eq!(form.answered(&ask, &others), 1);
        others[2] = "later".into();
        form.typed(&ask, 2, "later");
        assert_eq!(form.answered(&ask, &others), 2, "Other's text counts");
        form.typed(&ask, 2, "  ");
        assert_eq!(form.answered(&ask, &others), 1, "a blank one doesn't");
    }

    #[test]
    fn a_preview_is_clean_text() {
        assert_eq!(
            clean_preview("a\tb\r\n\x1b[31mred\x1b[0m\x07"),
            "a    b\nred"
        );
        assert_eq!(clean_preview("one\rtwo"), "one\ntwo");
        assert_eq!(
            clean_preview("\x1b]0;title\x07x\x1b]8;;http://a\x1b\\y\x1bcz"),
            "xyz",
            "OSC and two-character escapes go too"
        );
    }

    #[test]
    fn the_preview_follows_the_highlight() {
        let ask = one(json!({ "question": "Which?", "options": [
            { "label": "A", "preview": "zero" }, { "label": "B" }
        ] }));
        let mut form = Form::new(&ask);
        form.highlight = Some(Row::Option(0, 1));
        assert_eq!(ask.preview_of(0, &form), Some(None), "No preview");
        form.highlight = Some(Row::Option(0, 0));
        assert_eq!(ask.preview_of(0, &form), Some(Some("zero")));
        form.highlight = Some(Row::Note);
        assert_eq!(
            ask.preview_of(0, &form),
            Some(Some("zero")),
            "nothing picked: the first with one"
        );
        form.pick(&ask, 0, 1);
        assert_eq!(ask.preview_of(0, &form), Some(None), "the picked one's");
        let plain = one(json!({ "question": "Which?", "options": ["A", "B"] }));
        assert_eq!(plain.preview_of(0, &Form::new(&plain)), None);
    }
}
