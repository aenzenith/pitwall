//! The Today page's export: the day files' figures over a stretch of days as a CSV file, written
//! once into the file the user picked in the save dialog (`commands::day_export`). One row per
//! day, project and agent: its working and waiting minutes, turns, tokens, share of Claude's week
//! and API-equivalent cost, as the Today page counts them (`Core::day_figures`). The projects'
//! names are the only text in it: no session's name, no path, command or card, nothing an agent
//! or the user wrote. Spreadsheets in languages that write a decimal comma expect a semicolon
//! between fields, so the language picks both; a byte order mark tells Excel it is UTF-8.

use super::activity::{day_bounds, DayFigures, DAY_MS, KEEP_DAYS};
use super::*;
use crate::clock::day_name;

/// The stretch of days an export would cover, as the dialog shows it before saving.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    /// The days asked for, cut to those there can be files of: `YYYY-MM-DD`, local.
    pub from: String,
    pub to: String,
    /// The earliest day a file is kept of.
    pub oldest: String,
    pub projects: u32,
    pub rows: u32,
    /// The file's column heads, in the app's language.
    pub columns: Vec<String>,
}

/// One row of the file, its project named.
struct Row {
    date: String,
    name: String,
    listed: bool,
    figures: DayFigures,
}

impl Core {
    /// What an export of `from`..=`to` would hold.
    pub fn export_preview(&self, from: &str, to: &str) -> Result<ExportPreview, String> {
        let (from, to, oldest) = export_range(from, to)?;
        let (rows, agents) = self.export_rows(&from, &to);
        let projects = rows.iter().map(|row| (row.listed, row.name.as_str())).collect::<HashSet<_>>().len() as u32;
        Ok(ExportPreview { from, to, oldest, projects, rows: rows.len() as u32, columns: columns(agents) })
    }

    /// Writes the export of `from`..=`to` into `file`, the one the user picked.
    pub fn export_write(&self, file: &Path, from: &str, to: &str) -> Result<(), String> {
        let (from, to, _) = export_range(from, to)?;
        let (rows, agents) = self.export_rows(&from, &to);
        let text = csv(&rows, agents, Format::of(i18n::current()));
        fs::write(file, text).map_err(|error| t!("core.error.exportWrite", error = error))
    }

    /// The rows of the days `from`..=`to` (already cut), the earliest day first and each day's
    /// projects by name, those not listed last; and whether the file names its agents: Codex is on,
    /// or one of the rows is its.
    fn export_rows(&self, from: &str, to: &str) -> (Vec<Row>, bool) {
        let names: HashMap<String, String> = self.snapshot().projects.into_iter().map(|p| (p.path, p.name)).collect();
        let listed: Vec<String> = names.keys().cloned().collect();

        let mut rows = Vec::new();
        let mut day = from.to_string();
        while day.as_str() <= to {
            let Some((_, end)) = day_bounds(&day) else {
                break;
            };
            let mut figures: Vec<Row> = self
                .day_figures(&day, &listed)
                .into_iter()
                .map(|figures| {
                    let listed = !figures.path.is_empty();
                    let name = match names.get(&figures.path) {
                        Some(name) => name.clone(),
                        None if listed => folder_name(&figures.path),
                        None => t!("core.export.unlisted"),
                    };
                    Row { date: day.clone(), name, listed, figures }
                })
                .collect();
            figures.sort_by(|a, b| {
                (!a.listed, a.name.to_lowercase(), &a.figures.path, a.figures.agent).cmp(&(!b.listed, b.name.to_lowercase(), &b.figures.path, b.figures.agent))
            });
            rows.extend(figures);
            day = day_name(end);
        }

        let agents = self.agent_on(Agent::Codex) || rows.iter().any(|row| row.figures.agent != Agent::Claude);
        (rows, agents)
    }
}

/// The name the save dialog offers for an export of `from`..=`to`: the days it covers.
pub fn export_file_name(from: &str, to: &str) -> Result<String, String> {
    let (from, to, _) = export_range(from, to)?;
    Ok(if from == to { format!("Pitwall {from}.csv") } else { format!("Pitwall {from} – {to}.csv") })
}

/// `from`..=`to` cut to the days there can be files of (the oldest kept to today), and the oldest.
fn export_range(from: &str, to: &str) -> Result<(String, String, String), String> {
    let now = now_ms();
    let today = day_name(now);
    let oldest = day_name(now.saturating_sub(KEEP_DAYS * DAY_MS));
    if day_bounds(from).is_none() || day_bounds(to).is_none() {
        return Err(t!("core.error.exportRange"));
    }
    let from = from.max(oldest.as_str()).to_string();
    let to = to.min(today.as_str()).to_string();
    if from > to {
        return Err(t!("core.error.exportRange"));
    }
    Ok((from, to, oldest))
}

fn columns(agents: bool) -> Vec<String> {
    let mut keys = vec!["core.export.date", "core.export.project"];
    if agents {
        keys.push("core.export.agent");
    }
    keys.extend(["core.export.worked", "core.export.waited", "core.export.turns", "core.export.tokens", "core.export.week", "core.export.cost"]);
    keys.into_iter().map(|key| t!(key)).collect()
}

/// How numbers and fields are written for a language's spreadsheets.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Format {
    separator: char,
    decimal: char,
}

impl Format {
    fn of(language: &str) -> Self {
        match language {
            "tr" | "de" | "es" | "ru" => Self { separator: ';', decimal: ',' },
            _ => Self { separator: ',', decimal: '.' },
        }
    }

    fn number(self, value: f64) -> String {
        let text = format!("{value:.2}");
        if self.decimal == '.' { text } else { text.replace('.', &self.decimal.to_string()) }
    }

    /// A text field: quoted when it holds the separator, a quote or a line break; one that a
    /// spreadsheet would take for a formula goes in after an apostrophe, so it stays text.
    fn text(self, value: &str) -> String {
        let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{value}") } else { value.to_string() };
        if value.contains([self.separator, '"', '\n', '\r']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value
        }
    }
}

fn csv(rows: &[Row], agents: bool, format: Format) -> String {
    let separator = format.separator.to_string();
    let line = |fields: Vec<String>| fields.join(&separator);

    let mut lines = vec![line(columns(agents).iter().map(|head| format.text(head)).collect())];
    for row in rows {
        let figures = &row.figures;
        let minutes = |ms: u64| if row.listed { ((ms + 30_000) / 60_000).to_string() } else { String::new() };
        let mut fields = vec![row.date.clone(), format.text(&row.name)];
        if agents {
            fields.push(figures.agent.name().to_string());
        }
        fields.extend([
            minutes(figures.work),
            minutes(figures.wait),
            if row.listed { figures.turns.to_string() } else { String::new() },
            figures.tokens.map(|tokens| tokens.to_string()).unwrap_or_default(),
            figures.week.map(|week| format.number(week)).unwrap_or_default(),
            figures.cost.map(|cost| format.number(cost)).unwrap_or_default(),
        ]);
        lines.push(line(fields));
    }

    format!("\u{feff}{}\r\n", lines.join("\r\n"))
}
