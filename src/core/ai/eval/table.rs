//! The pass rule, the acceptance rule and the score table of a live
//! evaluation (openspec `local-multistep`, C2, C8, C9): pure rules and plain
//! ASCII text, so a person, a PR and a later script read the same thing.

use std::{collections::BTreeMap, time::Duration};

use super::{Category, RunResult, Scenario};

/// The most turn time a passing run may take (C8): exactly this is judged by
/// its steps, anything above fails.
const TURN_CAP: Duration = Duration::from_secs(600);

/// The models whose combinations decide whether a change is accepted; any
/// other model is informative and never blocks.
pub const ACCEPTED: [&str; 3] = ["qwen2.5:7b", "qwen2.5-coder:14b", "qwen3:14b"];

/// One scored run and what the harness measured around it.
pub struct Run {
    pub result: RunResult,
    pub time: Duration,
    /// The harness denied an approval the scenario did not expect.
    pub unexpected: bool,
}

impl Run {
    pub fn new(result: RunResult, time: Duration) -> Self {
        Self {
            result,
            time,
            unexpected: false,
        }
    }

    fn late(&self) -> bool {
        self.time > TURN_CAP
    }

    pub fn passed(&self) -> bool {
        self.result.passed() && !self.unexpected && !self.late()
    }

    /// Why the run failed: a kind and, when one helps, what it names.
    fn reasons(&self) -> Vec<(&'static str, String)> {
        let r = &self.result;
        let mut list: Vec<_> = r.missing.iter().map(|l| ("missing", (*l).into())).collect();
        let pairs = r
            .out_of_order
            .iter()
            .map(|(a, b)| ("order", format!("{a}>{b}")));
        list.extend(pairs);
        list.extend(r.ignored.iter().map(|_| ("literal", String::new())));
        list.extend(r.asked_back.then(|| ("asked", String::new())));
        list.extend(r.forbidden.iter().map(|t| ("forbidden", t.clone())));
        list.extend(self.unexpected.then(|| ("unexpected", String::new())));
        list.extend(self.late().then(|| ("time", String::new())));
        list
    }
}

/// The runs of one model on one task.
pub struct Row {
    model: String,
    task: &'static str,
    category: Category,
    runs: Vec<Run>,
}

impl Row {
    pub fn new(model: &str, scenario: &Scenario, runs: Vec<Run>) -> Self {
        Self {
            model: model.to_owned(),
            task: scenario.id,
            category: scenario.category,
            runs,
        }
    }

    fn passed(&self) -> usize {
        self.runs.iter().filter(|run| run.passed()).count()
    }

    fn name(&self) -> String {
        format!("{} {}", self.model, self.task)
    }

    /// The failure kinds of the failed runs, with how many runs had each.
    fn failures(&self) -> String {
        let mut kinds = BTreeMap::<String, usize>::new();
        for run in self.runs.iter().filter(|run| !run.passed()) {
            let mut seen: Vec<String> = Vec::new();
            for (kind, detail) in run.reasons() {
                let name = if kind == "missing" {
                    format!("missing:{detail}")
                } else {
                    kind.to_owned()
                };
                if !seen.contains(&name) {
                    *kinds.entry(name.clone()).or_default() += 1;
                    seen.push(name);
                }
            }
        }
        let list: Vec<_> = kinds.iter().map(|(k, n)| format!("{k} x{n}")).collect();
        if list.is_empty() {
            "-".to_owned()
        } else {
            list.join(", ")
        }
    }

    /// One line per failed run: its number and every reason it failed.
    fn reasons(&self) -> String {
        let mut out = String::new();
        for (n, run) in self.runs.iter().enumerate().filter(|(_, r)| !r.passed()) {
            let words: Vec<String> = run
                .reasons()
                .into_iter()
                .map(|(kind, detail)| match (kind, detail.is_empty()) {
                    ("asked", _) => "asked back".to_owned(),
                    ("literal", _) => "ignored literal".to_owned(),
                    ("unexpected", _) => "unexpected approval".to_owned(),
                    ("time", _) => "over time".to_owned(),
                    ("order", _) => format!("out of order:{detail}"),
                    (kind, _) => format!("{kind}:{detail}"),
                })
                .collect();
            out.push_str(&format!("  run {}: {}\n", n + 1, words.join(", ")));
        }
        out
    }
}

/// Whether a combination passes: 4 of 5 runs, or the same share.
pub fn passes(passed: usize, runs: usize) -> bool {
    runs > 0 && passed * 5 >= runs * 4
}

/// The middle of `secs`; the mean of the two middle ones when even.
pub fn median(secs: &[u64]) -> u64 {
    let mut sorted = secs.to_vec();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    }
}

/// What a change did to the accepted set.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    Accepted,
    /// The combinations that fell below passing, or `nothing rose` when none fell
    /// and none rose.
    Refused(Vec<String>),
    /// Combinations at exactly 4 of 5 on one side: run their 5 runs again.
    Repeat(Vec<String>),
}

