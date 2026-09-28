//! Characterization of the report seam before PLAN 1.2 moves enumeration and
//! projection into `core::entries` and `core::sections`.
//!
//! Everything here is built from a synthetic `Config`, `tabs_from_config` and
//! fixed `TabState`s at one instant — no config file, credentials, cache,
//! Desktop profile store or network. It deliberately does not repeat what
//! other tests already own:
//!
//! | Behaviour                                              | Owned by               |
//! |--------------------------------------------------------|------------------------|
//! | Per-vendor rows, footnotes, headlines, severities      | `core::sections` tests |
//! | Tab expansion, default-tab suppression, Desktop merge  | `core::entries` tests  |
//! | Slug, display name, short code and glyph tables        | `vendor` tests         |
//! | Primary resolution and schema version                  | `report::tests`        |
//! | IPC id bounds and the tray payload wrapper             | `tray::{ipc,payload}`  |
//!
//! Added here: the identity of every source a config can enumerate, the
//! Desktop account identity, identity sanitization, the whole GUI document as
//! a golden file, and the equality of a targeted refresh with the full report.

use super::*;
use crate::balance::DisplayPrefs;
use crate::config::{AnthropicAccount, CustomProviderConfig, OpenAiAccount, OpenRouterAccount};
use crate::core::entries::{ReadyTab, tabs_from_config};
use crate::custom::types::{CustomMetric, CustomSnapshot, CustomText};
use crate::usage::{
    AnthropicSnapshot, OpenAiCredits, OpenAiSnapshot, OpenAiSource, OpenRouterSnapshot,
    ResetCredit, ResetCredits, UsageWindow, VendorSnapshot,
};
use crate::vendor::VendorId;

/// `collect_json`'s document for [`gui_config`] with every `refresh_one`
/// replaced by [`gui_state`]. Update it only for a deliberate contract change.
const GUI_REPORT: &str = include_str!("../tests/fixtures/report/gui_report.json");

/// The instant every projection is taken at; rows count down from here.
fn now() -> DateTime<Utc> {
    "2026-09-27T12:00:00Z".parse().expect("fixed instant")
}

fn json(rendered: &str) -> serde_json::Value {
    serde_json::from_str(rendered).expect("the report renders valid JSON")
}

/// Every built-in switched on or off explicitly, so no test depends on which
/// vendors happen to be enabled by default. Parsed from the section names the
/// `config` guard test proves reach each vendor's switch.
fn config_enabling(enabled: &[VendorId]) -> Config {
    let text: String = VendorId::all()
        .iter()
        .map(|vendor| {
            format!(
                "[{}]\nenabled = {}\n",
                vendor.config_section(),
                enabled.contains(vendor)
            )
        })
        .collect();
    toml::from_str(&text).expect("the synthetic config parses")
}

fn claude_account(label: &str) -> AnthropicAccount {
    AnthropicAccount {
        label: label.into(),
        credentials_path: format!("/synthetic/{label}/.credentials.json").into(),
    }
}

fn openrouter_account(label: &str) -> OpenRouterAccount {
    OpenRouterAccount {
        label: label.into(),
        api_key_env: None,
        api_key: None,
    }
}

fn custom(id: &str, name: &str, short_name: &str, brand: Option<&str>) -> CustomProviderConfig {
    CustomProviderConfig {
        id: id.into(),
        name: name.into(),
        short_name: short_name.into(),
        brand: brand.map(str::to_string),
        enabled: true,
        ..Default::default()
    }
}

