//! Typed boundary for the messages the popover WebView posts to the macOS host.
//!
//! Every `window.ipc.postMessage` body passes through [`accept`] before the
//! host acts on it: origin, byte length, JSON shape, enums and bounds are
//! checked here, once. A message that fails any check becomes a
//! [`Rejection`] and does nothing. Checks that need live host state (is this
//! entry still in the report, is this account known) stay in the host.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::menu_bar::UsageWindow;

/// Largest body the host parses. A real `strip` layout is a few KB.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

/// The page's own origin; `build_webview` loads `aiub://localhost/index.html`.
const TRUSTED_ORIGIN: &str = "aiub://localhost/";

/// Why a message was dropped. Only these static reasons are ever logged: never
/// the body, the command name or the parser's message, which can echo input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    UntrustedOrigin,
    Oversized,
    Malformed,
}

impl Rejection {
    pub fn reason(self) -> &'static str {
        match self {
            Self::UntrustedOrigin => "untrusted origin",
            Self::Oversized => "oversized",
            Self::Malformed => "malformed",
        }
    }
}

/// True only for documents served by the embedded `aiub` protocol.
/// The trailing `/` ends the authority, so neither `localhost.evil`, a port
/// nor user info can pass.
pub fn trusted_origin(url: &str) -> bool {
    url.starts_with(TRUSTED_ORIGIN)
}

/// Validate one IPC message posted from `origin`. The length is checked
/// before any parsing.
pub fn accept(origin: &str, body: &str) -> Result<Command, Rejection> {
    if !trusted_origin(origin) {
        return Err(Rejection::UntrustedOrigin);
    }
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(Rejection::Oversized);
    }
    // Serde also reads a tagged enum from an array (`["quit"]`); the popover
    // only ever sends an object.
    if !body.trim_start().starts_with('{') {
        return Err(Rejection::Malformed);
    }
    serde_json::from_str(body).map_err(|_| Rejection::Malformed)
}

/// Every command the popover may send. Unit-like commands are empty struct
/// variants so an extra field is refused rather than ignored.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    Ready {},
    Detect {},
    Refresh {},
    OpenSettings {},
    Close {},
    Quit {},
    ToggleStartup {},
    ResetPanelSize {},
    NextMenuBarProvider {},
    SwitchAccount {
        vendor: AccountText,
        label: AccountText,
    },
    Resize(Measurement),
    SetPinned {
        value: bool,
    },
    RefreshEntry {
        id: EntryId,
    },
    SetShortcut {
        value: ShortcutText,
    },
    SetRefresh {
        minutes: RefreshMinutes,
    },
    SetNotificationsEnabled {
        value: bool,
    },
    SetNotificationsThreshold {
        value: Percent,
    },
    SetMenuBarProvider {
        value: EntryId,
    },
    SetMenuBarShowAll {
        value: bool,
    },
    SetMenuBarHideValue {
        value: bool,
    },
    SetMenuBarWindow {
        value: WindowChoice,
    },
    SetMenuBarChart {
        value: bool,
    },
    SetMenuBarItem(MenuBarItemChange),
    SetMenuBarColorValue {
        value: bool,
    },
    SetMenuBarCentered {
        value: bool,
    },
    SetMenuBarActiveAccountOnly {
        value: bool,
    },
    Strip(StripLayout),
    OpenUrl {
        url: WebUrl,
    },
}

// The popover caps card ids at 180 UTF-16 units and names at 80; these
// bounds sit above them so a legitimate layout is never refused.
const MAX_ID_CHARS: usize = 256;
// normalizeAccounts preserves complete configured labels up to this bound.
const MAX_ACCOUNT_CHARS: usize = 4096;
const MAX_NAME_CHARS: usize = 256;
const MAX_STAR_KEY_CHARS: usize = 512;
const MAX_SHORTCUT_CHARS: usize = 128;
/// The report renders at most 64 cards.
const MAX_CARDS: usize = 128;
const MAX_HEIGHT: f64 = 100_000.0;

fn bounded(text: &str, min: usize, max: usize) -> bool {
    let chars = text.chars().count();
    (min..=max).contains(&chars)
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// A report entry id, or `highest` for the menu-bar provider. Membership in
/// the current report is the host's check.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct EntryId(String);

impl EntryId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EntryId {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if bounded(&text, 1, MAX_ID_CHARS) {
            Ok(Self(text))
        } else {
            Err("entry id")
        }
    }
}

/// The focused provider a measurement belongs to; empty when none is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct FocusId(String);

