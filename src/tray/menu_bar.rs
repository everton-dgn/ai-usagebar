//! Omarchy-style provider summary for the macOS status item.
//!
//! The report owns provider names and metric values. This module only chooses
//! the visible entry and quota window, so the native host can repaint without
//! asking the WebView to be open.

use std::collections::BTreeMap;

use serde_json::Value;

pub const HIGHEST_PROVIDER: &str = "highest";

/// The popover's bundled provider marks (`windows/popover/src/icons/providers`),
/// drawn by the macOS status item in place of the provider's name.
pub const PROVIDER_MARKS: &[(&str, &str)] = &[
    (
        "anthropic",
        include_str!("../../windows/popover/src/icons/providers/anthropic.svg"),
    ),
    (
        "anthropic_api",
        include_str!("../../windows/popover/src/icons/providers/anthropic_api.svg"),
    ),
    (
        "antigravity",
        include_str!("../../windows/popover/src/icons/providers/antigravity.svg"),
    ),
    (
        "copilot",
        include_str!("../../windows/popover/src/icons/providers/copilot.svg"),
    ),
    (
        "cursor",
        include_str!("../../windows/popover/src/icons/providers/cursor.svg"),
    ),
    (
        "deepseek",
        include_str!("../../windows/popover/src/icons/providers/deepseek.svg"),
    ),
    (
        "grok",
        include_str!("../../windows/popover/src/icons/providers/grok.svg"),
    ),
    (
        "grokbot",
        include_str!("../../windows/popover/src/icons/providers/grokbot.svg"),
    ),
    (
        "kimi",
        include_str!("../../windows/popover/src/icons/providers/kimi.svg"),
    ),
    (
        "minimax",
        include_str!("../../windows/popover/src/icons/providers/minimax.svg"),
    ),
    (
        "moonshot",
        include_str!("../../windows/popover/src/icons/providers/moonshot.svg"),
    ),
    (
        "openai",
        include_str!("../../windows/popover/src/icons/providers/openai.svg"),
    ),
    (
        "opencode_go",
        include_str!("../../windows/popover/src/icons/providers/opencode_go.svg"),
    ),
    (
        "openrouter",
        include_str!("../../windows/popover/src/icons/providers/openrouter.svg"),
    ),
    (
        "zai",
        include_str!("../../windows/popover/src/icons/providers/zai.svg"),
    ),
];

/// Entry slugs whose mark file has another name; mirrors `ICON_ALIAS` in
/// `windows/popover/src/model.js` (a test keeps the two in step).
const MARK_ALIASES: &[(&str, &str)] = &[("supergrok", "grok"), ("opencode-go", "opencode_go")];

/// One provider in the menu bar: its mark when one is bundled, its name for
/// the tooltip and for when the mark cannot be drawn, and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub mark: Option<&'static str>,
    pub name: String,
    pub value: Option<String>,
}

impl Chip {
    /// The chip as plain text, for a status item that cannot draw marks.
    pub fn text(&self) -> String {
        match &self.value {
            Some(value) if !self.name.is_empty() => format!("{} {value}", self.name),
            _ => self.name.clone(),
        }
    }
}

/// The bundled mark for an entry id, like the popover's `providerIconId`.
pub fn mark_for(id: &str) -> Option<&'static str> {
    let slug = id
        .split('@')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let slug = MARK_ALIASES
        .iter()
        .find(|(from, _)| *from == slug)
        .map_or(slug.as_str(), |(_, to)| to);
    PROVIDER_MARKS
        .iter()
        .find(|(name, _)| *name == slug)
        .map(|(_, svg)| *svg)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UsageWindow {
    #[default]
    Auto,
    Session,
    Weekly,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Monthly,
}