/// Every source a config can enumerate — all built-ins, named Claude, Codex
/// and OpenRouter accounts, and `[[custom]]` providers — keeps the id, names,
/// tag and position a frontend stores preferences and notifications under.
/// Each id also addresses exactly its own tab, which a targeted refresh needs.
#[test]
fn every_configured_source_keeps_its_identity_and_position() {
    let mut config = config_enabling(VendorId::all());
    config.anthropic.accounts = vec![claude_account("work"), claude_account("personal")];
    config.openai.accounts.push(OpenAiAccount {
        label: "work".into(),
        codex_auth_path: "/synthetic/codex-work/auth.json".into(),
    });
    config.openrouter.accounts.push(openrouter_account("team"));
    config.custom = vec![
        custom("mytool", "My Tool", "myt", Some("opencode-go")),
        custom("other", "Other Tool", "oth", None),
    ];

    let tabs = tabs_from_config(&config);
    let failed = TabState::error("synthetic");
    let entries: Vec<Entry> = tabs
        .iter()
        .map(|tab| entry_from_state_with_config(&config, tab, &failed, now()))
        .collect();
    let report = json(&render_json_entries(&entries));
    let rows = report["entries"].as_array().unwrap();

    let identity: Vec<(&str, &str, &str, &str)> = rows
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap(),
                row["name"].as_str().unwrap(),
                row["display_name"].as_str().unwrap(),
                row["short_name"].as_str().unwrap(),
            )
        })
        .collect();
    let expected = [
        ("anthropic", "anthropic", "Claude", "cld"),
        ("anthropic@work", "anthropic · work", "Claude · work", "cld"),
        (
            "anthropic@personal",
            "anthropic · personal",
            "Claude · personal",
            "cld",
        ),
        ("anthropic_api", "anthropic_api", "Anthropic API", "aac"),
        ("openai", "openai", "Codex", "gpt"),
        ("openai@work", "openai · work", "Codex · work", "gpt"),
        ("copilot", "copilot", "GitHub Copilot", "ghc"),
        ("zai", "zai", "Z.AI", "zai"),
        ("openrouter", "openrouter", "OpenRouter", "opr"),
        (
            "openrouter@team",
            "openrouter · team",
            "OpenRouter · team",
            "opr",
        ),
        ("deepseek", "deepseek", "DeepSeek", "dsk"),
        ("kimi", "kimi", "Kimi", "kmi"),
        ("kilo", "kilo", "Kilo", "klo"),
        ("novita", "novita", "Novita", "nvt"),
        ("moonshot", "moonshot", "Moonshot", "msh"),
        ("grok", "grok", "Grok", "grk"),
        ("supergrok", "supergrok", "SuperGrok", "sgk"),
        ("grokbot", "grokbot", "Grok Bot", "gbt"),
        ("antigravity", "antigravity", "Antigravity", "agy"),
        ("cursor", "cursor", "Cursor", "cur"),
        ("minimax", "minimax", "MiniMax", "mmx"),
        ("kiro", "kiro", "Kiro", "kir"),
        ("nous", "nous", "Nous Research", "nrs"),
        ("opencode-go", "opencode-go", "OpenCode Go", "ocg"),
        ("commandcode", "commandcode", "Command Code", "cmc"),
        ("ollama", "ollama", "Ollama Cloud", "oll"),
        ("orcarouter", "orcarouter", "OrcaRouter", "orc"),
        ("modelstudio", "modelstudio", "Model Studio", "mst"),
        ("custom:mytool", "My Tool", "My Tool", "myt"),
        ("custom:other", "Other Tool", "Other Tool", "oth"),
    ];
    assert_eq!(identity, expected);

    for (tab, row) in tabs.iter().zip(rows) {
        let id = row["id"].as_str().unwrap();
        assert_eq!(tabs_matching(&tabs, id), vec![tab.clone()], "{id}");
        match tab.vendor_id() {
            // A built-in relays the shared glyph and is its own brand.
            Some(vendor) => {
                assert_eq!(row["icon"], vendor.bar_icon(), "{id}");
                assert_eq!(row["brand"], vendor.slug(), "{id}");
            }
            // A custom provider's tag doubles as its glyph.
            None => assert_eq!(row["icon"], row["short_name"], "{id}"),
        }
    }
    // Only a custom provider that declared a brand carries one.
    assert_eq!(rows[rows.len() - 2]["brand"], "opencode-go");
    assert!(rows[rows.len() - 1].get("brand").is_none());
}