impl TryFrom<String> for FocusId {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if bounded(&text, 0, MAX_ID_CHARS) {
            Ok(Self(text))
        } else {
            Err("focus id")
        }
    }
}

impl PartialEq<&str> for FocusId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// An account vendor or label. The host only acts on one it reported itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct AccountText(pub String);

impl TryFrom<String> for AccountText {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if bounded(&text, 1, MAX_ACCOUNT_CHARS) {
            Ok(Self(text))
        } else {
            Err("account text")
        }
    }
}

/// A shortcut as the recorder spells it; empty clears the shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ShortcutText(pub String);

impl TryFrom<String> for ShortcutText {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if bounded(&text, 0, MAX_SHORTCUT_CHARS) && !has_control(&text) {
            Ok(Self(text))
        } else {
            Err("shortcut")
        }
    }
}

/// An http(s) URL `browse::open` will hand to the default browser.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct WebUrl(pub String);

impl TryFrom<String> for WebUrl {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        match super::browse::http_url(&text) {
            Some(url) if url.len() == text.len() => Ok(Self(text)),
            _ => Err("url"),
        }
    }
}

/// One of `crate::config::TRAY_REFRESH_MINUTES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u64")]
pub struct RefreshMinutes(pub u64);

impl TryFrom<u64> for RefreshMinutes {
    type Error = &'static str;
    fn try_from(minutes: u64) -> Result<Self, Self::Error> {
        if crate::config::TRAY_REFRESH_MINUTES.contains(&minutes) {
            Ok(Self(minutes))
        } else {
            Err("refresh minutes")
        }
    }
}

/// A whole percentage, 1 through 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u64")]
pub struct Percent(pub u8);

impl TryFrom<u64> for Percent {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match u8::try_from(value) {
            Ok(value) if (1..=100).contains(&value) => Ok(Self(value)),
            _ => Err("percent"),
        }
    }
}

/// The menu-bar windows Settings offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowChoice {
    Auto,
    Session,
    Weekly,
    Monthly,
}

impl WindowChoice {
    pub fn usage_window(self) -> UsageWindow {
        match self {
            Self::Auto => UsageWindow::Auto,
            Self::Session => UsageWindow::Session,
            Self::Weekly => UsageWindow::Weekly,
            Self::Monthly => UsageWindow::Monthly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

/// The popover's `Screen` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    About,
    Customize,
    Dashboard,
    Provider,
    Settings,
}

impl Screen {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::About => "about",
            Self::Customize => "customize",
            Self::Dashboard => "dashboard",
            Self::Provider => "provider",
            Self::Settings => "settings",
        }
    }
}

/// Content height in points: finite, positive and below any real screen stack.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "f64")]
pub struct Height(pub f64);

impl TryFrom<f64> for Height {
    type Error = &'static str;
    fn try_from(height: f64) -> Result<Self, Self::Error> {
        if height.is_finite() && height > 0.0 && height <= MAX_HEIGHT {
            Ok(Self(height))
        } else {
            Err("height")
        }
    }
}

impl PartialEq<f64> for Height {
    fn eq(&self, other: &f64) -> bool {
        self.0 == *other
    }
}

/// The panel height `App.tsx` measured for one presentation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub height: Height,
    pub compact: bool,
    pub theme: Theme,
    pub provider: FocusId,
    pub screen: Screen,
    pub revision: u64,
}

impl Measurement {
    pub fn provider(&self) -> &str {
        &self.provider.0
    }
}