impl UsageWindow {
    /// Read by the macOS host's set-menu-bar-window IPC handler; the Linux
    /// test build never calls it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn parse(value: &str) -> Self {
        match value {
            "session" => Self::Session,
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            _ => Self::Auto,
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Session => "session",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
}

pub fn selected_id<'a>(
    payload: &'a Value,
    remembered: &str,
    window: UsageWindow,
    visible: Option<&[String]>,
) -> Option<&'a str> {
    let entries = eligible_entries(payload, visible);
    if entries.is_empty() {
        return None;
    }
    let has_id = |id: &str| {
        entries
            .iter()
            .any(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
    };
    if !remembered.is_empty() && remembered != HIGHEST_PROVIDER && has_id(remembered) {
        return entries
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(remembered))
            .and_then(|entry| entry.get("id").and_then(Value::as_str));
    }
    let mut best: Option<(&Value, f64)> = None;
    for entry in &entries {
        let Some(percent) = highest_percent(entry, window) else {
            continue;
        };
        if best.is_none_or(|(_, previous)| percent > previous) {
            best = Some((entry, percent));
        }
    }
    if let Some((entry, _)) = best {
        return entry.get("id").and_then(Value::as_str);
    }
    if let Some(primary) = payload.get("primary").and_then(Value::as_str) {
        if has_id(primary) {
            return Some(primary);
        }
        if let Some(entry) = entries.iter().find(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.split('@').next() == Some(primary))
        }) {
            return entry.get("id").and_then(Value::as_str);
        }
    }
    entries
        .iter()
        .find(|entry| is_ready(entry))
        .or_else(|| entries.first())?
        .get("id")
        .and_then(Value::as_str)
}

pub fn next_id(
    payload: &Value,
    remembered: &str,
    window: UsageWindow,
    visible: Option<&[String]>,
) -> Option<String> {
    let ids: Vec<&str> = eligible_entries(payload, visible)
        .into_iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .collect();
    if ids.is_empty() {
        return None;
    }
    let current = selected_id(payload, remembered, window, visible).unwrap_or(ids[0]);
    let index = ids.iter().position(|id| *id == current).unwrap_or(0);
    Some(ids[(index + 1) % ids.len()].to_owned())
}

/// The menu bar's providers, in display order. `names` are the popover's
/// custom card titles, which win over the report's names.
pub fn chips(
    payload: &Value,
    remembered: &str,
    show_all: bool,
    show_value: bool,
    window: UsageWindow,
    visible: Option<&[String]>,
    names: &BTreeMap<String, String>,
) -> Vec<Chip> {
    displayed_entries(payload, remembered, show_all, window, visible)
        .into_iter()
        .map(|entry| chip(entry, show_value, window, names))
        .filter(|chip| chip.mark.is_some() || !chip.name.is_empty())
        .collect()
}

/// The menu bar as plain text: every chip's name and value.
pub fn text(chips: &[Chip]) -> String {
    chips
        .iter()
        .map(Chip::text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("   ")
}

/// Rendered by the macOS status item only; the Linux test build never calls it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tooltip(
    payload: &Value,
    remembered: &str,
    show_all: bool,
    window: UsageWindow,
    visible: Option<&[String]>,
    names: &BTreeMap<String, String>,
) -> String {
    let lines: Vec<String> = displayed_entries(payload, remembered, show_all, window, visible)
        .into_iter()
        .map(|entry| {
            let name = entry_name(entry, names).unwrap_or("AI Usage");
            let stale = if entry.get("stale").and_then(Value::as_bool) == Some(true) {
                " · cached"
            } else {
                ""
            };
            format!(
                "{} · {}{stale}",
                safe_text(name, 48),
                headline(entry, window)
            )
        })
        .collect();
    if lines.is_empty() {
        "AI Usage".into()
    } else {
        lines.join("\n")
    }
}

fn displayed_entries<'a>(
    payload: &'a Value,
    remembered: &'a str,
    show_all: bool,
    window: UsageWindow,
    visible: Option<&[String]>,
) -> Vec<&'a Value> {
    let entries = eligible_entries(payload, visible);
    if show_all {
        let ready: Vec<&Value> = entries
            .iter()
            .copied()
            .filter(|entry| entry.get("status").and_then(Value::as_str) != Some("error"))
            .collect();
        if !ready.is_empty() {
            return ready;
        }
    }
    let selected = selected_id(payload, remembered, window, visible);
    entries
        .into_iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) == selected)
        .collect()
}

