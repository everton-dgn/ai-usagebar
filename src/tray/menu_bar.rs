//! Omarchy-style provider summary for the macOS status item.
//!
//! The report owns provider names and metric values. This module only chooses
//! the visible entry and quota window, so the native host can repaint without
//! asking the WebView to be open.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::MenuBarItemConfig;

pub const HIGHEST_PROVIDER: &str = "highest";

/// Space between two providers in the plain-text menu bar the tests read.
#[cfg(test)]
pub const CHIP_GAP: &str = "     ";

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

/// How full a quota is, as the popover's bar colors read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Green,
    Yellow,
    Red,
}

/// Where a bar turns yellow and red, in percent used; the popover's
/// `DEFAULT_COLOR_THRESHOLDS`.
pub const DEFAULT_THRESHOLDS: (f64, f64) = (70.0, 85.0);

/// One provider in the menu bar: its mark when one is bundled, its name for
/// the tooltip and for when the mark cannot be drawn, and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub id: String,
    /// The value comes from the cache after a failed refresh.
    pub stale: bool,
    /// The value's color, when it is a percentage and coloring is on.
    pub level: Option<Level>,
    pub mark: Option<&'static str>,
    pub name: String,
    pub value: Option<String>,
}

impl Chip {
    #[cfg(test)]
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
    select_from(
        payload,
        &eligible_entries(payload, visible),
        remembered,
        window,
    )
}

/// The remembered entry, else the highest usage in `window`, else the
/// report's primary, else the first ready entry, chosen among `entries`.
fn select_from<'a>(
    payload: &'a Value,
    entries: &[&'a Value],
    remembered: &str,
    window: UsageWindow,
) -> Option<&'a str> {
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
    for entry in entries {
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

/// What the menu bar shows and how: the `[tray]` settings, each provider's
/// overrides, and the popover's card order and custom titles.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub remembered: &'a str,
    pub show_all: bool,
    pub show_value: bool,
    pub window: UsageWindow,
    /// The popover's visible cards in order; `None` before it reports them.
    pub visible: Option<&'a [String]>,
    /// The popover's custom card titles, which win over the report's names.
    pub names: &'a BTreeMap<String, String>,
    pub items: &'a BTreeMap<String, MenuBarItemConfig>,
    /// With several accounts of one provider, only the one in use shows.
    pub active_account_only: bool,
    pub color_value: bool,
    /// Yellow and red thresholds in percent used.
    pub thresholds: (f64, f64),
}

impl View<'_> {
    /// The quota window a provider's chip reads.
    pub fn window_for(&self, id: &str) -> UsageWindow {
        self.items
            .get(id)
            .and_then(|item| item.window.as_deref())
            .filter(|window| *window != "auto")
            .map_or(self.window, UsageWindow::parse)
    }

    fn show_value_for(&self, id: &str) -> bool {
        self.items
            .get(id)
            .and_then(|item| item.hide_value)
            .map_or(self.show_value, |hide| !hide)
    }

    fn color_value_for(&self, id: &str) -> bool {
        self.items
            .get(id)
            .and_then(|item| item.color_value)
            .unwrap_or(self.color_value)
    }

    fn hidden(&self, id: &str) -> bool {
        self.items.get(id).is_some_and(|item| item.hidden)
    }
}

/// The menu bar's providers, in display order.
pub fn chips(payload: &Value, view: &View) -> Vec<Chip> {
    displayed_entries(payload, view)
        .into_iter()
        .map(|entry| chip(entry, view))
        .filter(|chip| chip.mark.is_some() || !chip.name.is_empty())
        .collect()
}

/// The menu bar as plain text: every chip's name and value.
#[cfg(test)]
pub fn text(chips: &[Chip]) -> String {
    chips
        .iter()
        .map(Chip::text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(CHIP_GAP)
}

/// Rendered by the macOS status item only; the Linux test build never calls it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tooltip(payload: &Value, view: &View) -> String {
    let lines: Vec<String> = displayed_entries(payload, view)
        .into_iter()
        .map(|entry| {
            let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
            let window = view.window_for(id);
            let name = entry_name(entry, view.names).unwrap_or("AI Usage");
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

fn displayed_entries<'a>(payload: &'a Value, view: &View) -> Vec<&'a Value> {
    let accounts = payload.get("accounts");
    let entries: Vec<&Value> = eligible_entries(payload, view.visible)
        .into_iter()
        .filter(|entry| {
            let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
            !view.hidden(id) && (!view.active_account_only || is_active_account(id, accounts))
        })
        .collect();
    if view.show_all {
        let ready: Vec<&Value> = entries
            .iter()
            .copied()
            .filter(|entry| entry.get("status").and_then(Value::as_str) != Some("error"))
            .collect();
        if !ready.is_empty() {
            return ready;
        }
    }
    let selected = select_from(payload, &entries, view.remembered, view.window);
    entries
        .into_iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) == selected)
        .collect()
}

fn eligible_entries<'a>(payload: &'a Value, visible: Option<&[String]>) -> Vec<&'a Value> {
    let Some(entries) = payload.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(order) = visible else {
        return entries
            .iter()
            .filter(|entry| entry.get("id").is_some())
            .collect();
    };
    order
        .iter()
        .filter_map(|id| {
            entries
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id.as_str()))
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