/// One provider's menu-bar override from Settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawMenuBarItem")]
pub struct MenuBarItemChange {
    pub id: EntryId,
    pub setting: ItemSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSetting {
    /// `None` is "Same as the Menu Bar": the override is cleared.
    Window(Option<UsageWindow>),
    HideValue(bool),
    Hidden(bool),
    ColorValue(bool),
}

impl ItemSetting {
    /// The `[tray.menu_bar_items.<id>]` key this setting writes.
    pub fn key(self) -> &'static str {
        match self {
            Self::Window(_) => "window",
            Self::HideValue(_) => "hide_value",
            Self::Hidden(_) => "hidden",
            Self::ColorValue(_) => "color_value",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMenuBarItem {
    id: EntryId,
    key: ItemKey,
    value: ItemValue,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ItemKey {
    Window,
    HideValue,
    Hidden,
    ColorValue,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ItemValue {
    Flag(bool),
    Window(WindowChoice),
}

impl TryFrom<RawMenuBarItem> for MenuBarItemChange {
    type Error = &'static str;
    fn try_from(raw: RawMenuBarItem) -> Result<Self, Self::Error> {
        let setting = match (raw.key, raw.value) {
            (ItemKey::Window, ItemValue::Window(WindowChoice::Auto)) => ItemSetting::Window(None),
            (ItemKey::Window, ItemValue::Window(window)) => {
                ItemSetting::Window(Some(window.usage_window()))
            }
            (ItemKey::HideValue, ItemValue::Flag(flag)) => ItemSetting::HideValue(flag),
            (ItemKey::Hidden, ItemValue::Flag(flag)) => ItemSetting::Hidden(flag),
            (ItemKey::ColorValue, ItemValue::Flag(flag)) => ItemSetting::ColorValue(flag),
            _ => return Err("menu bar item value"),
        };
        Ok(Self {
            id: raw.id,
            setting,
        })
    }
}

/// The popover's card layout, as `stripCommand` builds it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawStrip")]
pub struct StripLayout {
    pub stars: BTreeMap<String, Vec<String>>,
    pub order: Vec<String>,
    pub names: BTreeMap<String, String>,
    pub language: String,
    /// (yellow, red) percentages used.
    pub thresholds: (f64, f64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStrip {
    #[allow(dead_code)] // Only "bars" parses; the strip has no other style.
    style: StripStyleName,
    stars: UniqueMap<EntryId, Vec<String>>,
    order: Vec<EntryId>,
    names: UniqueMap<EntryId, String>,
    language: Language,
    thresholds: UniqueMap<ThresholdKey, Percent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum StripStyleName {
    Bars,
}

#[derive(Deserialize)]
enum Language {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ThresholdKey {
    Yellow,
    Red,
}

impl TryFrom<RawStrip> for StripLayout {
    type Error = &'static str;
    fn try_from(raw: RawStrip) -> Result<Self, Self::Error> {
        if raw.order.len() > MAX_CARDS {
            return Err("too many cards");
        }
        let mut order: Vec<String> = Vec::with_capacity(raw.order.len());
        for id in raw.order {
            if order.contains(&id.0) {
                return Err("duplicate card");
            }
            order.push(id.0);
        }
        let mut stars = BTreeMap::new();
        for (EntryId(id), keys) in raw.stars.0 {
            if keys.is_empty() || keys.len() > super::strip::MAX_STARS_PER_PROVIDER {
                return Err("star count");
            }
            for (at, key) in keys.iter().enumerate() {
                if !bounded(key, 1, MAX_STAR_KEY_CHARS) || keys[..at].contains(key) {
                    return Err("star key");
                }
            }
            stars.insert(id, keys);
        }
        let mut names = BTreeMap::new();
        for (EntryId(id), name) in raw.names.0 {
            if !bounded(&name, 1, MAX_NAME_CHARS) || has_control(&name) {
                return Err("card name");
            }
            names.insert(id, name);
        }
        let threshold = |wanted: ThresholdKey| {
            raw.thresholds
                .0
                .iter()
                .find(|(key, _)| *key == wanted)
                .map(|(_, percent)| percent.0)
        };
        let (Some(yellow), Some(red)) = (
            threshold(ThresholdKey::Yellow),
            threshold(ThresholdKey::Red),
        ) else {
            return Err("thresholds");
        };
        if yellow >= red {
            return Err("thresholds");
        }
        let language = match raw.language {
            Language::En => "en",
            Language::PtBr => "pt-BR",
        };
        Ok(Self {
            stars,
            order,
            names,
            language: language.into(),
            thresholds: (f64::from(yellow), f64::from(red)),
        })
    }
}

/// A JSON object (never an array) that refuses repeated keys and more than
/// `MAX_CARDS` entries; each key is validated by `K`. Serde's own maps keep
/// the last duplicate silently.
struct UniqueMap<K, V>(Vec<(K, V)>);

impl<'de, K, V> Deserialize<'de> for UniqueMap<K, V>
where
    K: Deserialize<'de> + PartialEq,
    V: Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<K, V>(std::marker::PhantomData<(K, V)>);

        impl<'de, K, V> serde::de::Visitor<'de> for Entries<K, V>
        where
            K: Deserialize<'de> + PartialEq,
            V: Deserialize<'de>,
        {
            type Value = UniqueMap<K, V>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object without repeated keys")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let mut entries: Vec<(K, V)> = Vec::new();
                while let Some(key) = map.next_key::<K>()? {
                    if entries.len() == MAX_CARDS {
                        return Err(A::Error::custom("too many entries"));
                    }
                    if entries.iter().any(|(seen, _)| *seen == key) {
                        return Err(A::Error::custom("repeated key"));
                    }
                    let value = map.next_value()?;
                    entries.push((key, value));
                }
                Ok(UniqueMap(entries))
            }
        }

        deserializer.deserialize_map(Entries(std::marker::PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const ORIGIN: &str = "aiub://localhost/index.html";

    fn parse(value: Value) -> Result<Command, Rejection> {
        accept(ORIGIN, &value.to_string())
    }

    fn rejected(value: Value) {
        assert_eq!(
            parse(value.clone()),
            Err(Rejection::Malformed),
            "must reject {value}"
        );
    }

    fn id(text: &str) -> EntryId {
        EntryId::try_from(text.to_owned()).unwrap()
    }

    #[test]
    fn origin_is_the_embedded_page_only() {
        assert!(trusted_origin("aiub://localhost/index.html"));
        assert!(trusted_origin("aiub://localhost/"));
        for url in [
            "https://aiub.localhost/index.html",
            "http://localhost/index.html",
            "https://status.anthropic.com/",
            "aiub://localhost.evil.example/index.html",
            "aiub://user@localhost/index.html",
            "aiub://localhost:8080/index.html",
            "aiub://localhost",
            "about:blank",
            "about:srcdoc",
            "data:text/html,<p>x</p>",
            "blob:aiub://localhost/0f7c",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "AIUB://localhost/index.html",
            "",
        ] {
            assert!(!trusted_origin(url), "{url} must be denied");
        }
    }

    #[test]
    fn messages_from_another_origin_are_refused_before_parsing() {
        assert_eq!(
            accept("https://evil.example/", r#"{"cmd":"quit"}"#),
            Err(Rejection::UntrustedOrigin)
        );
    }

    #[test]
    fn length_is_checked_before_json() {
        let padding = " ".repeat(MAX_MESSAGE_BYTES);
        let body = format!(r#"{{"cmd":"quit"}}{padding}"#);
        assert_eq!(accept(ORIGIN, &body), Err(Rejection::Oversized));
        // Not even valid JSON: the length refusal must come first.
        assert_eq!(
            accept(ORIGIN, &"[".repeat(MAX_MESSAGE_BYTES + 1)),
            Err(Rejection::Oversized)
        );
        assert_eq!(accept(ORIGIN, r#"{"cmd":"quit"}"#), Ok(Command::Quit {}));
    }

    #[test]
    fn simple_commands_the_popover_sends_are_accepted() {
        for (cmd, expected) in [
            ("ready", Command::Ready {}),
            ("detect", Command::Detect {}),
            ("refresh", Command::Refresh {}),
            ("open-settings", Command::OpenSettings {}),
            ("close", Command::Close {}),
            ("quit", Command::Quit {}),
            ("toggle-startup", Command::ToggleStartup {}),
            ("reset-panel-size", Command::ResetPanelSize {}),
            ("next-menu-bar-provider", Command::NextMenuBarProvider {}),
        ] {
            assert_eq!(parse(json!({ "cmd": cmd })), Ok(expected), "{cmd}");
        }
    }

    #[test]
    fn junk_shapes_are_malformed() {
        for body in [
            "",
            "null",
            "true",
            "42",
            r#""quit""#,
            r#"["quit"]"#,
            r#"["set-pinned", true]"#,
            r#"  ["quit"]"#,
            "{}",
            r#"{"cmd":""}"#,
            r#"{"cmd":null}"#,
            r#"{"cmd":7}"#,
            r#"{"cmd":"hijack"}"#,
            r#"{"cmd":"QUIT"}"#,
            r#"{"cmd":"quit""#,
            r#"{"cmd":"quit"} {"cmd":"quit"}"#,
        ] {
            assert_eq!(accept(ORIGIN, body), Err(Rejection::Malformed), "{body}");
        }
    }

    #[test]
    fn unknown_or_duplicate_fields_are_refused() {
        rejected(json!({"cmd": "quit", "force": true}));
        rejected(json!({"cmd": "set-pinned", "value": true, "extra": 1}));
        rejected(json!({"cmd": "open-url", "url": "https://x.example/", "tab": "new"}));
        assert_eq!(
            accept(ORIGIN, r#"{"cmd":"refresh","cmd":"quit"}"#),
            Err(Rejection::Malformed)
        );
        assert_eq!(
            accept(ORIGIN, r#"{"cmd":"set-pinned","value":true,"value":false}"#),
            Err(Rejection::Malformed)
        );
    }

    #[test]
    fn boolean_settings_need_a_real_boolean() {
        assert_eq!(
            parse(json!({"cmd": "set-pinned", "value": true})),
            Ok(Command::SetPinned { value: true })
        );
        assert_eq!(
            parse(json!({"cmd": "set-menu-bar-centered", "value": false})),
            Ok(Command::SetMenuBarCentered { value: false })
        );
        for cmd in [
            "set-pinned",
            "set-notifications-enabled",
            "set-menu-bar-show-all",
            "set-menu-bar-hide-value",
            "set-menu-bar-chart",
            "set-menu-bar-color-value",
            "set-menu-bar-centered",
            "set-menu-bar-active-account-only",
        ] {
            // A missing or mistyped flag used to read as `false`.
            rejected(json!({ "cmd": cmd }));
            rejected(json!({ "cmd": cmd, "value": null }));
            rejected(json!({ "cmd": cmd, "value": "true" }));
            rejected(json!({ "cmd": cmd, "value": 1 }));
        }
    }

    #[test]
    fn refresh_minutes_are_the_offered_choices() {
        for minutes in [1u64, 5, 10] {
            assert_eq!(
                parse(json!({"cmd": "set-refresh", "minutes": minutes})),
                Ok(Command::SetRefresh {
                    minutes: RefreshMinutes(minutes)
                })
            );
        }
        for minutes in [
            json!(0),
            json!(2),
            json!(60),
            json!(-5),
            json!(5.5),
            json!("5"),
            json!(null),
        ] {
            rejected(json!({"cmd": "set-refresh", "minutes": minutes}));
        }
        rejected(json!({"cmd": "set-refresh"}));
    }

    #[test]
    fn notification_threshold_is_a_whole_percent() {
        assert_eq!(
            parse(json!({"cmd": "set-notifications-threshold", "value": 1})),
            Ok(Command::SetNotificationsThreshold { value: Percent(1) })
        );
        assert_eq!(
            parse(json!({"cmd": "set-notifications-threshold", "value": 100})),
            Ok(Command::SetNotificationsThreshold {
                value: Percent(100)
            })
        );
        for value in [
            json!(0),
            json!(101),
            json!(-1),
            json!(80.5),
            json!("80"),
            json!(1e9),
        ] {
            rejected(json!({"cmd": "set-notifications-threshold", "value": value}));
        }
    }

    #[test]
    fn shortcut_text_is_bounded_and_empty_clears() {
        assert_eq!(
            parse(json!({"cmd": "set-shortcut", "value": "Cmd+Shift+U"})),
            Ok(Command::SetShortcut {
                value: ShortcutText("Cmd+Shift+U".into())
            })
        );
        assert_eq!(
            parse(json!({"cmd": "set-shortcut", "value": ""})),
            Ok(Command::SetShortcut {
                value: ShortcutText(String::new())
            })
        );
        // Missing used to clear the shortcut; it must now do nothing.
        rejected(json!({"cmd": "set-shortcut"}));
        rejected(json!({"cmd": "set-shortcut", "value": null}));
        rejected(json!({"cmd": "set-shortcut", "value": 3}));
        rejected(json!({"cmd": "set-shortcut", "value": "Cmd+\u{1b}[31mU"}));
        rejected(json!({"cmd": "set-shortcut", "value": "K".repeat(MAX_SHORTCUT_CHARS + 1)}));
    }

    #[test]
    fn menu_bar_window_is_one_of_the_offered_windows() {
        for (value, window) in [
            ("auto", UsageWindow::Auto),
            ("session", UsageWindow::Session),
            ("weekly", UsageWindow::Weekly),
            ("monthly", UsageWindow::Monthly),
        ] {
            let Ok(Command::SetMenuBarWindow { value: choice }) =
                parse(json!({"cmd": "set-menu-bar-window", "value": value}))
            else {
                panic!("{value} must parse");
            };
            assert_eq!(choice.usage_window(), window);
        }
        // `UsageWindow::parse` turns junk into Auto; the boundary must not.
        for value in [
            json!("daily"),
            json!("Weekly"),
            json!(""),
            json!(1),
            json!(null),
        ] {
            rejected(json!({"cmd": "set-menu-bar-window", "value": value}));
        }
    }

    #[test]
    fn entry_ids_are_bounded_text() {
        assert_eq!(
            parse(json!({"cmd": "refresh-entry", "id": "openai:work@example.test"})),
            Ok(Command::RefreshEntry {
                id: id("openai:work@example.test")
            })
        );
        assert_eq!(
            parse(json!({"cmd": "set-menu-bar-provider", "value": "highest"})),
            Ok(Command::SetMenuBarProvider {
                value: id("highest")
            })
        );
        for value in [
            json!(""),
            json!(null),
            json!(7),
            json!(["openai"]),
            json!("x".repeat(MAX_ID_CHARS + 1)),
        ] {
            rejected(json!({"cmd": "refresh-entry", "id": value}));
            rejected(json!({"cmd": "set-menu-bar-provider", "value": value}));
        }
        rejected(json!({"cmd": "refresh-entry"}));
        assert_eq!(id("openai:work").as_str(), "openai:work");
    }

    #[test]
    fn menu_bar_item_matches_the_value_type_to_its_key() {
        let item = |key: &str, value: Value| {
            parse(json!({"cmd": "set-menu-bar-item", "id": "openai", "key": key, "value": value}))
        };
        assert_eq!(
            item("hidden", json!(true)),
            Ok(Command::SetMenuBarItem(MenuBarItemChange {
                id: id("openai"),
                setting: ItemSetting::Hidden(true)
            }))
        );
        assert_eq!(
            item("hide_value", json!(false)),
            Ok(Command::SetMenuBarItem(MenuBarItemChange {
                id: id("openai"),
                setting: ItemSetting::HideValue(false)
            }))
        );
        assert_eq!(
            item("color_value", json!(true)),
            Ok(Command::SetMenuBarItem(MenuBarItemChange {
                id: id("openai"),
                setting: ItemSetting::ColorValue(true)
            }))
        );
        assert_eq!(
            item("window", json!("weekly")),
            Ok(Command::SetMenuBarItem(MenuBarItemChange {
                id: id("openai"),
                setting: ItemSetting::Window(Some(UsageWindow::Weekly))
            }))
        );
        // "Same as the Menu Bar" clears the override, as before.
        assert_eq!(
            item("window", json!("auto")),
            Ok(Command::SetMenuBarItem(MenuBarItemChange {
                id: id("openai"),
                setting: ItemSetting::Window(None)
            }))
        );
        // Each setting writes the config key the popover named.
        for (setting, key) in [
            (ItemSetting::Window(None), "window"),
            (ItemSetting::HideValue(true), "hide_value"),
            (ItemSetting::Hidden(false), "hidden"),
            (ItemSetting::ColorValue(true), "color_value"),
        ] {
            assert_eq!(setting.key(), key);
        }
        // Mismatches used to clear the stored override.
        for (key, value) in [
            ("hidden", json!("true")),
            ("hidden", json!(null)),
            ("hide_value", json!(1)),
            ("color_value", json!("weekly")),
            ("window", json!(true)),
            ("window", json!("daily")),
            ("window", json!(null)),
            ("label", json!(true)),
            ("", json!(true)),
        ] {
            assert_eq!(
                item(key, value.clone()),
                Err(Rejection::Malformed),
                "{key}={value}"
            );
        }
        rejected(json!({"cmd": "set-menu-bar-item", "id": "", "key": "hidden", "value": true}));
        rejected(json!({"cmd": "set-menu-bar-item", "key": "hidden", "value": true}));
        rejected(json!({"cmd": "set-menu-bar-item", "id": "openai", "key": "hidden"}));
        rejected(
            json!({"cmd": "set-menu-bar-item", "id": "openai", "key": "hidden", "value": true, "x": 1}),
        );
    }

    #[test]
    fn account_switch_preserves_labels_the_frontend_accepts() {
        // normalizeAccounts keeps complete labels up to 4096 UTF-16 units.
        // The boundary must not silently reject a configured label the UI sends.
        let label = "w".repeat(4096);
        assert_eq!(
            parse(json!({"cmd": "switch-account", "vendor": "openai", "label": label})),
            Ok(Command::SwitchAccount {
                vendor: AccountText("openai".into()),
                label: AccountText(label),
            })
        );
    }

    #[test]
    fn account_switch_carries_bounded_strings() {
        assert_eq!(
            parse(json!({"cmd": "switch-account", "vendor": "anthropic", "label": "work"})),
            Ok(Command::SwitchAccount {
                vendor: AccountText("anthropic".into()),
                label: AccountText("work".into())
            })
        );
        rejected(json!({"cmd": "switch-account", "vendor": "anthropic"}));
        rejected(json!({"cmd": "switch-account", "vendor": "anthropic", "label": ""}));
        rejected(json!({"cmd": "switch-account", "vendor": "", "label": "work"}));
        rejected(json!({"cmd": "switch-account", "vendor": "anthropic", "label": 4}));
        rejected(
            json!({"cmd": "switch-account", "vendor": "anthropic", "label": "w".repeat(MAX_ACCOUNT_CHARS + 1)}),
        );
    }

    #[test]
    fn open_url_accepts_only_web_urls() {
        assert_eq!(
            parse(json!({"cmd": "open-url", "url": "https://status.anthropic.com/"})),
            Ok(Command::OpenUrl {
                url: WebUrl("https://status.anthropic.com/".into())
            })
        );
        for url in [
            json!("javascript:alert(1)"),
            json!("file:///etc/passwd"),
            json!("aiub://localhost/index.html"),
            json!("https://ok.example\nhttps://evil"),
            json!(""),
            json!(null),
            json!(format!("https://x.example/{}", "a".repeat(2048))),
        ] {
            rejected(json!({"cmd": "open-url", "url": url}));
        }
    }

    #[test]
    fn retired_terminal_and_update_commands_are_refused() {
        for cmd in [
            "open-tui",
            "check-update",
            "install-update",
            "snooze-update",
            "set-updates",
        ] {
            rejected(json!({ "cmd": cmd }));
        }
        rejected(json!({"cmd": "set-updates", "mode": "notify"}));
    }

    fn resize(extra: Value) -> Value {
        let mut base = json!({
            "cmd": "resize", "height": 512, "compact": false, "theme": "dark",
            "provider": "", "screen": "dashboard", "revision": 3
        });
        for (key, value) in extra.as_object().unwrap() {
            if value.is_null() {
                base.as_object_mut().unwrap().remove(key);
            } else {
                base[key] = value.clone();
            }
        }
        base
    }

    #[test]
    fn resize_is_the_measurement_app_tsx_reports() {
        let Ok(Command::Resize(m)) = parse(resize(json!({}))) else {
            panic!("real resize payload must parse");
        };
        assert_eq!(m.height, 512.0);
        assert!(!m.compact);
        assert_eq!(m.theme, Theme::Dark);
        assert_eq!(m.provider, "");
        assert_eq!(m.screen, Screen::Dashboard);
        assert_eq!(m.revision, 3);
        let Ok(Command::Resize(m)) = parse(resize(
            json!({"height": 317.5, "compact": true, "theme": "light", "provider": "openai", "screen": "provider"}),
        )) else {
            panic!("focused resize must parse");
        };
        assert_eq!(m.provider, "openai");
        assert_eq!(m.provider(), "openai");
        assert_eq!(m.screen.as_str(), "provider");
        for screen in ["about", "customize", "dashboard", "provider", "settings"] {
            assert!(
                parse(resize(json!({ "screen": screen }))).is_ok(),
                "{screen}"
            );
        }
    }

    #[test]
    fn resize_refuses_bad_or_missing_fields() {
        for field in [
            "height", "compact", "theme", "provider", "screen", "revision",
        ] {
            rejected(resize(json!({ field: null })));
        }
        for bad in [
            json!({"height": 0}),
            json!({"height": -10}),
            json!({"height": MAX_HEIGHT + 1.0}),
            json!({"height": 1e308}),
            json!({"height": "512"}),
            json!({"compact": "true"}),
            json!({"theme": "system"}),
            json!({"theme": "Dark"}),
            json!({"screen": "overview"}),
            json!({"revision": -1}),
            json!({"revision": 1.5}),
            json!({"provider": 5}),
            json!({"provider": "p".repeat(MAX_ID_CHARS + 1)}),
            json!({"width": 300}),
        ] {
            rejected(resize(bad));
        }
    }

    fn strip(extra: Value) -> Value {
        let mut base = json!({
            "cmd": "strip",
            "style": "bars",
            "stars": {"anthropic": ["metric:Weekly", "metric:Session"], "openai": ["metric:Codex weekly"]},
            "order": ["openai", "anthropic"],
            "names": {"openai": "Work"},
            "language": "pt-BR",
            "thresholds": {"yellow": 60, "red": 85}
        });
        for (key, value) in extra.as_object().unwrap() {
            if value.is_null() {
                base.as_object_mut().unwrap().remove(key);
            } else {
                base[key] = value.clone();
            }
        }
        base
    }

    #[test]
    fn strip_is_the_layout_strip_command_builds() {
        let Ok(Command::Strip(layout)) = parse(strip(json!({}))) else {
            panic!("real strip payload must parse");
        };
        assert_eq!(
            layout.order,
            vec!["openai".to_string(), "anthropic".to_string()]
        );
        assert_eq!(
            layout.stars["anthropic"],
            vec!["metric:Weekly", "metric:Session"]
        );
        assert_eq!(layout.stars["openai"], vec!["metric:Codex weekly"]);
        assert_eq!(
            layout.names,
            BTreeMap::from([("openai".into(), "Work".into())])
        );
        assert_eq!(layout.language, "pt-BR");
        assert_eq!(layout.thresholds, (60.0, 85.0));
        // Before the first report there are no cards: empty order and names.
        let Ok(Command::Strip(empty)) = parse(strip(
            json!({"stars": {}, "order": [], "names": {}, "language": "en"}),
        )) else {
            panic!("empty layout must parse");
        };
        assert!(empty.order.is_empty() && empty.stars.is_empty() && empty.names.is_empty());
        assert_eq!(empty.language, "en");
    }

    #[test]
    fn strip_refuses_junk_instead_of_dropping_parts() {
        for field in ["style", "stars", "order", "names", "language", "thresholds"] {
            rejected(strip(json!({ field: null })));
        }
        let many: Vec<String> = (0..=MAX_CARDS).map(|i| format!("p{i}")).collect();
        for bad in [
            json!({"style": "text"}),
            json!({"language": "fr"}),
            json!({"thresholds": {"yellow": 90, "red": 80}}),
            json!({"thresholds": {"yellow": 80, "red": 80}}),
            json!({"thresholds": {"yellow": 0, "red": 80}}),
            json!({"thresholds": {"yellow": 60, "red": 101}}),
            json!({"thresholds": {"yellow": 60}}),
            json!({"thresholds": {"yellow": 60, "red": 80, "green": 10}}),
            json!({"thresholds": [60, 80]}),
            json!({"order": ["openai", "openai"]}),
            json!({"order": ["openai", ""]}),
            json!({"order": ["openai", 3]}),
            json!({"order": "openai"}),
            json!({"order": many}),
            json!({"stars": {"anthropic": ["a", "b", "c"]}}),
            json!({"stars": {"anthropic": ["a", "a"]}}),
            json!({"stars": {"anthropic": []}}),
            json!({"stars": {"anthropic": [""]}}),
            json!({"stars": {"": ["a"]}}),
            json!({"stars": {"anthropic": "a"}}),
            json!({"stars": {"anthropic": ["k".repeat(MAX_STAR_KEY_CHARS + 1)]}}),
            json!({"names": {"openai": ""}}),
            json!({"names": {"openai": 3}}),
            json!({"names": {"": "Work"}}),
            json!({"names": {"openai": "W\u{7}ork"}}),
            json!({"names": {"openai": "n".repeat(MAX_NAME_CHARS + 1)}}),
            json!({"extra": true}),
        ] {
            rejected(strip(bad));
        }
        let many_names: serde_json::Map<String, Value> = (0..=MAX_CARDS)
            .map(|i| (format!("p{i}"), json!("n")))
            .collect();
        rejected(strip(json!({ "names": many_names })));
    }

    #[test]
    fn strip_maps_refuse_duplicate_keys() {
        let body = r#"{"cmd":"strip","style":"bars","stars":{},"order":[],
            "names":{"openai":"Work","openai":"Home"},"language":"en",
            "thresholds":{"yellow":60,"red":85}}"#;
        assert_eq!(accept(ORIGIN, body), Err(Rejection::Malformed));
        let body = r#"{"cmd":"strip","style":"bars","stars":{"a":["x"],"a":["y"]},
            "order":[],"names":{},"language":"en","thresholds":{"yellow":60,"red":85}}"#;
        assert_eq!(accept(ORIGIN, body), Err(Rejection::Malformed));
        let body = r#"{"cmd":"strip","style":"bars","stars":{},"order":[],"names":{},
            "language":"en","thresholds":{"yellow":60,"red":85,"red":95}}"#;
        assert_eq!(accept(ORIGIN, body), Err(Rejection::Malformed));
    }

    #[test]
    fn rejection_reasons_never_carry_input() {
        for rejection in [
            Rejection::UntrustedOrigin,
            Rejection::Oversized,
            Rejection::Malformed,
        ] {
            assert!(!rejection.reason().is_empty());
        }
    }
}