fn eligible_entries<'a>(payload: &'a Value, visible: Option<&[String]>) -> Vec<&'a Value> {
    let Some(entries) = payload.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| {
            let Some(id) = entry.get("id").and_then(Value::as_str) else {
                return false;
            };
            visible.is_none_or(|ids| ids.iter().any(|allowed| allowed == id))
        })
        .collect()
}

fn is_ready(entry: &Value) -> bool {
    entry.get("status").and_then(Value::as_str) != Some("error")
        && !entry
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| !error.is_empty())
}

fn highest_percent(entry: &Value, window: UsageWindow) -> Option<f64> {
    if !is_ready(entry) {
        return None;
    }
    best_metric(entry, window, false)?.get("percent")?.as_f64()
}

fn best_metric(entry: &Value, window: UsageWindow, fallback: bool) -> Option<&Value> {
    let metrics: Vec<&Value> = entry
        .get("sections")?
        .as_array()?
        .iter()
        .filter(|section| section.get("type").and_then(Value::as_str) == Some("metric"))
        .collect();
    let candidates: Vec<&Value> = metrics
        .iter()
        .copied()
        .filter(|metric| matches_window(metric, window))
        .collect();
    let pool = if candidates.is_empty() {
        if !fallback {
            return None;
        }
        &metrics
    } else {
        &candidates
    };
    pool.iter().copied().max_by(|a, b| {
        let a = a.get("percent").and_then(Value::as_f64).unwrap_or(0.0);
        let b = b.get("percent").and_then(Value::as_f64).unwrap_or(0.0);
        a.total_cmp(&b)
    })
}

/// The custom title for an entry, else the report's name.
fn entry_name<'a>(entry: &'a Value, names: &'a BTreeMap<String, String>) -> Option<&'a str> {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    names
        .get(id)
        .map(String::as_str)
        .or_else(|| entry.get("display_name").and_then(Value::as_str))
        .or_else(|| entry.get("name").and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
}

/// One entry's chip: its mark, its name (custom, reported, short, or the id's
/// vendor) and, unless values are hidden, its headline.
fn chip(
    entry: &Value,
    show_value: bool,
    window: UsageWindow,
    names: &BTreeMap<String, String>,
) -> Chip {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    let name = entry_name(entry, names)
        .or_else(|| entry.get("short_name").and_then(Value::as_str))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.split('@').next().unwrap_or(""));
    let name = safe_text(name, 24);
    Chip {
        mark: mark_for(id),
        value: (show_value && !name.is_empty()).then(|| headline(entry, window)),
        name,
    }
}

fn headline(entry: &Value, window: UsageWindow) -> String {
    if entry.get("status").and_then(Value::as_str) == Some("error")
        || entry
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| !error.is_empty())
    {
        return "!".into();
    }
    if let Some(metric) = best_metric(entry, window, true) {
        if metric.get("headline").and_then(Value::as_str) == Some("value")
            && let Some(value) = metric.get("value").and_then(Value::as_str)
            && !value.is_empty()
        {
            return safe_text(value, 24);
        }
        if let Some(percent) = metric.get("percent") {
            return format!("{percent}%");
        }
    }
    if let Some(sections) = entry.get("sections").and_then(Value::as_array) {
        for section in sections {
            if section.get("type").and_then(Value::as_str) != Some("text") {
                continue;
            }
            let label = section
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if ["balance", "available", "spend", "prepaid"]
                .iter()
                .any(|term| label.contains(term))
                && let Some(value) = section.get("value").and_then(Value::as_str)
                && !value.is_empty()
            {
                return safe_text(value, 24);
            }
        }
    }
    "Ready".into()
}

fn matches_window(metric: &Value, window: UsageWindow) -> bool {
    if window == UsageWindow::Auto {
        return true;
    }
    let seconds = metric.get("window_secs").and_then(Value::as_u64);
    if let Some(18_000 | 604_800) = seconds {
        return match window {
            UsageWindow::Session => seconds == Some(18_000),
            UsageWindow::Weekly => seconds == Some(604_800),
            _ => false,
        };
    }
    let label = metric
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match window {
        UsageWindow::Session => ["5h", "5-hour", "session", "rolling"]
            .iter()
            .any(|term| label.contains(term)),
        UsageWindow::Weekly => ["week", "7d", "7-day"]
            .iter()
            .any(|term| label.contains(term)),
        UsageWindow::Monthly => ["month", "30d", "30-day", "spend (mo)"]
            .iter()
            .any(|term| label.contains(term)),
        UsageWindow::Auto => true,
    }
}