/// Whether an entry is the account in use for its provider. Providers without
/// switchable accounts always count; with them, the unnamed entry stands for
/// the live login only when no named account holds it.
fn is_active_account(id: &str, accounts: Option<&Value>) -> bool {
    let (vendor, label) = match id.split_once('@') {
        Some((vendor, label)) => (vendor, Some(label)),
        None => (id, None),
    };
    let Some(info) = accounts.and_then(|accounts| accounts.get(vendor)) else {
        return true;
    };
    let active = info
        .get("active")
        .and_then(Value::as_str)
        .filter(|active| !active.is_empty());
    label == active
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
fn chip(entry: &Value, view: &View) -> Chip {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    let window = view.window_for(id);
    let show_value = view.show_value_for(id);
    let name = entry_name(entry, view.names)
        .or_else(|| entry.get("short_name").and_then(Value::as_str))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.split('@').next().unwrap_or(""));
    let name = safe_text(name, 24);
    Chip {
        id: id.to_owned(),
        stale: entry.get("stale").and_then(Value::as_bool) == Some(true),
        level: (show_value && view.color_value_for(id))
            .then(|| used_percent(entry, window))
            .flatten()
            .map(|used| level(used, view.thresholds)),
        mark: mark_for(id),
        value: (show_value && !name.is_empty()).then(|| headline(entry, window)),
        name,
    }
}

/// The percentage the headline shows, when it shows one.
fn used_percent(entry: &Value, window: UsageWindow) -> Option<f64> {
    if !is_ready(entry) {
        return None;
    }
    let metric = best_metric(entry, window, true)?;
    if metric.get("headline").and_then(Value::as_str) == Some("value") {
        return None;
    }
    metric.get("percent")?.as_f64()
}