/// A Claude Desktop profile is addressed exactly like a CLI account of the
/// same label — the id does not say where the token comes from — and only the
/// names mark the source. `build_tabs` keeps one tab per label, so the shared
/// id never names two entries (see the Desktop tests in `core::entries`).
#[test]
fn a_desktop_account_shares_the_cli_id_form_and_names_its_source() {
    let desktop = TabId::desktop_account("gmail");
    let entry = entry_from_state(&desktop, &TabState::error("synthetic"), now());

    assert_eq!(entry.id, "anthropic@gmail");
    assert_eq!(entry.id, tab_id(&TabId::account("gmail")));
    assert_eq!(entry.name, "anthropic · gmail (desktop)");
    assert_eq!(entry.display_name, "Claude · gmail (desktop)");
    assert_eq!(entry.short_name, "cld");
    assert_eq!(entry.brand.as_deref(), Some("anthropic"));

    let tabs = [TabId::account("work"), desktop.clone()];
    assert_eq!(tabs_matching(&tabs, "anthropic@gmail"), vec![desktop]);
}

/// Names reach a frontend as text, so they are sanitized where the report
/// builds them. The id is not: it is the key `tabs_matching` compares, so it
/// is relayed exactly as composed from the label.
#[test]
fn identity_names_are_sanitized_while_the_id_stays_verbatim() {
    let failed = TabState::error("synthetic");

    let spec = custom("mytool", "My\u{1b}[31m Tool\u{202e}", "m\u{7}yt", None);
    let tool = entry_from_state(&TabId::custom(&spec), &failed, now());
    assert_eq!(tool.name, "My[31m Tool");
    assert_eq!(tool.display_name, "My[31m Tool");
    assert_eq!(tool.short_name, "myt");
    assert_eq!(tool.icon, "myt");

    let desktop = TabId::desktop_account("gm\u{202e}ail");
    let profile = entry_from_state(&desktop, &failed, now());
    assert_eq!(profile.display_name, "Claude · gmail (desktop)");
    assert_eq!(profile.id, "anthropic@gm\u{202e}ail");
    assert_eq!(
        tabs_matching(std::slice::from_ref(&desktop), &profile.id),
        vec![desktop]
    );
}

/// A default and a named Claude account, Codex, a default and a named
/// OpenRouter account, and a branded `[[custom]]` provider — enumerated by
/// `tabs_from_config`, as `collect_json` does minus the Desktop profile scan.
fn gui_config() -> Config {
    let mut config =
        config_enabling(&[VendorId::Anthropic, VendorId::Openai, VendorId::Openrouter]);
    config.anthropic.accounts.push(claude_account("work"));
    config.openrouter.accounts.push(openrouter_account("team"));
    config.custom = vec![custom("mytool", "My Tool", "myt", Some("opencode-go"))];
    config.ui.primary = Some(VendorId::Anthropic);
    config
}