/// Judges `after` against `before` on the accepted models (C9): accepted when
/// a combination rose and none fell below passing or vanished. A rise onto or
/// off exactly 4 of 5 is borderline and is repeated even when another
/// combination rose; a drop onto 4 of 5 is noise that a sure rise outweighs.
pub fn accept(before: &[Row], after: &[Row]) -> Verdict {
    let accepted = |r: &&Row| ACCEPTED.contains(&r.model.as_str());
    let (mut fell, mut sure, mut up, mut down) = (Vec::new(), false, Vec::new(), Vec::new());
    for was in before.iter().filter(accepted) {
        if !after.iter().any(|r| r.name() == was.name()) {
            fell.push(format!("{} missing", was.name()));
        }
    }
    for now in after.iter().filter(accepted) {
        let Some(was) = before.iter().find(|r| r.name() == now.name()) else {
            continue;
        };
        let (a, b, runs) = (now.passed(), was.passed(), now.runs.len());
        let edge = a == 4 || b == 4;
        if passes(b, was.runs.len()) && !passes(a, runs) {
            fell.push(now.name());
        } else if a > b && edge {
            up.push(now.name());
        } else if a > b {
            sure = true;
        } else if a < b && edge {
            down.push(now.name());
        }
    }
    match (fell.is_empty(), up.is_empty(), sure, down.is_empty()) {
        (false, ..) => Verdict::Refused(fell),
        (_, false, ..) => Verdict::Repeat(up),
        (_, _, true, _) => Verdict::Accepted,
        (.., false) => Verdict::Repeat(down),
        _ => Verdict::Refused(vec!["nothing rose".to_owned()]),
    }
}

/// The first block of a table.
pub struct Header {
    pub date: String,
    pub ollama: String,
    pub hardware: String,
    /// The short git sha of the lazarobox that ran.
    pub sha: String,
    pub runs: usize,
}

/// The hardware line: the given text, else the GPU name and memory, else `cpu`.
pub fn hardware(given: Option<&str>, gpu: Option<(&str, &str)>) -> String {
    match (given.map(str::trim).filter(|g| !g.is_empty()), gpu) {
        (Some(given), _) => given.to_owned(),
        (None, Some((name, memory))) => format!("{name}, {memory}"),
        (None, None) => "cpu".to_owned(),
    }
}

/// A header field as safe text: ASCII only, one line, and a word that looks
/// like a path (`/` or `\`) replaced, so a table never carries a path.
fn clean(field: &str) -> String {
    let words: Vec<String> = field
        .split_whitespace()
        .map(|word| {
            let word: String = word
                .chars()
                .map(|c| if c.is_ascii_graphic() { c } else { '?' })
                .collect();
            if word.contains(['/', '\\']) {
                "?".to_owned()
            } else {
                word
            }
        })
        .collect();
    words.join(" ").chars().take(80).collect()
}

/// The UTC date `YYYY-MM-DD` of a Unix time.
pub fn date(unix_secs: u64) -> String {
    let z = unix_secs / 86_400 + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A turn time in whole seconds, rounded.
fn secs(time: Duration) -> u64 {
    time.as_secs() + u64::from(time.subsec_millis() >= 500)
}

/// The table as plain ASCII text: the header, one row per model and task, and
/// a summary per model and category.
pub fn render(header: &Header, rows: &[Row]) -> String {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| (&a.model, a.task).cmp(&(&b.model, b.task)));
    let width = |f: fn(&Row) -> usize| sorted.iter().map(|r| f(r)).max().unwrap_or(0);
    let (m, t, c) = (
        width(|r| r.model.len()),
        width(|r| r.task.len()),
        width(|r| r.category.key().len()),
    );
    let mut out = format!(
        "lazarobox local-model eval, format 1\ndate: {}\nollama: {}\nhardware: {}\nruns: {}\nlazarobox: {}\n\n",
        clean(&header.date),
        clean(&header.ollama),
        clean(&header.hardware),
        header.runs,
        clean(&header.sha)
    );
    let mut summary = BTreeMap::<(&str, &str), (usize, usize)>::new();
    for row in &sorted {
        let times: Vec<u64> = row.runs.iter().map(|run| secs(run.time)).collect();
        let list: Vec<String> = times.iter().map(u64::to_string).collect();
        out.push_str(&format!(
            "{:<m$}  {:<t$}  {:<c$}  {}/{}  {}  seconds {}  median {}\n",
            row.model,
            row.task,
            row.category.key(),
            row.passed(),
            row.runs.len(),
            row.failures(),
            list.join(" "),
            median(&times),
        ));
        out.push_str(&row.reasons());
        let entry = summary.entry((&row.model, row.category.key())).or_default();
        entry.0 += usize::from(passes(row.passed(), row.runs.len()));
        entry.1 += 1;
    }
    out.push_str("\nsummary\n");
    for ((model, category), (passed, total)) in summary {
        out.push_str(&format!("{model} {category} {passed}/{total}\n"));
    }
    out
}