fn level(used: f64, (yellow, red): (f64, f64)) -> Level {
    if used >= red {
        Level::Red
    } else if used >= yellow {
        Level::Yellow
    } else {
        Level::Green
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

    static NO_NAMES: BTreeMap<String, String> = BTreeMap::new();
    static NO_ITEMS: BTreeMap<String, MenuBarItemConfig> = BTreeMap::new();

    fn view<'a>(
        remembered: &'a str,
        show_all: bool,
        show_value: bool,
        window: UsageWindow,
        visible: Option<&'a [String]>,
    ) -> View<'a> {
        View {
            remembered,
            show_all,
            show_value,
            window,
            visible,
            names: &NO_NAMES,
            items: &NO_ITEMS,
            active_account_only: false,
            color_value: false,
            thresholds: DEFAULT_THRESHOLDS,
        }
    }

    #[test]
    fn values_take_the_bar_colors_per_provider() {
        let report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "sections":[
                {"type":"metric", "label":"Weekly", "percent":90}
            ]},
            {"id":"openai", "display_name":"Codex", "status":"ready", "sections":[
                {"type":"metric", "label":"Weekly", "percent":72}
            ]},
            {"id":"openrouter", "display_name":"OpenRouter", "status":"ready", "sections":[
                {"type":"metric", "label":"Balance", "percent":10, "headline":"value", "value":"$4"}
            ]}
        ]});
        let items = BTreeMap::from([(
            "openai".to_string(),
            MenuBarItemConfig {
                color_value: Some(false),
                ..Default::default()
            },
        )]);
        let view = View {
            color_value: true,
            items: &items,
            ..view("", true, true, UsageWindow::Auto, None)
        };
        let levels: Vec<Option<Level>> =
            chips(&report, &view).into_iter().map(|c| c.level).collect();
        assert_eq!(levels, [Some(Level::Red), None, None]);
        let custom = View {
            items: &NO_ITEMS,
            thresholds: (50.0, 95.0),
            ..view
        };
        let levels: Vec<Option<Level>> = chips(&report, &custom)
            .into_iter()
            .map(|c| c.level)
            .collect();
        assert_eq!(levels, [Some(Level::Yellow), Some(Level::Yellow), None]);
    }

    #[test]
    fn providers_follow_the_popover_card_order() {
        let report = json!({"entries":[
            {"id":"anthropic@conta2", "display_name":"Claude · 2", "status":"ready"},
            {"id":"anthropic@principal", "display_name":"Claude", "status":"ready"},
            {"id":"zai", "display_name":"Z.AI", "status":"ready"}
        ]});
        let order = vec![
            "anthropic@principal".to_string(),
            "zai".into(),
            "anthropic@conta2".into(),
        ];
        let ids: Vec<String> = chips(
            &report,
            &view("", true, false, UsageWindow::Auto, Some(&order)),
        )
        .into_iter()
        .map(|chip| chip.id)
        .collect();
        assert_eq!(ids, ["anthropic@principal", "zai", "anthropic@conta2"]);
    }

    #[test]
    fn a_provider_set_to_auto_follows_the_menu_bar_window() {
        let items = BTreeMap::from([(
            "openai".to_string(),
            MenuBarItemConfig {
                window: Some("auto".into()),
                ..Default::default()
            },
        )]);
        let view = View {
            items: &items,
            ..view("", true, true, UsageWindow::Weekly, None)
        };
        assert_eq!(view.window_for("openai"), UsageWindow::Weekly);
    }

    #[test]
    fn one_provider_mode_picks_among_the_providers_left_after_filters() {
        let report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "stale": true},
            {"id":"openai", "display_name":"Codex", "status":"ready"}
        ]});
        let items = BTreeMap::from([(
            "anthropic".to_string(),
            MenuBarItemConfig {
                hidden: true,
                ..Default::default()
            },
        )]);
        let hidden = View {
            items: &items,
            ..view("anthropic", false, false, UsageWindow::Auto, None)
        };
        let ids: Vec<String> = chips(&report, &hidden).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["openai"]);
        assert!(
            chips(
                &report,
                &view("anthropic", false, false, UsageWindow::Auto, None)
            )[0]
            .stale
        );
    }

    /// The plain-text menu bar with only the `[tray]` settings.
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
            &view(remembered, show_all, show_value, window, visible),
        ))
    }

    #[test]
    fn providers_can_be_hidden_or_read_their_own_window_and_value() {
        let report = json!({"primary":"anthropic", "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready", "sections":[
                {"type":"metric", "label":"Session", "percent":40, "window_secs":18000},
                {"type":"metric", "label":"Weekly", "percent":70, "window_secs":604800}
            ]},
            {"id":"openai", "display_name":"Codex", "status":"ready", "sections":[
                {"type":"metric", "label":"Session", "percent":10, "window_secs":18000},
                {"type":"metric", "label":"Weekly", "percent":30, "window_secs":604800}
            ]},
            {"id":"kilo", "display_name":"Kilo", "status":"ready", "sections":[
                {"type":"metric", "label":"Weekly", "percent":5}
            ]}
        ]});
        let items = BTreeMap::from([
            (
                "anthropic".to_string(),
                MenuBarItemConfig {
                    window: Some("session".into()),
                    ..Default::default()
                },
            ),
            (
                "openai".to_string(),
                MenuBarItemConfig {
                    hide_value: Some(true),
                    ..Default::default()
                },
            ),
            (
                "kilo".to_string(),
                MenuBarItemConfig {
                    hidden: true,
                    ..Default::default()
                },
            ),
        ]);
        let view = View {
            items: &items,
            ..view("", true, true, UsageWindow::Weekly, None)
        };
        let shown = chips(&report, &view);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].value.as_deref(), Some("40%"));
        assert_eq!(shown[1].value, None);
        assert_eq!(view.window_for("openai"), UsageWindow::Weekly);
        assert!(!tooltip(&report, &view).contains("Kilo"));
    }

    #[test]
    fn active_account_only_keeps_the_login_in_use() {
        let report = json!({"primary":"anthropic",
        "accounts": {"anthropic": {"active": "work", "labels": ["work", "home"]}},
        "entries":[
            {"id":"anthropic", "display_name":"Claude", "status":"ready"},
            {"id":"anthropic@work", "display_name":"Claude · work", "status":"ready"},
            {"id":"anthropic@home", "display_name":"Claude · home", "status":"ready"},
            {"id":"openai", "display_name":"Codex", "status":"ready"}
        ]});
        let all = chips(&report, &view("", true, false, UsageWindow::Auto, None));
        assert_eq!(all.len(), 4);
        let active = View {
            active_account_only: true,
            ..view("", true, false, UsageWindow::Auto, None)
        };
        let ids: Vec<String> = chips(&report, &active)
            .into_iter()
            .map(|chip| chip.id)
            .collect();
        assert_eq!(ids, ["anthropic@work", "openai"]);

        // No named account holds the login: the unnamed entry is the one in use.
        let mut report = report;
        report["accounts"]["anthropic"]["active"] = Value::Null;
        let ids: Vec<String> = chips(&report, &active)
            .into_iter()
            .map(|chip| chip.id)
            .collect();
        assert_eq!(ids, ["anthropic", "openai"]);
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
        let named = View {
            names: &names,
            ..view("", true, true, UsageWindow::Auto, None)
        };
        let shown = chips(&report, &named);
        assert_eq!(shown[0].name, "Work Claude");
        assert_eq!(shown[0].value.as_deref(), Some("21%"));
        assert!(shown[0].mark.is_some());
        // No bundled mark: the name is what the status item shows.
        assert_eq!(shown[1].mark, None);
        assert_eq!(shown[1].text(), "Kilo 5%");
        assert_eq!(text(&shown), "Work Claude 21%     Kilo 5%");
        assert!(tooltip(&report, &named).starts_with("Work Claude · 21%"));
        // Hidden values leave the mark alone, or the name without a mark.
        let bare = chips(
            &report,
            &View {
                show_value: false,
                ..named
            },
        );
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
            "cdx 80%     opr $12.50"
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
            "Claude 21%     Codex 15%"
        );
        assert_eq!(
            title(&report, "", true, true, UsageWindow::Session, None),
            "Claude 17%     Codex 15%"
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