/// What `refresh_one` would hand the report for each [`gui_config`] tab.
fn gui_state(config: &Config, tab: &TabId) -> TabState {
    let now = now();
    let ready = |snapshot, stale, last_error, fetched_at| {
        TabState::Ready(Box::new(ReadyTab {
            email: None,
            snapshot,
            stale,
            last_error,
            fetched_at,
            // Same split as `refresh_one`: a built-in's configured bar
            // settings, none for a custom provider.
            display: tab
                .vendor_id()
                .map_or_else(DisplayPrefs::default, |vendor| config.display_prefs(vendor)),
        }))
    };
    match tab_id(tab).as_str() {
        // Quota served from cache after a rate-limited refresh.
        "anthropic" => ready(
            VendorSnapshot::Anthropic(AnthropicSnapshot {
                plan: "Max 20x".into(),
                session: UsageWindow {
                    utilization_pct: 29,
                    resets_at: Some(now + chrono::Duration::minutes(50)),
                    window_duration: chrono::Duration::hours(5),
                },
                weekly: UsageWindow {
                    utilization_pct: 32,
                    resets_at: Some(now + chrono::Duration::hours(98)),
                    window_duration: chrono::Duration::days(7),
                },
                sonnet: None,
                scoped: Vec::new(),
                extra: None,
            }),
            true,
            Some((429, "rate limited".to_string())),
            Some(now - chrono::Duration::minutes(15)),
        ),
        // A failed refresh that still knows the plan from the OAuth blob.
        "anthropic@work" => TabState::error_with_plan(
            "HTTP 401: authentication rejected",
            Some("Claude Max 5x".into()),
        ),
        // No weekly window, a zero credit balance, and one banked reset. The
        // reset carries no expiry: a stated one renders in local time.
        "openai" => ready(
            VendorSnapshot::Openai(OpenAiSnapshot {
                plan: "ChatGPT Pro".into(),
                session: Some(UsageWindow {
                    utilization_pct: 12,
                    resets_at: Some(now + chrono::Duration::hours(3)),
                    window_duration: chrono::Duration::hours(5),
                }),
                weekly: None,
                code_review: None,
                additional_limits: Vec::new(),
                unavailable_models: Vec::new(),
                credits: Some(OpenAiCredits {
                    balance: "$0.00".into(),
                    has_credits: false,
                    unlimited: false,
                    approx_local_messages: None,
                    approx_cloud_messages: None,
                }),
                reset_credits: ResetCredits {
                    available: 1,
                    credits: vec![ResetCredit {
                        title: Some("Full reset".into()),
                        expires_at: None,
                    }],
                },
                source: OpenAiSource::CodexOauth,
            }),
            false,
            None,
            None,
        ),
        // Purchased credits behind a per-key limit.
        "openrouter" => ready(
            VendorSnapshot::Openrouter(OpenRouterSnapshot {
                label: "prod".into(),
                total_credits: 100.0,
                total_usage: 25.0,
                usage_daily: 1.0,
                usage_weekly: 5.0,
                usage_monthly: 25.0,
                is_free_tier: false,
                limit: Some(50.0),
                limit_remaining: Some(24.5),
            }),
            false,
            None,
            None,
        ),
        // A free tier that bought nothing: no denominator to meter against.
        "openrouter@team" => ready(
            VendorSnapshot::Openrouter(OpenRouterSnapshot {
                label: "team-key".into(),
                total_credits: 0.0,
                total_usage: 0.0,
                usage_daily: 0.0,
                usage_weekly: 0.0,
                usage_monthly: 0.0,
                is_free_tier: true,
                limit: None,
                limit_remaining: None,
            }),
            false,
            None,
            None,
        ),
        // No plan, one exact rolling window and one text row.
        "custom:mytool" => ready(
            VendorSnapshot::Custom(CustomSnapshot {
                plan: None,
                metrics: vec![CustomMetric {
                    label: "Session".into(),
                    pct: 40,
                    footnote: "40 of 100".into(),
                    resets_at: Some(now + chrono::Duration::hours(2)),
                    window_secs: Some(18_000),
                }],
                texts: vec![CustomText {
                    label: "Region".into(),
                    value: "eu".into(),
                }],
            }),
            false,
            None,
            None,
        ),
        other => panic!("no synthetic state for {other}"),
    }
}

/// `collect_entries_for` with the network replaced by [`gui_state`].
fn gui_entries(config: &Config, tabs: &[TabId]) -> Vec<Entry> {
    tabs.iter()
        .map(|tab| entry_from_state_with_config(config, tab, &gui_state(config, tab), now()))
        .collect()
}