/// Text fit for the status item: no control or bidi-override characters, at
/// most `limit` characters. Custom names come from the WebView, so this is
/// their sink too.
fn safe_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Marks and overrides that can reorder the text around them.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `title` without custom names, as every report-only case reads it.
    fn title(
        payload: &Value,
        remembered: &str,
        show_all: bool,
        show_value: bool,
        window: UsageWindow,
        visible: Option<&[String]>,
    ) -> String {
        text(&chips(
            payload,
            remembered,
            show_all,
            show_value,
            window,
            visible,
            &BTreeMap::new(),
        ))
    }

    #[test]
    fn custom_names_win_in_the_chips_and_the_tooltip() {
        let report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "sections":[
                {"type":"metric", "label":"Weekly", "percent":21}
            ]},
            {"id":"kilo", "display_name":"Kilo", "status":"ready", "sections":[
                {"type":"metric", "label":"Weekly", "percent":5}
            ]}
        ]});
        let names = BTreeMap::from([("anthropic".to_string(), "Work \u{202e}Claude".to_string())]);
        let shown = chips(&report, "", true, true, UsageWindow::Auto, None, &names);
        assert_eq!(shown[0].name, "Work Claude");
        assert_eq!(shown[0].value.as_deref(), Some("21%"));
        assert!(shown[0].mark.is_some());
        // No bundled mark: the name is what the status item shows.
        assert_eq!(shown[1].mark, None);
        assert_eq!(shown[1].text(), "Kilo 5%");
        assert_eq!(text(&shown), "Work Claude 21%   Kilo 5%");
        assert!(
            tooltip(&report, "", true, UsageWindow::Auto, None, &names)
                .starts_with("Work Claude · 21%")
        );
        // Hidden values leave the mark alone, or the name without a mark.
        let bare = chips(&report, "", true, false, UsageWindow::Auto, None, &names);
        assert_eq!(bare[0].value, None);
        assert_eq!(bare[1].text(), "Kilo");
    }

    #[test]
    fn marks_follow_the_vendor_slug_and_its_aliases() {
        assert!(mark_for("openai@work").is_some());
        assert_eq!(mark_for("supergrok"), mark_for("grok"));
        assert_eq!(mark_for("opencode-go"), mark_for("opencode_go"));
        assert!(mark_for("opencode-go").is_some());
        assert_eq!(mark_for("kilo"), None);
        assert_eq!(mark_for(""), None);
    }

    #[test]
    fn the_mark_table_matches_the_popover_icons_and_aliases() {
        // Repo files, not user state: the popover's icon folder and alias map.
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/windows/popover/src/icons/providers"
        );
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .expect("popover icons")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".svg").map(str::to_owned)
            })
            .collect();
        files.sort();
        let mut marks: Vec<String> = PROVIDER_MARKS
            .iter()
            .map(|(n, _)| (*n).to_owned())
            .collect();
        marks.sort();
        assert_eq!(marks, files);

        let model = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/windows/popover/src/model.js"
        ))
        .expect("model.js");
        let line = model
            .lines()
            .find(|line| line.starts_with("const ICON_ALIAS = "))
            .expect("ICON_ALIAS");
        for (from, to) in MARK_ALIASES {
            assert!(
                line.contains(&format!("\"{from}\": \"{to}\""))
                    || line.contains(&format!("{from}: \"{to}\"")),
                "{from} -> {to} missing from {line}"
            );
        }
        assert_eq!(line.matches(':').count(), MARK_ALIASES.len());
    }

    #[test]
    fn remembers_a_provider_and_cycles_in_report_order() {
        let report = json!({"primary":"claude", "entries":[
            {"id":"claude", "short_name":"cld"},
            {"id":"openai@work", "short_name":"cdx"},
            {"id":"cursor", "short_name":"cur"}
        ]});
        assert_eq!(
            selected_id(&report, "openai@work", UsageWindow::Auto, None),
            Some("openai@work")
        );
        assert_eq!(
            next_id(&report, "openai@work", UsageWindow::Auto, None),
            Some("cursor".into())
        );
        assert_eq!(
            next_id(&report, "cursor", UsageWindow::Auto, None),
            Some("claude".into())
        );
        assert_eq!(
            selected_id(&report, "missing", UsageWindow::Auto, None),
            Some("claude")
        );
    }

    #[test]
    fn pinned_window_falls_back_and_value_headlines_stay_values() {
        let report = json!({"primary":"openai", "entries":[
            {"id":"openai", "short_name":"cdx", "sections":[
                {"type":"metric", "label":"5h", "percent":20, "window_secs":18000},
                {"type":"metric", "label":"Weekly", "percent":80, "window_secs":604800}
            ]},
            {"id":"openrouter", "short_name":"opr", "sections":[
                {"type":"metric", "label":"Balance", "percent":40, "headline":"value", "value":"$12.50"}
            ]}
        ]});
        assert_eq!(
            title(&report, "", false, true, UsageWindow::Auto, None),
            "cdx 80%"
        );
        assert_eq!(
            title(&report, "", false, true, UsageWindow::Session, None),
            "cdx 20%"
        );
        assert_eq!(
            title(
                &report,
                "openrouter",
                false,
                true,
                UsageWindow::Weekly,
                None
            ),
            "opr $12.50"
        );
        assert_eq!(
            title(&report, "", true, true, UsageWindow::Auto, None),
            "cdx 80%   opr $12.50"
        );
    }

    #[test]
    fn default_strip_shows_ready_providers_and_skips_disabled_failures() {
        let report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "sections":[
                {"type":"metric", "label":"Session (5h)", "percent":17},
                {"type":"metric", "label":"Weekly (7d)", "percent":21}
            ]},
            {"id":"openai", "display_name":"Codex", "status":"ready", "sections":[
                {"type":"metric", "label":"Codex weekly", "percent":15}
            ]},
            {"id":"zai", "display_name":"Z.AI", "status":"error", "sections":[]}
        ]});
        assert_eq!(
            title(&report, "", true, true, UsageWindow::Auto, None),
            "Claude 21%   Codex 15%"
        );
        assert_eq!(
            title(&report, "", true, true, UsageWindow::Session, None),
            "Claude 17%   Codex 15%"
        );
        assert_eq!(
            title(&report, "zai", false, true, UsageWindow::Auto, None),
            "Z.AI !"
        );
    }

    #[test]
    fn highest_consumption_tracks_window_and_enabled_providers() {
        let mut report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "sections":[
                {"type":"metric", "label":"Session", "percent":70, "window_secs":18000},
                {"type":"metric", "label":"Weekly", "percent":20, "window_secs":604800}
            ]},
            {"id":"openai", "display_name":"Codex", "status":"ready", "sections":[
                {"type":"metric", "label":"Session", "percent":30, "window_secs":18000},
                {"type":"metric", "label":"Weekly", "percent":80, "window_secs":604800}
            ]},
            {"id":"zai", "display_name":"Z.AI", "status":"error", "sections":[
                {"type":"metric", "label":"Session", "percent":99, "window_secs":18000}
            ]}
        ]});
        assert_eq!(
            title(
                &report,
                HIGHEST_PROVIDER,
                false,
                true,
                UsageWindow::Session,
                None
            ),
            "Claude 70%"
        );
        assert_eq!(
            title(
                &report,
                HIGHEST_PROVIDER,
                false,
                true,
                UsageWindow::Weekly,
                None
            ),
            "Codex 80%"
        );
        assert_eq!(
            title(&report, "anthropic", false, true, UsageWindow::Weekly, None),
            "Claude 20%"
        );

        let visible = vec!["anthropic".into()];
        assert_eq!(
            title(
                &report,
                HIGHEST_PROVIDER,
                false,
                true,
                UsageWindow::Weekly,
                Some(&visible)
            ),
            "Claude 20%"
        );
        report["entries"][0]["sections"][1]["percent"] = json!(90);
        assert_eq!(
            title(
                &report,
                HIGHEST_PROVIDER,
                false,
                true,
                UsageWindow::Weekly,
                None
            ),
            "Claude 90%"
        );
    }
}