/// The whole document the tray renders, compared with the golden file. The
/// anchors run first, so a regenerated golden still has to keep what they
/// name: order, primary, the `metrics` view, staleness, error shape, a missing
/// window, both credit headlines and a custom provider's brand and plan.
#[test]
fn the_gui_report_document_matches_its_golden_fixture() {
    let config = gui_config();
    let tabs = tabs_from_config(&config);
    let report = json(&render_json_for_primary(
        &gui_entries(&config, &tabs),
        config.ui.primary.map(|vendor| vendor.slug()),
    ));
    let rows = report["entries"].as_array().unwrap();
    let row = |id: &str| {
        rows.iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("no entry {id}"))
    };

    let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "anthropic",
            "anthropic@work",
            "openai",
            "openrouter",
            "openrouter@team",
            "custom:mytool",
        ]
    );
    assert_eq!(report["primary"], "anthropic");

    // `metrics` is exactly the metric sections, in order, minus the tag.
    for entry in rows {
        let untagged: Vec<serde_json::Value> = entry["sections"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|section| section["type"] == "metric")
            .map(|section| {
                let mut metric = section.clone();
                metric.as_object_mut().unwrap().remove("type");
                metric
            })
            .collect();
        assert_eq!(entry["metrics"], json!(untagged), "{}", entry["id"]);
    }

    // Cached figures after a failed refresh: flagged, stamped in UTC, rolling
    // windows keep their exact length, and the failure is a row of its own.
    let claude = row("anthropic");
    assert_eq!(claude["status"], "ready");
    assert_eq!(claude["stale"], true);
    assert_eq!(claude["fetched_at"], "2026-09-27T11:45:00Z");
    assert_eq!(claude["plan"], "Claude Max 20x");
    assert_eq!(claude["metrics"][0]["reset_at"], "2026-09-27T12:50:00Z");
    assert_eq!(claude["metrics"][0]["window_secs"], 18_000);
    assert_eq!(claude["metrics"][1]["window_secs"], 604_800);
    assert!(
        claude["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| { section["type"] == "text" && section["label"] == "HTTP 429" })
    );

    // A failed entry keeps its plan and invents no rows.
    let work = row("anthropic@work");
    assert_eq!(work["status"], "error");
    assert_eq!(work["plan"], "Claude Max 5x");
    assert_eq!(work["sections"], json!([]));
    assert_eq!(work["metrics"], json!([]));

    // A missing window is absent rather than a zero, a zero credit balance is
    // not shown, and the banked reset travels structured beside its text.
    let codex = row("openai");
    let labels: Vec<&str> = codex["sections"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|section| section["label"].as_str())
        .collect();
    assert_eq!(labels, ["Codex 5h", "Reset credits"]);
    assert_eq!(codex["reset_credits"]["available"], 1);

    // Purchased credits headline the percentage; a free tier, the money.
    assert_eq!(row("openrouter")["metrics"][0]["headline"], "percent");
    let free = &row("openrouter@team")["metrics"][0];
    assert_eq!(free["headline"], "value");
    assert_eq!(free["percent"], 0);
    assert!(free["reset_at"].is_null());
    assert!(free.get("window_secs").is_none());

    // A custom provider relays its borrowed brand, uses its tag as glyph, and
    // reports an empty plan string, not null, when it states no plan.
    let tool = row("custom:mytool");
    assert_eq!(tool["brand"], "opencode-go");
    assert_eq!(tool["icon"], "myt");
    assert_eq!(tool["plan"], "");

    pretty_assertions::assert_eq!(report, json(GUI_REPORT));
}

/// A per-provider refresh (`collect_entry_json`) narrows the same tab list by
/// the id the frontend holds and must return the very row the full report
/// carried, so replacing one card changes nothing else about it.
#[test]
fn a_targeted_refresh_returns_the_row_the_full_report_carried() {
    let config = gui_config();
    let tabs = tabs_from_config(&config);
    let full = json(&render_json_for_primary(&gui_entries(&config, &tabs), None));

    for row in full["entries"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        let matched = tabs_matching(&tabs, id);
        assert_eq!(matched.len(), 1, "{id}");
        let single = json(&render_json_entries(&gui_entries(&config, &matched)));
        assert!(single.get("primary").is_none(), "{id}");
        assert_eq!(single["schema_version"], full["schema_version"], "{id}");
        assert_eq!(single["entries"], json!([row]), "{id}");
    }
}
