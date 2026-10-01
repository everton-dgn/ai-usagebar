//! NSStatusItem + WKWebView popover. macOS-only.

use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use fs2::FileExt;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ProtocolObject};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent, NSEventMask,
    NSGlassEffectView, NSGlassEffectViewStyle, NSImage, NSImageScaling, NSScreen, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowAnimationBehavior, NSWindowDidEndLiveResizeNotification,
    NSWindowDidResizeNotification, NSWindowOrderingMode, NSWindowWillStartLiveResizeNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect,
    NSSize,
};
use objc2_quartz_core::kCACornerCurveContinuous;
use serde_json::{Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{
    ActivationPolicy, EventLoopExtMacOS, WindowBuilderExtMacOS, WindowExtMacOS,
};
use tao::window::{Window, WindowBuilder};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::http::{Request, Response};
use wry::{NewWindowResponse, WebView, WebViewBuilder, WebViewBuilderExtDarwin};

use super::assets;
use super::browse;
use super::hotkey::{self, HotkeyBinding};
use super::icon::{Severity, tray_icon_rgba};
use super::ipc::{self, Command, ItemSetting, Measurement, WindowChoice};
use super::menu_bar::{self, UsageWindow};
use super::menu_space;
use super::panel::{
    CLICK_LOCK_MS, CORNER_RADIUS, CocoaRect, FALLBACK_WORK_AREA_HEIGHT, HorizontalResize,
    MIN_POPOVER_WIDTH, MIN_USER_HEIGHT, PanelSize, PopoverPlacement, WINDOW_HEIGHT, close_on_blur,
    close_on_outside_click, cocoa_popover_frame, fit_popover_height, fit_provider_popover_height,
    menu_bar_bottom_y,
};
use super::payload::{AccountSwitchFact, HostFacts, host_payload, wrap_report};
use super::startup;
use super::status_items::{self, ItemAction, MenuLine, ProviderItems};
use super::strip::{
    BARS_PIXEL_SIDE, BARS_POINT_SIDE, Stars, StripStyle, bar_fill, bars_layout, bars_rgba,
    content_from_payload,
};
use crate::config::{Config, MenuBarItemConfig};

enum UserEvent {
    Tray(TrayIconEvent),
    Chart(status_items::ChartAction),
    /// The menu bar began (`true`) or ended the chart item's expanded
    /// session: its left click on macOS 27.
    ChartSession(bool),
    /// A popover message that already passed `ipc::accept`.
    Ipc(Command),
    Report(Value),
    Entry(Value),
    FocusPopover,
    Hotkey,
    Facts,
    AccessibilityChanged,
    /// Final native geometry, captured before AppKit exits its resize loop.
    UserResized {
        frame: CocoaRect,
        previous_height: f64,
    },
    /// A mouse press in another app, the menu bar or the desktop, at this
    /// Cocoa screen point and `NSEvent` timestamp.
    OutsideClick(f64, f64, f64),
    /// A click on a provider's own menu-bar item, or a pick in its menu.
    ProviderItem(ItemAction),
}

enum WorkerCmd {
    Refresh,
    RefreshEntry(String),
    Detect,
    Shutdown,
}

type SharedFacts = Arc<Mutex<HostFacts>>;

fn facts_snapshot(facts: &SharedFacts) -> HostFacts {
    facts.lock().map(|f| f.clone()).unwrap_or_default()
}

fn with_facts(facts: &SharedFacts, edit: impl FnOnce(&mut HostFacts)) {
    if let Ok(mut guard) = facts.lock() {
        edit(&mut guard);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Light,
    Dark,
}

impl From<ipc::Theme> for Theme {
    fn from(theme: ipc::Theme) -> Self {
        match theme {
            ipc::Theme::Light => Self::Light,
            ipc::Theme::Dark => Self::Dark,
        }
    }
}

struct TrayState {
    // Unregister before tao drops the window.
    _resize_observer: Option<WindowResizeObserver>,
    window: Window,
    webview: Option<WebView>,
    tray: TrayIcon,
    worker: mpsc::Sender<WorkerCmd>,
    proxy: EventLoopProxy<UserEvent>,
    payload: Value,
    js_ready: bool,
    popover_open: bool,
    blur_guard_until: Option<Instant>,
    /// Cocoa (points, y-up) location of the last click / shortcut, used to
    /// keep the popover on that screen instead of tao's primary-display space.
    last_anchor: Option<(f64, f64)>,
    /// Last CSS/logical height from the `resize` IPC; not derived from
    /// `inner_size / scale_factor`, which is wrong after a scale-factor change.
    popover_height: f64,
    compact_popover: bool,
    /// Width and height cap the user dragged the panel to, saved on close.
    panel_size: PanelSize,
    panel_size_dirty: bool,
    /// When the global monitor last saw a press on the status item. A quick
    /// click is released before the popover's blur arrives, so the blur reads
    /// this instead of the live button state.
    status_item_pressed_at: Option<Instant>,
    /// Set from the panel's pin: blurs and outside clicks leave it open.
    pinned: bool,
    /// Keeps the global mouse monitor alive; dropping it would end it.
    _outside_click_monitor: Option<Retained<AnyObject>>,
    theme: Theme,
    facts: SharedFacts,
    hotkey: Option<HotkeyBinding>,
    strip_style: StripStyle,
    stars: Stars,
    strip_order: Vec<String>,
    strip_order_known: bool,
    /// The popover's custom card titles, for the menu bar and its tooltip.
    strip_names: std::collections::BTreeMap<String, String>,
    menu_bar_provider: String,
    menu_bar_show_all: bool,
    menu_bar_hide_value: bool,
    menu_bar_window: UsageWindow,
    menu_bar_chart: bool,
    menu_bar_items: std::collections::BTreeMap<String, MenuBarItemConfig>,
    menu_bar_active_account_only: bool,
    menu_bar_color_value: bool,
    menu_bar_centered: bool,
    /// The popover's bar-color thresholds, (yellow, red) in percent used.
    color_thresholds: (f64, f64),
    /// The providers' own menu-bar items, left of the chart glyph.
    provider_items: ProviderItems,
    /// The chart item's menu-bar session, where the OS has the API.
    chart_session: Option<status_items::ExpandedSession>,
    /// The provider whose item opened the popover, if one did.
    focused_provider: Option<String>,
    /// The popover's language, for the native provider menus.
    language: String,
    /// The provider whose native menu is open.
    menu_provider: Option<String>,
    /// A provider click is waiting for its tab's height before showing.
    show_pending: bool,
    /// Identifies the render requested by the latest open/close, including a
    /// reopening of the same provider. Old WebKit measurements cannot reveal it.
    presentation_revision: u64,
    presentation_screen: &'static str,
    notifications_enabled: bool,
    notifications_threshold: u8,
}

pub fn run() -> i32 {
    let Some(_lock) = SingleInstance::acquire() else {
        return 0;
    };
    if let Err(error) = run_loop() {
        eprintln!("{error}");
        return 1;
    }
    0
}

fn run_loop() -> Result<(), String> {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = event_loop.create_proxy();

    {
        let proxy = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Tray(event));
        }));
    }
    let panel_size = load_panel_size();
    let window = WindowBuilder::new()
        .with_title("AI Usage")
        .with_inner_size(LogicalSize::new(panel_size.width, WINDOW_HEIGHT))
        .with_min_inner_size(LogicalSize::new(MIN_POPOVER_WIDTH, MIN_USER_HEIGHT))
        .with_visible(false)
        .with_decorations(false)
        .with_always_on_top(true)
        .with_resizable(true)
        .with_focused(false)
        .with_transparent(true)
        .with_has_shadow(true)
        .build(&event_loop)
        .map_err(|error: tao::error::OsError| error.to_string())?;

    let config = Config::load().unwrap_or_default();
    let facts: SharedFacts = Arc::new(Mutex::new(host_facts(&config)));

    let mut hotkey_binding = HotkeyBinding::new().ok();
    {
        let proxy = proxy.clone();
        hotkey::install_press_handler(move |_| {
            let _ = proxy.send_event(UserEvent::Hotkey);
        });
    }
    if let Some(configured) = config.tray.shortcut.as_deref() {
        let outcome = bind_shortcut(hotkey_binding.as_mut(), configured);
        with_facts(&facts, |f| apply_shortcut_outcome(f, outcome));
    }
    let (cmd_tx, cmd_rx) = mpsc::channel();
    spawn_worker(proxy.clone(), cmd_rx, facts.clone());
    let _ = cmd_tx.send(WorkerCmd::Refresh);

    let empty = wrap_report("{}", &facts_snapshot(&facts), now_ms(), None);
    let menu_bar_window =
        UsageWindow::parse(config.tray.menu_bar_window.as_deref().unwrap_or("auto"));
    let menu_bar_chart = config.tray.menu_bar_style.as_deref() == Some("bars");
    let menu_bar_show_all = config.tray.menu_bar_show_all();
    let tray = build_tray()?;

    let theme = Theme::Light;
    let webview = build_webview(&window, proxy.clone()).ok();
    round_corners(&window);
    install_glass_background(&window);

    let resize_observer = install_resize_observer(&window, proxy.clone());
    let chart_session = tray.ns_status_item().and_then(|item| {
        let proxy = proxy.clone();
        status_items::ExpandedSession::attach(&item, move |began| {
            let _ = proxy.send_event(UserEvent::ChartSession(began));
        })
    });
    let mut state = TrayState {
        _resize_observer: resize_observer,
        window,
        webview,
        tray,
        worker: cmd_tx,
        proxy: proxy.clone(),
        payload: empty,
        js_ready: false,
        popover_open: false,
        blur_guard_until: None,
        last_anchor: None,
        popover_height: WINDOW_HEIGHT,
        panel_size,
        panel_size_dirty: false,
        pinned: false,
        status_item_pressed_at: None,
        _outside_click_monitor: install_outside_click_monitor(proxy.clone()),
        theme,
        facts,
        hotkey: hotkey_binding,
        strip_style: StripStyle::Bars,
        stars: Stars::new(),
        strip_order: Vec::new(),
        strip_order_known: false,
        strip_names: Default::default(),
        menu_bar_provider: config
            .tray
            .menu_bar_provider
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| menu_bar::HIGHEST_PROVIDER.into()),
        menu_bar_show_all,
        menu_bar_hide_value: config.tray.menu_bar_hide_value,
        menu_bar_window,
        menu_bar_chart,
        menu_bar_items: config.tray.menu_bar_items.clone(),
        menu_bar_active_account_only: config.tray.menu_bar_active_account_only,
        menu_bar_color_value: config.tray.menu_bar_color_value.unwrap_or(true),
        menu_bar_centered: config.tray.menu_bar_centered,
        color_thresholds: menu_bar::DEFAULT_THRESHOLDS,
        provider_items: {
            let proxy = proxy.clone();
            let mtm = MainThreadMarker::new().ok_or("the tray runs on the main thread")?;
            ProviderItems::new(mtm, move |action| {
                let _ = proxy.send_event(UserEvent::ProviderItem(action));
            })
        },
        chart_session,
        focused_provider: None,
        compact_popover: false,
        language: "en".into(),
        menu_provider: None,
        show_pending: false,
        presentation_revision: 0,
        presentation_screen: "dashboard",
        notifications_enabled: config.notifications.enabled,
        notifications_threshold: config.notifications.threshold,
    };
    apply_strip_icon(&mut state);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Tray(tray_event)) => handle_tray(&mut state, tray_event),
            Event::UserEvent(UserEvent::Chart(action)) => match action {
                status_items::ChartAction::Pressed => {
                    state.status_item_pressed_at = Some(Instant::now())
                }
                status_items::ChartAction::Cancelled => state.status_item_pressed_at = None,
                status_items::ChartAction::Released(button) => {
                    handle_chart_click(&mut state, button);
                    if let Some(button) = MainThreadMarker::new()
                        .and_then(|mtm| state.tray.ns_status_item()?.button(mtm))
                    {
                        status_items::acknowledge_chart_click(&button);
                    }
                    mark_open_item(&state);
                }
            },
            Event::UserEvent(UserEvent::ChartSession(began)) => {
                handle_chart_session(&mut state, began)
            }
            Event::UserEvent(UserEvent::ProviderItem(action)) => {
                let chart = match action {
                    ItemAction::Menu { tag } => chart_menu_command(tag)
                        .map(Some)
                        .or_else(|| chart_global_command(&mut state, tag)),
                    _ => None,
                };
                match chart {
                    Some(Some(command)) => handle_command(&mut state, command, control_flow),
                    // A global pick that could not be saved changes nothing.
                    Some(None) => {}
                    None => handle_provider_item(&mut state, action),
                }
            }
            Event::UserEvent(UserEvent::Ipc(command)) => {
                handle_command(&mut state, command, control_flow);
            }
            Event::UserEvent(UserEvent::Report(payload)) => apply_payload(&mut state, payload),
            Event::UserEvent(UserEvent::Entry(entry)) => apply_entry(&mut state, entry),
            Event::UserEvent(UserEvent::Facts) => apply_facts(&mut state),
            Event::UserEvent(UserEvent::AccessibilityChanged) => apply_strip_icon(&mut state),
            Event::UserEvent(UserEvent::Hotkey) => toggle_popover_from_keyboard(&mut state),
            Event::UserEvent(UserEvent::UserResized {
                frame,
                previous_height,
            }) => {
                note_user_resize(&mut state, frame, previous_height);
            }
            Event::UserEvent(UserEvent::OutsideClick(x, y, timestamp)) => {
                if press_on_open_session_item(&state, x, y, timestamp) {
                    hide_popover(&mut state);
                    return;
                }
                if !status_item_frame(&state.tray).is_some_and(|frame| frame.contains(x, y)) {
                    cancel_chart_press(&state);
                }
                let on_status_item = status_item_frames(&state)
                    .iter()
                    .any(|frame| frame.contains(x, y));
                if on_status_item {
                    state.status_item_pressed_at = Some(Instant::now());
                }
                if close_on_outside_click(
                    state.popover_open || state.show_pending,
                    state.pinned,
                    on_status_item,
                ) {
                    hide_popover(&mut state);
                }
            }
            Event::UserEvent(UserEvent::FocusPopover) => {
                if state.popover_open {
                    guard_blur(&mut state);
                    state.window.set_focus();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => {
                if state.popover_open
                    && !state.pinned
                    && close_on_blur(
                        blur_guarded(&state),
                        press_on_status_item(&state) || status_item_just_pressed(&state),
                    )
                {
                    hide_popover(&mut state);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => hide_popover(&mut state),
            Event::LoopDestroyed => {
                save_panel_size(&mut state);
                let _ = state.worker.send(WorkerCmd::Shutdown);
            }
            _ => {}
        }
    })
}

fn spawn_worker(
    proxy: EventLoopProxy<UserEvent>,
    rx: mpsc::Receiver<WorkerCmd>,
    facts: SharedFacts,
) {
    std::thread::Builder::new()
        .name("ai-usagebar-tray-fetch".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let Ok(rt) = rt else {
                return;
            };
            // Reconcile enabled legacy login items without blocking the window.
            // A missing item stays disabled. Refusals preserve its original file.
            let _ = startup::reconcile();
            if let Ok(mut current) = facts.lock() {
                current.startup_enabled = startup::is_enabled();
            }
            run_detection(false);
            loop {
                rt.block_on(push_report(&proxy, &facts));
                let deadline =
                    Instant::now() + Duration::from_secs(facts_snapshot(&facts).refresh_secs);
                loop {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(WorkerCmd::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Ok(WorkerCmd::Detect) => {
                            run_detection(true);
                            break;
                        }
                        Ok(WorkerCmd::RefreshEntry(id)) => {
                            rt.block_on(push_entry(&proxy, &facts, &id));
                        }
                        Ok(WorkerCmd::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return;
                        }
                    }
                }
            }
        })
        .ok();
}

fn run_detection(force: bool) {
    if let Ok(state_path) = crate::detect::default_state_path() {
        let _ = crate::detect::run_once(None, &state_path, force);
    }
}

fn host_facts(config: &Config) -> HostFacts {
    let mut facts = HostFacts::new(env!("CARGO_PKG_VERSION"), startup::is_enabled());
    facts.refresh_secs = config.tray.refresh_minutes() * 60;
    facts.accounts = account_facts(config);
    facts.account_emails = account_emails(config);
    facts
}

/// Which Claude CLI and Codex logins are active, for the switch control on
/// each account's card. Read fresh on every report, so a switch made from the
/// terminal shows up too.
fn account_facts(config: &Config) -> Vec<AccountSwitchFact> {
    let mut out = Vec::new();
    let claude = config.anthropic.all_accounts();
    if config.anthropic.enabled && !claude.is_empty() {
        let active = crate::anthropic::cli_account::home_claude_json()
            .ok()
            .and_then(|home| crate::anthropic::cli_account::resolve_active_label(&home, &claude));
        out.push(AccountSwitchFact {
            vendor: "anthropic".into(),
            active,
            labels: claude.iter().map(|account| account.label.clone()).collect(),
            ..AccountSwitchFact::default()
        });
    }
    let codex = &config.openai.accounts;
    if config.openai.enabled && !codex.is_empty() {
        let active = config
            .openai
            .resolve_auth_path(None)
            .ok()
            .and_then(|default| crate::openai::account::resolve_active_label(&default, codex));
        out.push(AccountSwitchFact {
            vendor: "openai".into(),
            active,
            labels: codex.iter().map(|account| account.label.clone()).collect(),
            ..AccountSwitchFact::default()
        });
    }
    out
}

/// Local account identity for the dropdown, never written to the usage cache.
fn account_emails(config: &Config) -> std::collections::BTreeMap<String, String> {
    use crate::anthropic::cli_account;
    let mut emails = std::collections::BTreeMap::new();
    if config.openai.enabled {
        let labels = std::iter::once(None).chain(
            config
                .openai
                .accounts
                .iter()
                .map(|account| Some(account.label.as_str())),
        );
        for label in labels {
            if let Some(email) = config
                .openai
                .fetch_auth_path(label)
                .ok()
                .and_then(|path| crate::openai::account::account_email_in(&path))
            {
                let id = label.map_or_else(|| "openai".into(), |label| format!("openai@{label}"));
                emails.insert(id, email);
            }
        }
    }
    if config.anthropic.enabled {
        let mut by_uuid = std::collections::BTreeMap::new();
        let markers = std::iter::once((
            "anthropic".to_string(),
            cli_account::home_claude_json().ok(),
        ))
        .chain(
            config
                .anthropic
                .all_accounts()
                .iter()
                .map(|account| {
                    (
                        format!("anthropic@{}", account.label),
                        Some(cli_account::marker_path(&account.config_dir())),
                    )
                })
                .collect::<Vec<_>>(),
        );
        for (id, marker) in markers {
            if let Some(marker) = marker
                && let Some(email) = cli_account::account_email_in(&marker)
            {
                if let Some(uuid) = cli_account::account_uuid_in(&marker) {
                    by_uuid.insert(uuid, email.clone());
                }
                emails.insert(id, email);
            }
        }
        if let Ok(paths) = crate::claude_desktop::Paths::resolve(&config.anthropic) {
            for profile in crate::claude_desktop::load_profiles(&paths.profiles_dir) {
                if let Some(email) = profile
                    .email
                    .filter(|email| !email.trim().is_empty())
                    .or_else(|| by_uuid.get(&profile.account_uuid).cloned())
                {
                    emails.insert(format!("anthropic@{}", profile.label), email);
                }
            }
        }
    }
    emails
}

/// Replace identities and account facts, preserving pending switches/errors.
fn refresh_account_facts(facts: &SharedFacts) {
    let config = Config::load().unwrap_or_default();
    let fresh = account_facts(&config);
    let emails = account_emails(&config);
    with_facts(facts, |f| {
        f.account_emails = emails;
        f.accounts = fresh
            .into_iter()
            .map(|mut fact| {
                if let Some(old) = f.accounts.iter().find(|old| old.vendor == fact.vendor) {
                    fact.target.clone_from(&old.target);
                    fact.switching = old.switching;
                    fact.error.clone_from(&old.error);
                }
                fact
            })
            .collect();
    });
}

/// Keep credential mutations isolated from the UI and refresh worker. Only a
/// typed, bounded result crosses back into the card; no stderr text is shown.
fn run_account_switch(facts: &SharedFacts, vendor: &str, label: &str) {
    let result = std::env::current_exe()
        .map_err(|_| crate::core::accounts::Failure::WorkerUnavailable)
        .and_then(|tray| super::account_worker::switch_with(&tray, vendor, label));
    let error = result
        .err()
        .map_or_else(String::new, |error| error.message().to_owned());
    with_facts(facts, |f| {
        for fact in f.accounts.iter_mut().filter(|fact| fact.vendor == vendor) {
            fact.switching = false;
            fact.error.clone_from(&error);
        }
    });
}

async fn push_report(proxy: &EventLoopProxy<UserEvent>, facts: &SharedFacts) {
    refresh_account_facts(facts);
    let mut snapshot = facts_snapshot(facts);
    snapshot.startup_enabled = startup::is_enabled();
    let now = now_ms();
    let report = crate::report::collect_json().await;
    refresh_account_facts(facts);
    snapshot.retain_stable_emails(&facts_snapshot(facts));
    let payload = match report {
        Ok(json) => wrap_report(&json, &snapshot, now, None),
        Err(error) => wrap_report("{}", &snapshot, now, Some(&error)),
    };
    let _ = proxy.send_event(UserEvent::Report(payload));
}

async fn push_entry(proxy: &EventLoopProxy<UserEvent>, facts: &SharedFacts, id: &str) {
    let entry = refreshed_entry_with(
        id,
        || {
            refresh_account_facts(facts);
            facts_snapshot(facts)
        },
        crate::report::collect_entry_json(id),
    )
    .await;
    if let Some(entry) = entry {
        let _ = proxy.send_event(UserEvent::Entry(entry));
    }
}

/// Attach only local identities unchanged across the usage request.
async fn refreshed_entry_with(
    id: &str,
    mut refresh_facts: impl FnMut() -> HostFacts,
    report: impl std::future::Future<Output = Result<String, String>>,
) -> Option<Value> {
    let mut facts = refresh_facts();
    let mut entry = match report.await {
        Ok(json) => serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|v| v.get("entries")?.as_array()?.first().cloned()),
        Err(error) => Some(serde_json::json!({
            "id": id,
            "status": "error",
            "error": crate::display::sanitize_untrusted_field(&error),
            "sections": [],
        })),
    }?;
    facts.retain_stable_emails(&refresh_facts());
    super::payload::attach_account_email(&mut entry, &facts);
    Some(entry)
}

fn apply_payload(state: &mut TrayState, payload: Value) {
    state.payload = payload;
    stamp_facts(state);
    apply_strip_icon(state);
    if state.js_ready {
        push_to_webview(state);
    }
}

fn stamp_facts(state: &mut TrayState) {
    let facts = facts_snapshot(&state.facts);
    let stamped = wrap_report("{}", &facts, 0, None);
    let Some(obj) = state.payload.as_object_mut() else {
        return;
    };
    for key in [
        "shortcut",
        "shortcut_error",
        "refresh_minutes",
        "version",
        "accounts",
    ] {
        obj.insert(key.into(), stamped[key].clone());
    }
}

fn apply_facts(state: &mut TrayState) {
    stamp_facts(state);
    if state.js_ready {
        push_to_webview(state);
    }
}

fn apply_entry(state: &mut TrayState, entry: Value) {
    let Some(id) = entry.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return;
    };
    let Some(entries) = state
        .payload
        .get_mut("entries")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    match entries
        .iter_mut()
        .find(|e| e.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        Some(slot) => *slot = entry,
        None => entries.push(entry),
    }
    apply_strip_icon(state);
    if state.js_ready {
        push_to_webview(state);
    }
}

fn apply_strip_icon(state: &mut TrayState) {
    let content = content_from_payload(&state.payload, &state.stars, &state.strip_order);
    let visible = state
        .strip_order_known
        .then_some(state.strip_order.as_slice());
    let view = menu_bar::View {
        remembered: &state.menu_bar_provider,
        show_all: state.menu_bar_show_all,
        show_value: !state.menu_bar_hide_value,
        window: state.menu_bar_window,
        visible,
        names: &state.strip_names,
        items: &state.menu_bar_items,
        active_account_only: state.menu_bar_active_account_only,
        color_value: state.menu_bar_color_value,
        thresholds: state.color_thresholds,
    };
    let tooltip = menu_bar::tooltip(&state.payload, &view);
    let chips = if state.menu_bar_chart {
        Vec::new()
    } else {
        menu_bar::chips(&state.payload, &view)
    };
    let _ = state.tray.set_tooltip(Some(tooltip.as_str()));
    match state.strip_style {
        StripStyle::Bars => {
            let fractions: Vec<f64> = content.bars.iter().map(|m| m.fraction).collect();
            // Keep tray-icon's slot filled so the status item stays allocated,
            // then replace the image with a 1×/2×/3× template that stays sharp
            // on mixed-DPI monitors.
            if let Ok(icon) = bars_icon(&fractions) {
                let _ = state.tray.set_icon(Some(icon));
            }
            state.tray.set_icon_as_template(true);
            // The chart item carries only the glyph; each provider has its own
            // item to its left. tray-icon's macOS set_title(None) leaves the
            // old title in NSStatusBarButton; an empty title clears it.
            state.tray.set_title(Some(""));
            let tips: Vec<String> = chips.iter().map(status_items::tooltip_line).collect();
            state.provider_items.sync(
                &chips,
                &tips,
                state.menu_bar_centered,
                state.tray.ns_status_item(),
            );
            let image = if state.menu_bar_chart {
                template_bars_image(&fractions)
            } else {
                Some(status_items::template_main_image(f64::from(
                    BARS_POINT_SIDE,
                )))
            };
            if let Some(image) = image {
                set_status_button_image(&state.tray, &image);
            }
        }
        StripStyle::Text => {
            if let Ok(icon) = static_icon() {
                let _ = state.tray.set_icon(Some(icon));
            }
            state.tray.set_icon_as_template(true);
            let title = content.title_line();
            state
                .tray
                .set_title((!title.is_empty()).then_some(title.as_str()));
        }
    }
    // Icon/title updates can reset AppKit's button geometry. Fill and mark it
    // after the update in either display mode.
    mark_open_item(state);
}

fn bars_icon(fractions: &[f64]) -> Result<Icon, tray_icon::BadIcon> {
    let rgba = bars_rgba(fractions, BARS_PIXEL_SIDE);
    Icon::from_rgba(rgba, BARS_PIXEL_SIDE, BARS_PIXEL_SIDE)
}

fn static_icon() -> Result<Icon, tray_icon::BadIcon> {
    let (rgba, size) = tray_icon_rgba(BARS_PIXEL_SIDE, Severity::Low);
    Icon::from_rgba(rgba, size, size)
}

enum ShortcutOutcome {
    Bound(String),
    Cleared,
    Refused { attempted: String, reason: String },
}

fn bind_shortcut(binding: Option<&mut HotkeyBinding>, value: &str) -> ShortcutOutcome {
    let Some(binding) = binding else {
        return ShortcutOutcome::Refused {
            attempted: value.trim().to_string(),
            reason: "Could not set up the global shortcut".into(),
        };
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return match binding.apply(None) {
            Ok(()) => ShortcutOutcome::Cleared,
            Err(reason) => ShortcutOutcome::Refused {
                attempted: String::new(),
                reason,
            },
        };
    }
    match super::hotkey::normalize(trimmed) {
        Ok(normalized) => match binding.apply(Some(&normalized.canonical)) {
            Ok(()) => ShortcutOutcome::Bound(normalized.canonical),
            Err(reason) => ShortcutOutcome::Refused {
                attempted: normalized.canonical,
                reason,
            },
        },
        Err(reason) => ShortcutOutcome::Refused {
            attempted: trimmed.to_string(),
            reason,
        },
    }
}

fn apply_shortcut_outcome(facts: &mut HostFacts, outcome: ShortcutOutcome) {
    match outcome {
        ShortcutOutcome::Bound(canonical) => {
            facts.shortcut = canonical;
            facts.shortcut_error.clear();
        }
        ShortcutOutcome::Cleared => {
            facts.shortcut.clear();
            facts.shortcut_error.clear();
        }
        ShortcutOutcome::Refused { attempted, reason } => {
            facts.shortcut = attempted;
            facts.shortcut_error = reason;
        }
    }
}

fn config_path() -> Option<PathBuf> {
    crate::config::resolved_path().or_else(crate::config::default_path)
}

fn set_shortcut(state: &mut TrayState, value: &str) {
    let outcome = bind_shortcut(state.hotkey.as_mut(), value);
    let persisted = match &outcome {
        ShortcutOutcome::Bound(canonical) => Some(Some(canonical.clone())),
        ShortcutOutcome::Cleared => Some(None),
        ShortcutOutcome::Refused { .. } => None,
    };
    if let Some(value) = persisted
        && let Some(path) = config_path()
    {
        let _ = crate::config::set_tray_value(&path, "shortcut", value.map(Into::into));
    }
    with_facts(&state.facts, |f| apply_shortcut_outcome(f, outcome));
    apply_facts(state);
}

fn set_refresh(state: &mut TrayState, minutes: u64) {
    if !crate::config::TRAY_REFRESH_MINUTES.contains(&minutes) {
        return;
    }
    if let Some(path) = config_path() {
        let _ = crate::config::set_tray_value(
            &path,
            "refresh_minutes",
            Some(i64::try_from(minutes).unwrap_or(i64::MAX).into()),
        );
    }
    with_facts(&state.facts, |f| f.refresh_secs = minutes * 60);
    apply_facts(state);
    let _ = state.worker.send(WorkerCmd::Refresh);
}

fn push_to_webview(state: &TrayState) {
    let Some(webview) = state.webview.as_ref() else {
        return;
    };
    let json = popover_payload(state);
    let script = format!("window.__AIUB_APPLY__ && window.__AIUB_APPLY__({json})");
    let _ = webview.evaluate_script(&script);
}

fn popover_payload(state: &TrayState) -> String {
    let mut payload = state.payload.clone();
    payload["menu_bar_show_all"] = json!(state.menu_bar_show_all);
    payload["menu_bar_hide_value"] = json!(state.menu_bar_hide_value);
    payload["menu_bar_window"] = json!(state.menu_bar_window.as_str());
    payload["menu_bar_provider"] = json!(state.menu_bar_provider);
    payload["menu_bar_chart"] = json!(state.menu_bar_chart);
    payload["menu_bar_items"] = json!(state.menu_bar_items);
    payload["menu_bar_active_account_only"] = json!(state.menu_bar_active_account_only);
    payload["menu_bar_color_value"] = json!(state.menu_bar_color_value);
    payload["menu_bar_centered"] = json!(state.menu_bar_centered);
    payload["notifications_enabled"] = json!(state.notifications_enabled);
    payload["notifications_threshold"] = json!(state.notifications_threshold);
    host_payload(&payload)
}

fn handle_tray(state: &mut TrayState, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        handle_chart_click(state, button);
    }
}

fn handle_chart_click(state: &mut TrayState, button: MouseButton) {
    match button {
        MouseButton::Left => {
            // The press this click ends has been handled; a later blur
            // is not part of it.
            state.status_item_pressed_at = None;
            if state.popover_open && state.focused_provider.is_none() {
                hide_popover(state);
            } else {
                // The chart opens the popover as it was, not on a provider.
                state.last_anchor = Some(cocoa_mouse());
                prepare_popover(state, None);
            }
        }
        MouseButton::Right => show_chart_menu(state),
        MouseButton::Middle => next_menu_bar_provider(state),
    }
}

/// The chart item's left click on macOS 27: the menu bar opens a session on
/// the press and ends it on a long press released, a drag out or a cancel.
fn handle_chart_session(state: &mut TrayState, began: bool) {
    let chart_open = (state.popover_open || state.show_pending) && state.focused_provider.is_none();
    if began {
        state.status_item_pressed_at = None;
        if !chart_open {
            state.last_anchor = Some(cocoa_mouse());
            prepare_popover(state, None);
        }
        if let Some(session) = &state.chart_session {
            session.acknowledge();
        }
    } else if chart_open {
        hide_popover(state);
    }
    mark_open_item(state);
}

/// A press on the item whose session is open. The menu bar tracks that item
/// and sends it nothing, so this global press is the only sign of the click
/// that closes it, as it would a native menu.
fn press_on_open_session_item(state: &TrayState, x: f64, y: f64, timestamp: f64) -> bool {
    if !state.popover_open {
        return false;
    }
    match state.focused_provider.as_deref() {
        None => {
            state
                .chart_session
                .as_ref()
                .is_some_and(|session| session.open_before(timestamp))
                && status_item_frame(&state.tray).is_some_and(|frame| frame.contains(x, y))
        }
        Some(id) => state.provider_items.index_of(id).is_some_and(|index| {
            state.provider_items.session_open_before(index, timestamp)
                && state
                    .provider_items
                    .frames()
                    .into_iter()
                    .any(|(item, frame)| item == id && ns_rect_to_cocoa(frame).contains(x, y))
        }),
    }
}

const MENU_CHART_REFRESH: isize = 100;
const MENU_CHART_SETTINGS: isize = 101;
const MENU_CHART_QUIT: isize = 102;
/// The chart menu's global options, some of the Settings screen's. Unlike
/// there, the window, value and color picked here also replace every
/// provider's own choice.
const MENU_CHART_WINDOWS: [(isize, WindowChoice); 4] = [
    (103, WindowChoice::Auto),
    (104, WindowChoice::Session),
    (105, WindowChoice::Weekly),
    (106, WindowChoice::Monthly),
];
const MENU_CHART_TOGGLE_VALUE: isize = 107;
const MENU_CHART_TOGGLE_COLOR: isize = 108;
const MENU_CHART_ACTIVE_ACCOUNT_ONLY: isize = 109;
const MENU_CHART_CENTERED: isize = 110;

fn chart_menu_command(tag: isize) -> Option<Command> {
    match tag {
        MENU_CHART_REFRESH => Some(Command::Refresh {}),
        MENU_CHART_SETTINGS => Some(Command::OpenSettings {}),
        MENU_CHART_QUIT => Some(Command::Quit {}),
        _ => None,
    }
}

/// Where the menu bar's providers stand on the chart menu's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChartMenuState {
    /// Every provider in the menu bar shows its value.
    all_show: bool,
    /// Every provider in the menu bar colors its value.
    all_colored: bool,
    /// The window every provider in the menu bar reads, when they all read
    /// the same one.
    window: Option<UsageWindow>,
    active_only: bool,
    centered: bool,
}

fn chart_menu_state(state: &TrayState) -> ChartMenuState {
    let shown: Vec<Option<&MenuBarItemConfig>> = (0..)
        .map_while(|index| state.provider_items.id_at(index))
        .map(|id| state.menu_bar_items.get(id))
        .collect();
    ChartMenuState {
        all_show: all_on(&shown, !state.menu_bar_hide_value, |item| {
            item.hide_value.map(|hide| !hide)
        }),
        all_colored: all_on(&shown, state.menu_bar_color_value, |item| item.color_value),
        window: same_window(&shown, state.menu_bar_window),
        active_only: state.menu_bar_active_account_only,
        centered: state.menu_bar_centered,
    }
}

/// The window every provider shown reads, by its own choice or by the
/// global one, when they all read the same. With none shown, the global one.
fn same_window(shown: &[Option<&MenuBarItemConfig>], global: UsageWindow) -> Option<UsageWindow> {
    let mut windows = shown
        .iter()
        .map(|item| menu_bar::item_window(*item, global));
    let first = windows.next().unwrap_or(global);
    windows.all(|window| window == first).then_some(first)
}

/// Whether every provider shown has an option on: by its own choice, or by
/// the global one where it has none. With none shown, the global one.
fn all_on(
    shown: &[Option<&MenuBarItemConfig>],
    global: bool,
    own: fn(&MenuBarItemConfig) -> Option<bool>,
) -> bool {
    if shown.is_empty() {
        return global;
    }
    shown
        .iter()
        .all(|item| item.and_then(own).unwrap_or(global))
}

/// The command one of the chart menu's global options stands for, and the
/// provider choice it replaces. Value and color turn off only when every
/// provider has them on, so each pick changes what the menu bar shows.
fn chart_global_pick(tag: isize, bar: ChartMenuState) -> Option<(Command, Option<&'static str>)> {
    Some(match tag {
        MENU_CHART_TOGGLE_VALUE => (
            Command::SetMenuBarHideValue {
                value: bar.all_show,
            },
            Some("hide_value"),
        ),
        MENU_CHART_TOGGLE_COLOR => (
            Command::SetMenuBarColorValue {
                value: !bar.all_colored,
            },
            Some("color_value"),
        ),
        MENU_CHART_ACTIVE_ACCOUNT_ONLY => (
            Command::SetMenuBarActiveAccountOnly {
                value: !bar.active_only,
            },
            None,
        ),
        MENU_CHART_CENTERED => (
            Command::SetMenuBarCentered {
                value: !bar.centered,
            },
            None,
        ),
        _ => {
            let (_, value) = MENU_CHART_WINDOWS.iter().find(|(t, _)| *t == tag)?;
            (Command::SetMenuBarWindow { value: *value }, Some("window"))
        }
    })
}

/// A pick among the chart menu's global options, or `None` when `tag` is not
/// one. The window, value and color apply to every provider, so each
/// provider's own choice for them goes first, whether it came from its menu or
/// from Settings; when that cannot be saved, the pick runs no command.
fn chart_global_command(state: &mut TrayState, tag: isize) -> Option<Option<Command>> {
    let (command, key) = chart_global_pick(tag, chart_menu_state(state))?;
    if let Some(key) = key
        && !replace_own_choices(
            &mut state.menu_bar_items,
            key,
            &command,
            config_path().as_deref(),
        )
    {
        return Some(None);
    }
    Some(Some(command))
}

/// Drop each provider's own choice for `key` and save that with the global
/// value `command` sets, in one write to the config at `path`. Whether it was
/// saved: if not, the choices stay in memory too.
fn replace_own_choices(
    items: &mut std::collections::BTreeMap<String, MenuBarItemConfig>,
    key: &str,
    command: &Command,
    path: Option<&std::path::Path>,
) -> bool {
    if let (Some(path), Some((global, value))) = (path, global_value(command))
        && crate::config::set_tray_value_for_all_items(path, global, value, key).is_err()
    {
        return false;
    }
    clear_own_choice(items, key);
    true
}

/// The `[tray]` preference a global option writes.
fn global_value(command: &Command) -> Option<(&'static str, toml_edit::Value)> {
    Some(match command {
        Command::SetMenuBarWindow { value } => {
            ("menu_bar_window", value.usage_window().as_str().into())
        }
        Command::SetMenuBarHideValue { value } => ("menu_bar_hide_value", (*value).into()),
        Command::SetMenuBarColorValue { value } => ("menu_bar_color_value", (*value).into()),
        _ => return None,
    })
}

/// Drop each provider's own choice for `key`: `window`, `hide_value` or
/// `color_value`, and the providers left with no choice of their own.
fn clear_own_choice(items: &mut std::collections::BTreeMap<String, MenuBarItemConfig>, key: &str) {
    for item in items.values_mut() {
        match key {
            "window" => item.window = None,
            "hide_value" => item.hide_value = None,
            "color_value" => item.color_value = None,
            _ => {}
        }
    }
    items.retain(|_, item| *item != MenuBarItemConfig::default());
}

fn show_chart_menu(state: &mut TrayState) {
    state.status_item_pressed_at = None;
    if state.popover_open || state.show_pending {
        hide_popover(state);
    }
    let Some(button) =
        MainThreadMarker::new().and_then(|mtm| state.tray.ns_status_item()?.button(mtm))
    else {
        return;
    };
    let pt = state.language == "pt-BR";
    let bar = chart_menu_state(state);
    let label = |en: &str, br: &str| (if pt { br } else { en }).to_owned();
    let pick = |title, tag, checked| MenuLine::Pick {
        title,
        tag,
        checked,
    };
    let mut lines = vec![
        pick(label("Refresh", "Atualizar"), MENU_CHART_REFRESH, false),
        pick(
            label("Settings", "Configurações"),
            MENU_CHART_SETTINGS,
            false,
        ),
        MenuLine::Separator,
        MenuLine::Heading(label("Window", "Janela")),
    ];
    for (tag, choice) in MENU_CHART_WINDOWS {
        let title = match choice {
            WindowChoice::Auto => label("Highest", "Maior uso"),
            WindowChoice::Session => label("5 hours", "5 horas"),
            WindowChoice::Weekly => label("Weekly", "Semanal"),
            WindowChoice::Monthly => label("Monthly", "Mensal"),
        };
        lines.push(pick(title, tag, bar.window == Some(choice.usage_window())));
    }
    lines.extend([
        MenuLine::Separator,
        pick(
            label("Show value", "Mostrar valor"),
            MENU_CHART_TOGGLE_VALUE,
            bar.all_show,
        ),
        pick(
            label("Color the value", "Colorir o valor"),
            MENU_CHART_TOGGLE_COLOR,
            bar.all_colored,
        ),
        pick(
            label("Only the account in use", "Só a conta em uso"),
            MENU_CHART_ACTIVE_ACCOUNT_ONLY,
            bar.active_only,
        ),
        pick(
            label("Center in the menu bar", "Centralizar no menu bar"),
            MENU_CHART_CENTERED,
            bar.centered,
        ),
    ]);
    if bar.centered && !menu_space::trusted() {
        lines.push(pick(
            label("Authorize centering…", "Autorizar centralização…"),
            MENU_ACCESSIBILITY,
            false,
        ));
    }
    lines.extend([
        MenuLine::Separator,
        pick(label("Quit", "Sair"), MENU_CHART_QUIT, false),
    ]);
    state.last_anchor = status_item_frame(&state.tray)
        .map(|frame| (frame.x + frame.w / 2.0, frame.y + frame.h / 2.0));
    if state.chart_session.is_some() {
        state
            .provider_items
            .show_menu_on_button(&button, &lines, state.chart_session.as_ref());
        return;
    }
    status_items::mark_open(&button, true);
    state
        .provider_items
        .show_menu_on_button(&button, &lines, None);
    status_items::mark_open(&button, false);
}

/// A provider item's click opens the popover under it on that provider's tab
/// (a second click closes it); its right click opens the provider's menu.
fn handle_provider_item(state: &mut TrayState, action: ItemAction) {
    match action {
        ItemAction::Click { index, right } => {
            state.status_item_pressed_at = None;
            let Some(id) = state.provider_items.id_at(index).map(str::to_owned) else {
                return;
            };
            if right {
                let lines = provider_menu(state, &id);
                state.provider_items.show_menu(index, &lines);
                return;
            }
            if (state.popover_open || state.show_pending)
                && state.focused_provider.as_deref() == Some(id.as_str())
            {
                hide_popover(state);
                return;
            }
            let frame = state
                .provider_items
                .frames()
                .into_iter()
                .find(|(item, _)| *item == id)
                .map(|(_, frame)| ns_rect_to_cocoa(frame));
            state.last_anchor = frame
                .map(|frame| (frame.x + frame.w / 2.0, frame.y + frame.h / 2.0))
                .or_else(|| Some(cocoa_mouse()));
            prepare_popover(state, Some(id));
        }
        ItemAction::Menu { tag } => apply_provider_menu_pick(state, tag),
        ItemAction::Expanded { index } => {
            let open_here = (state.popover_open || state.show_pending)
                && state.focused_provider.as_deref() == state.provider_items.id_at(index);
            if !open_here {
                handle_provider_item(
                    state,
                    ItemAction::Click {
                        index,
                        right: false,
                    },
                );
            }
            state.provider_items.acknowledge(index);
            mark_open_item(state);
        }
        ItemAction::Collapsed { index } => {
            if (state.popover_open || state.show_pending)
                && state.focused_provider.is_some()
                && state.focused_provider.as_deref() == state.provider_items.id_at(index)
            {
                hide_popover(state);
            }
        }
    }
}

/// Keep the old provider's surface out of the next provider's position. The
/// matching render acknowledgement supplies the new size before ordering front.
fn prepare_popover(state: &mut TrayState, provider: Option<String>) {
    prepare_popover_screen(state, provider, "dashboard");
}

fn prepare_popover_screen(state: &mut TrayState, provider: Option<String>, screen: &'static str) {
    guard_blur(state);
    state.window.set_visible(false);
    state.popover_open = false;
    state.show_pending = true;
    state.presentation_revision += 1;
    state.presentation_screen = screen;
    focus_provider(state, provider);
}

/// Show the popover a provider click held back, once.
fn show_pending(state: &mut TrayState) {
    if std::mem::take(&mut state.show_pending) && !state.popover_open {
        show_popover(state);
    }
}

/// Tell the popover which provider to open on, or to open as it was.
fn focus_provider(state: &mut TrayState, id: Option<String>) {
    state.focused_provider = id;
    mark_open_item(state);
    if let Some(webview) = state.webview.as_ref() {
        let arg = serde_json::to_string(&state.focused_provider).unwrap_or_else(|_| "null".into());
        let revision = state.presentation_revision;
        let screen = state.presentation_screen;
        let _ = webview.evaluate_script(&format!(
            "window.__AIUB_FOCUS__ && window.__AIUB_FOCUS__({arg}, {revision}, '{screen}')"
        ));
    }
}

/// A provider menu's tags name the pick only; the provider is the one whose
/// menu is open (`menu_provider`), since item indexes shift between refreshes.
const MENU_WINDOWS: [(isize, UsageWindow); 4] = [
    (1, UsageWindow::Auto),
    (2, UsageWindow::Session),
    (3, UsageWindow::Weekly),
    (4, UsageWindow::Monthly),
];
const MENU_TOGGLE_VALUE: isize = 10;
const MENU_HIDE: isize = 11;
const MENU_ACTIVE_ACCOUNT_ONLY: isize = 12;
const MENU_OPEN: isize = 13;
const MENU_TOGGLE_COLOR: isize = 14;
const MENU_CENTERED: isize = 15;
const MENU_ACCESSIBILITY: isize = 16;

/// Whether a provider's menu checks `choice`: "Same as the menu bar" while it
/// has no window of its own, otherwise that window.
fn window_checked(own: Option<UsageWindow>, choice: UsageWindow) -> bool {
    match own {
        None => choice == UsageWindow::Auto,
        Some(own) => own == choice && own != UsageWindow::Auto,
    }
}

fn provider_menu(state: &mut TrayState, id: &str) -> Vec<MenuLine> {
    state.menu_provider = Some(id.to_owned());
    let pt = state.language == "pt-BR";
    let label = |en: &str, br: &str| (if pt { br } else { en }).to_owned();
    let item = state.menu_bar_items.get(id).cloned().unwrap_or_default();
    let window = menu_bar::own_window(&item);
    let hide_value = item.hide_value.unwrap_or(state.menu_bar_hide_value);
    let name = state
        .strip_names
        .get(id)
        .cloned()
        .or_else(|| {
            state.payload["entries"]
                .as_array()?
                .iter()
                .find(|entry| entry["id"].as_str() == Some(id))?["display_name"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| id.to_owned());
    let mut lines = vec![
        MenuLine::Heading(name),
        MenuLine::Pick {
            title: label("Open", "Abrir"),
            tag: MENU_OPEN,
            checked: false,
        },
        MenuLine::Separator,
        MenuLine::Heading(label("Window", "Janela")),
    ];
    for (tag, choice) in MENU_WINDOWS {
        let title = match choice {
            UsageWindow::Auto => label("Same as the menu bar", "Igual ao menu bar"),
            UsageWindow::Session => label("5 hours", "5 horas"),
            UsageWindow::Weekly => label("Weekly", "Semanal"),
            UsageWindow::Monthly => label("Monthly", "Mensal"),
        };
        lines.push(MenuLine::Pick {
            title,
            tag,
            checked: window_checked(window, choice),
        });
    }
    lines.push(MenuLine::Separator);
    lines.push(MenuLine::Pick {
        title: label("Show value", "Mostrar valor"),
        tag: MENU_TOGGLE_VALUE,
        checked: !hide_value,
    });
    lines.push(MenuLine::Pick {
        title: label("Color the value", "Colorir o valor"),
        tag: MENU_TOGGLE_COLOR,
        checked: item.color_value.unwrap_or(state.menu_bar_color_value),
    });
    lines.push(MenuLine::Pick {
        title: label("Only the account in use", "Só a conta em uso"),
        tag: MENU_ACTIVE_ACCOUNT_ONLY,
        checked: state.menu_bar_active_account_only,
    });
    lines.push(MenuLine::Pick {
        title: label("Center in the menu bar", "Centralizar no menu bar"),
        tag: MENU_CENTERED,
        checked: state.menu_bar_centered,
    });
    if state.menu_bar_centered && !menu_space::trusted() {
        lines.push(MenuLine::Pick {
            title: label("Authorize centering…", "Autorizar centralização…"),
            tag: MENU_ACCESSIBILITY,
            checked: false,
        });
    }
    lines.push(MenuLine::Separator);
    lines.push(MenuLine::Pick {
        title: label("Hide from the menu bar", "Ocultar do menu bar"),
        tag: MENU_HIDE,
        checked: false,
    });
    lines
}

fn apply_provider_menu_pick(state: &mut TrayState, tag: isize) {
    if tag == MENU_ACCESSIBILITY {
        let proxy = state.proxy.clone();
        menu_space::request_access(move || {
            let _ = proxy.send_event(UserEvent::AccessibilityChanged);
        });
        return;
    }
    let Some(id) = state.menu_provider.clone() else {
        return;
    };
    if tag == MENU_OPEN {
        if let Some(index) = state.provider_items.index_of(&id) {
            handle_provider_item(
                state,
                ItemAction::Click {
                    index,
                    right: false,
                },
            );
        }
        return;
    }
    if tag == MENU_CENTERED {
        state.menu_bar_centered = !state.menu_bar_centered;
        if state.menu_bar_centered {
            let proxy = state.proxy.clone();
            menu_space::request_access(move || {
                let _ = proxy.send_event(UserEvent::AccessibilityChanged);
            });
        }
        persist_menu_bar_value("menu_bar_centered", state.menu_bar_centered.into());
    } else if tag == MENU_ACTIVE_ACCOUNT_ONLY {
        state.menu_bar_active_account_only = !state.menu_bar_active_account_only;
        persist_menu_bar_value(
            "menu_bar_active_account_only",
            state.menu_bar_active_account_only.into(),
        );
    } else if let Some((_, window)) = MENU_WINDOWS.iter().find(|(t, _)| *t == tag) {
        let value = (*window != UsageWindow::Auto).then(|| window.as_str().to_owned());
        set_menu_bar_item(state, &id, "window", value.map(Into::into));
    } else if tag == MENU_TOGGLE_VALUE {
        let hidden = state
            .menu_bar_items
            .get(&id)
            .and_then(|item| item.hide_value)
            .unwrap_or(state.menu_bar_hide_value);
        set_menu_bar_item(state, &id, "hide_value", Some((!hidden).into()));
    } else if tag == MENU_TOGGLE_COLOR {
        let colored = state
            .menu_bar_items
            .get(&id)
            .and_then(|item| item.color_value)
            .unwrap_or(state.menu_bar_color_value);
        set_menu_bar_item(state, &id, "color_value", Some((!colored).into()));
    } else if tag == MENU_HIDE {
        set_menu_bar_item(state, &id, "hidden", Some(true.into()));
    } else {
        return;
    }
    apply_strip_icon(state);
    push_to_webview(state);
}

/// Change one provider's menu-bar setting in memory and in config.toml.
fn set_menu_bar_item(state: &mut TrayState, id: &str, key: &str, value: Option<toml_edit::Value>) {
    let item = state.menu_bar_items.entry(id.to_owned()).or_default();
    match key {
        "window" => item.window = value.as_ref().and_then(|v| v.as_str()).map(str::to_owned),
        "hide_value" => item.hide_value = value.as_ref().and_then(toml_edit::Value::as_bool),
        "hidden" => {
            item.hidden = value
                .as_ref()
                .and_then(toml_edit::Value::as_bool)
                .unwrap_or(false)
        }
        "color_value" => item.color_value = value.as_ref().and_then(toml_edit::Value::as_bool),
        _ => return,
    }
    if *item == MenuBarItemConfig::default() {
        state.menu_bar_items.remove(id);
    }
    // `hidden = false` is the default; `hide_value = false` is an override.
    let value = value.filter(|v| key != "hidden" || v.as_bool() != Some(false));
    if let Some(path) = config_path() {
        let _ = crate::config::set_menu_bar_item_value(&path, id, key, value);
    }
}

fn persist_menu_bar_value(key: &str, value: toml_edit::Value) {
    if let Some(path) = config_path() {
        let _ = crate::config::set_tray_value(&path, key, Some(value));
    }
}

fn next_menu_bar_provider(state: &mut TrayState) {
    let visible = state
        .strip_order_known
        .then_some(state.strip_order.as_slice());
    if let Some(id) = menu_bar::next_id(
        &state.payload,
        &state.menu_bar_provider,
        state.menu_bar_window,
        visible,
    ) {
        state.menu_bar_provider = id.clone();
        persist_menu_bar_value("menu_bar_provider", id.into());
        // A cycle must visibly change the strip even when Show All was on.
        if state.menu_bar_show_all {
            state.menu_bar_show_all = false;
            persist_menu_bar_value("menu_bar_show_all", false.into());
        }
        apply_strip_icon(state);
    }
}

fn set_menu_bar_window(state: &mut TrayState, window: UsageWindow) {
    state.menu_bar_window = window;
    persist_menu_bar_value("menu_bar_window", window.as_str().into());
    apply_strip_icon(state);
}

/// Start a switch the popover asked for. Only a vendor and label the host
/// itself reported are accepted, and never while one is already running.
fn request_account_switch(state: &mut TrayState, vendor: &str, label: &str) {
    let allowed = facts_snapshot(&state.facts).accounts.iter().any(|fact| {
        fact.vendor == vendor
            && !fact.switching
            && fact.active.as_deref() != Some(label)
            && fact.labels.iter().any(|known| known == label)
    });
    if !allowed {
        return;
    }
    with_facts(&state.facts, |f| {
        for fact in f.accounts.iter_mut().filter(|fact| fact.vendor == vendor) {
            fact.target = label.to_string();
            fact.switching = true;
            fact.error.clear();
        }
    });
    apply_facts(state);
    let facts = state.facts.clone();
    let proxy = state.proxy.clone();
    let worker = state.worker.clone();
    let failed_vendor = vendor.to_string();
    let (vendor, label) = (vendor.to_string(), label.to_string());
    let spawned = std::thread::Builder::new()
        .name("ai-usagebar-tray-account-switch".into())
        .spawn(move || {
            run_account_switch(&facts, &vendor, &label);
            let _ = proxy.send_event(UserEvent::Facts);
            let _ = worker.send(WorkerCmd::Refresh);
        });
    if spawned.is_err() {
        with_facts(&state.facts, |f| {
            for fact in f
                .accounts
                .iter_mut()
                .filter(|fact| fact.vendor == failed_vendor)
            {
                fact.switching = false;
                fact.error = "could not start the account switch".into();
            }
        });
        apply_facts(state);
    }
}

/// Act on a validated popover command. The boundary already checked shape,
/// enums and bounds; what needs live state (is this entry still in the report,
/// is this account one the host reported) is checked here.
fn handle_command(state: &mut TrayState, command: Command, control_flow: &mut ControlFlow) {
    match command {
        Command::Ready {} => {
            state.js_ready = true;
            push_to_webview(state);
            // A first click can precede WebKit's handlers. Replay the native
            // selection now rather than leaving the dashboard in overview.
            sync_popover_visibility(state);
            if state.show_pending {
                focus_provider(state, state.focused_provider.clone());
            }
        }
        Command::Detect {} => {
            let _ = state.worker.send(WorkerCmd::Detect);
        }
        Command::Refresh {} => {
            let _ = state.worker.send(WorkerCmd::Refresh);
        }
        Command::OpenSettings {} => prepare_popover_screen(state, None, "settings"),
        Command::Close {} => hide_popover(state),
        Command::Quit {} => *control_flow = ControlFlow::Exit,
        Command::ToggleStartup {} => toggle_startup(state),
        Command::SwitchAccount { vendor, label } => {
            request_account_switch(state, &vendor.0, &label.0);
        }
        Command::Resize(measurement) => handle_resize(state, &measurement),
        Command::ResetPanelSize {} => reset_panel_size(state),
        Command::SetPinned { value } => state.pinned = value,
        Command::RefreshEntry { id } => {
            // An unknown id would come back as an error entry and be appended
            // to the report.
            if report_has_entry(&state.payload, id.as_str()) {
                let _ = state
                    .worker
                    .send(WorkerCmd::RefreshEntry(id.as_str().to_owned()));
            }
        }
        Command::SetShortcut { value } => set_shortcut(state, &value.0),
        Command::SetRefresh { minutes } => set_refresh(state, minutes.0),
        Command::SetNotificationsEnabled { value: enabled } => {
            if let Some(path) = config_path()
                && crate::config::set_notification_value(&path, "enabled", enabled.into()).is_ok()
            {
                state.notifications_enabled = enabled;
                push_to_webview(state);
            }
        }
        Command::SetNotificationsThreshold { value } => {
            let threshold = value.0;
            if let Some(path) = config_path()
                && crate::config::set_notification_value(
                    &path,
                    "threshold",
                    i64::from(threshold).into(),
                )
                .is_ok()
            {
                state.notifications_threshold = threshold;
                push_to_webview(state);
            }
        }
        Command::NextMenuBarProvider {} => {
            next_menu_bar_provider(state);
            push_to_webview(state);
        }
        Command::SetMenuBarProvider { value } => {
            let id = value.as_str();
            let visible = state
                .strip_order_known
                .then_some(state.strip_order.as_slice());
            if menu_bar_provider_eligible(&state.payload, visible, id) {
                state.menu_bar_provider = id.to_owned();
                persist_menu_bar_value("menu_bar_provider", id.into());
                if state.menu_bar_show_all {
                    state.menu_bar_show_all = false;
                    persist_menu_bar_value("menu_bar_show_all", false.into());
                }
                apply_strip_icon(state);
                push_to_webview(state);
            }
        }
        Command::SetMenuBarShowAll { value: enabled } => {
            state.menu_bar_show_all = enabled;
            persist_menu_bar_value("menu_bar_show_all", enabled.into());
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarHideValue { value: enabled } => {
            state.menu_bar_hide_value = enabled;
            persist_menu_bar_value("menu_bar_hide_value", enabled.into());
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarWindow { value } => {
            set_menu_bar_window(state, value.usage_window());
            push_to_webview(state);
        }
        Command::SetMenuBarChart { value: enabled } => {
            state.menu_bar_chart = enabled;
            persist_menu_bar_value(
                "menu_bar_style",
                (if enabled { "bars" } else { "provider" }).into(),
            );
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarItem(change) => {
            // Settings lists one row per report entry; nothing else is written.
            if !report_has_entry(&state.payload, change.id.as_str()) {
                return;
            }
            set_menu_bar_item(
                state,
                change.id.as_str(),
                change.setting.key(),
                menu_bar_item_value(change.setting),
            );
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarColorValue { value: enabled } => {
            state.menu_bar_color_value = enabled;
            persist_menu_bar_value("menu_bar_color_value", enabled.into());
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarCentered { value: enabled } => {
            state.menu_bar_centered = enabled;
            if enabled {
                let proxy = state.proxy.clone();
                menu_space::request_access(move || {
                    let _ = proxy.send_event(UserEvent::AccessibilityChanged);
                });
            }
            persist_menu_bar_value("menu_bar_centered", enabled.into());
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::SetMenuBarActiveAccountOnly { value: enabled } => {
            state.menu_bar_active_account_only = enabled;
            persist_menu_bar_value("menu_bar_active_account_only", enabled.into());
            apply_strip_icon(state);
            push_to_webview(state);
        }
        Command::Strip(layout) => {
            state.strip_style = StripStyle::Bars;
            state.stars = layout.stars;
            state.strip_order = layout.order;
            state.strip_order_known = true;
            state.strip_names = layout.names;
            state.color_thresholds = layout.thresholds;
            state.language = layout.language;
            apply_strip_icon(state);
        }
        Command::OpenUrl { url } => browse::open(&url.0),
    }
}

fn report_has_entry(payload: &Value, id: &str) -> bool {
    payload
        .get("entries")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
        })
}

/// `highest`, or a report entry the popover still shows.
fn menu_bar_provider_eligible(payload: &Value, visible: Option<&[String]>, id: &str) -> bool {
    id == menu_bar::HIGHEST_PROVIDER
        || report_has_entry(payload, id)
            && visible.is_none_or(|shown| shown.iter().any(|v| v == id))
}

/// The config value one menu-bar override writes; `None` clears it.
fn menu_bar_item_value(setting: ItemSetting) -> Option<toml_edit::Value> {
    match setting {
        ItemSetting::Window(window) => window.map(|window| window.as_str().into()),
        ItemSetting::HideValue(flag)
        | ItemSetting::Hidden(flag)
        | ItemSetting::ColorValue(flag) => Some(flag.into()),
    }
}

fn current_panel_measurement(
    measurement: &Measurement,
    revision: u64,
    provider: Option<&str>,
    pending: bool,
    screen: &str,
) -> bool {
    measurement.revision == revision
        && measurement.provider() == provider.unwrap_or("")
        && (!pending || measurement.screen.as_str() == screen)
}

fn handle_resize(state: &mut TrayState, measurement: &Measurement) {
    if !current_panel_measurement(
        measurement,
        state.presentation_revision,
        state.focused_provider.as_deref(),
        state.show_pending,
        state.presentation_screen,
    ) {
        return;
    }
    apply_theme(state, measurement.theme.into());
    let visible_h = anchor_visible_height(state.last_anchor);
    state.popover_height = measurement.height.0;
    state.compact_popover = measurement.compact;
    if state.show_pending {
        // The provider tab has its height: show it at that size.
        show_pending(state);
        return;
    }
    // While the user drags an edge the content reports its own height; fitting
    // to it would fight the drag. The next report after the drag applies.
    if in_live_resize(&state.window) {
        return;
    }
    fit_window_to_content(state, visible_h);
}

fn fit_window_to_content(state: &mut TrayState, visible_h: f64) {
    // Open: position_popover sets size and origin in one synchronous frame.
    // tao's set_inner_size is queued on the main dispatch queue, so it would
    // land after that frame and could apply a size computed before it.
    if state.popover_open {
        position_popover(state);
        return;
    }
    let target = fitted_popover_height(state, visible_h);
    state
        .window
        .set_inner_size(LogicalSize::new(state.panel_size.width, target));
}

/// Capture the final size even when tao delivers its events after live resize.
fn note_user_resize(state: &mut TrayState, frame: CocoaRect, previous_height: f64) {
    let mut next = PanelSize::dragged(frame.w, frame.h);
    if (frame.h - previous_height).abs() < 1.0 {
        next.max_height = state.panel_size.max_height;
    }
    state.panel_size = next;
    state.panel_size_dirty = true;
    save_panel_size(state);
    if state.popover_open {
        position_popover(state);
    }
}

struct WindowResizeObserver(Retained<ProtocolObject<dyn NSObjectProtocol>>);

impl Drop for WindowResizeObserver {
    fn drop(&mut self) {
        // SAFETY: this token came from this notification centre's block API.
        let observer: &AnyObject = (*self.0).as_ref();
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
    }
}

#[derive(Clone, Copy)]
struct NativeResizeSession {
    initial: CocoaRect,
    horizontal: Option<HorizontalResize>,
}

/// AppKit notifications run synchronously inside the native mouse tracking
/// loop; tao's queued Resized event arrives too late to centre the live drag.
fn install_resize_observer(
    window: &Window,
    proxy: EventLoopProxy<UserEvent>,
) -> Option<WindowResizeObserver> {
    let ptr = window.ns_window() as *mut NSWindow;
    // SAFETY: tao owns the window; the observer is removed before it is dropped.
    let window = unsafe { ptr.as_ref() }?;
    let session = Mutex::new(None::<NativeResizeSession>);
    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        if MainThreadMarker::new().is_none() {
            return;
        }
        // SAFETY: NotificationCenter supplies a valid notification for this call.
        let note = unsafe { note.as_ref() };
        let Some(object) = note.object() else {
            return;
        };
        let Some(window) = object.downcast_ref::<NSWindow>() else {
            return;
        };
        // setFrame posts another DidResize synchronously. Ignore our own resize
        // rather than re-entering the same drag or deadlocking on its state.
        let Ok(mut session) = session.try_lock() else {
            return;
        };
        let name = note.name();
        let current = ns_rect_to_cocoa(window.frame());
        // SAFETY: AppKit owns these process-lifetime notification names.
        if &*name == unsafe { NSWindowWillStartLiveResizeNotification } {
            *session = Some(NativeResizeSession {
                initial: current,
                horizontal: None,
            });
        } else if &*name == unsafe { NSWindowDidEndLiveResizeNotification } {
            if let Some(ended) = session.take() {
                let _ = proxy.send_event(UserEvent::UserResized {
                    frame: current,
                    previous_height: ended.initial.h,
                });
            }
        } else if &*name == unsafe { NSWindowDidResizeNotification }
            && let Some(drag) = session.as_mut()
        {
            let pointer = cocoa_mouse();
            if drag.horizontal.is_none() && (current.w - drag.initial.w).abs() >= 1.0 {
                drag.horizontal = Some(HorizontalResize::start(current, drag.initial.w, pointer.0));
            }
            if let Some(horizontal) = drag.horizontal
                && let Some(visible) = screen_visible_containing(
                    drag.initial.x + drag.initial.w / 2.0,
                    drag.initial.max_y(),
                )
            {
                let frame = horizontal.frame(current, pointer.0, visible);
                if (frame.x - current.x).abs() >= 0.5 || (frame.w - current.w).abs() >= 0.5 {
                    window.setFrame_display(
                        NSRect {
                            origin: NSPoint::new(frame.x, frame.y),
                            size: NSSize::new(frame.w, frame.h),
                        },
                        true,
                    );
                }
            }
        }
    });
    // SAFETY: the filter is our NSWindow; a nil queue runs on the posting
    // thread. The block captures only a Send mutex and an event-loop proxy.
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            None,
            Some(window),
            None,
            &block,
        )
    };
    Some(WindowResizeObserver(token))
}

fn in_live_resize(window: &Window) -> bool {
    let ptr = window.ns_window() as *mut NSWindow;
    // SAFETY: tao owns the NSWindow for the lifetime of `window`.
    unsafe { ptr.as_ref() }.is_some_and(|window| window.inLiveResize())
}

/// Back to the default width and a fully automatic height.
fn reset_panel_size(state: &mut TrayState) {
    state.panel_size = PanelSize::reset();
    state.panel_size_dirty = true;
    save_panel_size(state);
    let visible_h = anchor_visible_height(state.last_anchor);
    fit_window_to_content(state, visible_h);
}

fn panel_size_path() -> Option<std::path::PathBuf> {
    Some(
        crate::cache::xdg_cache_dir()
            .ok()?
            .join("ai-usagebar")
            .join("tray-panel.json"),
    )
}

fn load_panel_size() -> PanelSize {
    panel_size_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| PanelSize::parse(&bytes))
        .unwrap_or_default()
}

/// Best-effort: a size that fails to save only means the next run starts at
/// the default size.
fn save_panel_size(state: &mut TrayState) {
    if !state.panel_size_dirty {
        return;
    }
    let (Some(path), Ok(bytes)) = (panel_size_path(), serde_json::to_vec(&state.panel_size)) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Stay dirty on failure, so the next close or quit tries again.
    if crate::cache::atomic_write(&path, &bytes).is_ok() {
        state.panel_size_dirty = false;
    }
}

fn apply_theme(state: &mut TrayState, theme: Theme) {
    state.theme = theme;
    let ptr = state.window.ns_window() as *mut NSWindow;
    if let Some(window) = unsafe { ptr.as_ref() } {
        // SAFETY: AppKit exports these immutable appearance names for the
        // lifetime of the process.
        let name = unsafe {
            match theme {
                Theme::Light => NSAppearanceNameAqua,
                Theme::Dark => NSAppearanceNameDarkAqua,
            }
        };
        if let Some(appearance) = NSAppearance::appearanceNamed(name) {
            window.setAppearance(Some(&appearance));
        }
    }
}

fn anchor_visible_height(anchor: Option<(f64, f64)>) -> f64 {
    let (x, y) = anchor.unwrap_or((0.0, 0.0));
    screen_visible_containing(x, y)
        .map(|visible| visible.h)
        .unwrap_or(FALLBACK_WORK_AREA_HEIGHT)
}

fn toggle_startup(state: &mut TrayState) {
    let next = !startup::is_enabled();
    if startup::set_enabled(next).is_ok() {
        if let Some(obj) = state.payload.as_object_mut() {
            obj.insert("startup_enabled".into(), Value::Bool(next));
        }
        if state.js_ready {
            push_to_webview(state);
        }
    }
}

/// Shows the popover under the status item and guards it against the blur
/// that opening and focusing it can cause.
fn show_popover(state: &mut TrayState) {
    state.show_pending = false;
    if state.last_anchor.is_none() {
        state.last_anchor = Some(cocoa_mouse());
    }
    position_popover(state);
    guard_blur(state);
    state.window.set_visible(true);
    state.popover_open = true;
    mark_open_item(state);
    if let Some(webview) = state.webview.as_ref() {
        let _ = webview.evaluate_script(&format!(
            "window.__AIUB_LOCKCLICKS__ && window.__AIUB_LOCKCLICKS__({CLICK_LOCK_MS})"
        ));
    }
    sync_popover_visibility(state);
    if state.js_ready {
        push_to_webview(state);
    }
    let proxy = state.proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(CLICK_LOCK_MS));
        let _ = proxy.send_event(UserEvent::FocusPopover);
    });
}

/// A global monitor sees presses delivered to other processes: another app,
/// another status item, the empty menu bar, the desktop. Those take no focus
/// from the popover when they land in the menu bar, so without this the
/// popover would stay open. The status item's own button is drawn out of
/// process on current macOS, so its presses reach the monitor too; the
/// handler leaves those to the item's click.
fn install_outside_click_monitor(proxy: EventLoopProxy<UserEvent>) -> Option<Retained<AnyObject>> {
    let mask =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let block = RcBlock::new(move |event: NonNull<NSEvent>| {
        let (x, y) = cocoa_mouse();
        // SAFETY: AppKit passes a live event for the duration of the call.
        let timestamp = unsafe { event.as_ref() }.timestamp();
        let _ = proxy.send_event(UserEvent::OutsideClick(x, y, timestamp));
    });
    let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &block);
    if monitor.is_none() {
        // Losing focus still closes the popover; only presses that take no
        // focus (the menu bar, another status item) will not.
        eprintln!(
            "ai-usagebar-tray: could not watch clicks outside the popover; \
             clicks in the menu bar will not close it"
        );
    }
    monitor
}

/// Whether a mouse button is down over our status item right now.
fn press_on_status_item(state: &TrayState) -> bool {
    if NSEvent::pressedMouseButtons() == 0 {
        return false;
    }
    let (x, y) = cocoa_mouse();
    status_item_frames(state)
        .iter()
        .any(|frame| frame.contains(x, y))
}

/// The chart item's frame and every provider item's, in Cocoa screen space.
fn status_item_frames(state: &TrayState) -> Vec<CocoaRect> {
    status_item_frame(&state.tray)
        .into_iter()
        .chain(
            state
                .provider_items
                .frames()
                .into_iter()
                .map(|(_, frame)| ns_rect_to_cocoa(frame)),
        )
        .collect()
}

/// The status item's frame in Cocoa screen space, like `NSEvent::mouseLocation`.
fn status_item_frame(tray: &TrayIcon) -> Option<CocoaRect> {
    let mtm = MainThreadMarker::new()?;
    let window = tray.ns_status_item()?.button(mtm)?.window()?;
    Some(ns_rect_to_cocoa(window.frame()))
}

/// Whether the monitor saw a press on the status item just now, so the blur
/// that follows belongs to that click.
fn status_item_just_pressed(state: &TrayState) -> bool {
    state
        .status_item_pressed_at
        .is_some_and(|at| at.elapsed() < STATUS_ITEM_PRESS_WINDOW)
}

const STATUS_ITEM_PRESS_WINDOW: Duration = Duration::from_millis(500);

/// Whether the popover was opened or focused too recently for a lost focus
/// to mean the user left it.
fn blur_guarded(state: &TrayState) -> bool {
    state
        .blur_guard_until
        .is_some_and(|until| Instant::now() < until)
}

const BLUR_GUARD: Duration = Duration::from_millis(400);

/// Ignores focus loss for the next `BLUR_GUARD`.
fn guard_blur(state: &mut TrayState) {
    state.blur_guard_until = Some(Instant::now() + BLUR_GUARD);
}

fn hide_popover(state: &mut TrayState) {
    cancel_chart_press(state);
    state.window.set_visible(false);
    state.popover_open = false;
    state.focused_provider = None;
    // A provider click still waiting to show must not reopen what was closed.
    state.show_pending = false;
    state.presentation_revision += 1;
    mark_open_item(state);
    save_panel_size(state);
    sync_popover_visibility(state);
}

/// Visibility and the provider come from the same native state. Reopening
/// must restore the selection even if an earlier close reset the renderer.
fn sync_popover_visibility(state: &TrayState) {
    if let Some(webview) = state.webview.as_ref() {
        let provider =
            serde_json::to_string(&state.focused_provider).unwrap_or_else(|_| "null".into());
        let visible = state.popover_open;
        let screen = state.presentation_screen;
        let _ = webview.evaluate_script(&format!(
            "window.__AIUB_VISIBLE__ && window.__AIUB_VISIBLE__({visible}, {provider}, '{screen}')"
        ));
    }
}

fn fitted_popover_height(state: &TrayState, visible_h: f64) -> f64 {
    let fit = if state.compact_popover {
        fit_provider_popover_height
    } else {
        fit_popover_height
    };
    fit(state.popover_height, visible_h, state.panel_size.max_height)
}

fn toggle_popover_from_keyboard(state: &mut TrayState) {
    state.status_item_pressed_at = None;
    if state.popover_open || state.show_pending {
        hide_popover(state);
    } else {
        prepare_popover(state, None);
    }
}

fn position_popover(state: &mut TrayState) {
    let (icon_x, icon_y) = state.last_anchor.unwrap_or_else(cocoa_mouse);
    let (screen, visible) = screen_pair_containing(icon_x, icon_y).unwrap_or((
        CocoaRect {
            x: 0.0,
            y: 0.0,
            w: 1440.0,
            h: FALLBACK_WORK_AREA_HEIGHT,
        },
        CocoaRect {
            x: 0.0,
            y: 0.0,
            w: 1440.0,
            h: FALLBACK_WORK_AREA_HEIGHT,
        },
    ));
    let below_y = status_bar_bottom_y(screen).unwrap_or_else(|| menu_bar_bottom_y(screen, visible));
    let height = fitted_popover_height(state, visible.h);
    let frame = cocoa_popover_frame(PopoverPlacement {
        visible,
        below_y,
        icon_x,
        popover_w: state.panel_size.width,
        popover_h: height,
    });
    apply_cocoa_frame(&state.window, frame);
}

fn build_tray() -> Result<TrayIcon, String> {
    let icon = static_icon().map_err(|error| error.to_string())?;
    // NSStatusItem.setMenu intercepts clicks even when the tray-icon menu-on-
    // click flags are false. Keep the status item menu-free so both mouse
    // buttons reach handle_tray and open the WKWebView panel.
    TrayIconBuilder::new()
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(false)
        .build()
        .map_err(|error| error.to_string())
}

fn build_webview(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Result<WebView, String> {
    // Stable WKWebsiteDataStore so Customize layout / stars survive restarts
    // (wry has no data_directory on macOS; this is the Darwin stand-in).
    const STORE: [u8; 16] = [
        0xa1, 0x05, 0xa6, 0xeb, 0x74, 0x72, 0x61, 0x79, 0x77, 0x65, 0x62, 0x76, 0x61, 0x69, 0x75,
        0x62,
    ];
    WebViewBuilder::new()
        .with_custom_protocol("aiub".into(), move |_id, request| {
            protocol_response(request)
        })
        // WKWebView registers the custom scheme as `aiub://` (WebView2 uses
        // `http://aiub.localhost/` instead).
        .with_url("aiub://localhost/index.html")
        .with_ipc_handler(move |request| {
            match ipc::accept(&request.uri().to_string(), request.body()) {
                Ok(command) => {
                    let _ = proxy.send_event(UserEvent::Ipc(command));
                }
                // A static reason only: the body can carry account labels.
                Err(rejection) => eprintln!(
                    "ai-usagebar-tray: ignored a popover message ({})",
                    rejection.reason()
                ),
            }
        })
        // This page drives privileged host commands, so it never leaves the
        // embedded protocol; web links go through `open-url` to the browser.
        .with_navigation_handler(|url| ipc::trusted_origin(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_transparent(true)
        .with_background_color((0, 0, 0, 0))
        .with_accept_first_mouse(true)
        .with_data_store_identifier(STORE)
        .build(window)
        .map_err(|error| error.to_string())
}

fn protocol_response(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    assets::response(request)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn cocoa_mouse() -> (f64, f64) {
    let point = NSEvent::mouseLocation();
    (point.x, point.y)
}

fn ns_rect_to_cocoa(rect: NSRect) -> CocoaRect {
    CocoaRect {
        x: rect.origin.x,
        y: rect.origin.y,
        w: rect.size.width,
        h: rect.size.height,
    }
}

fn screen_visible_containing(x: f64, y: f64) -> Option<CocoaRect> {
    screen_pair_containing(x, y).map(|(_, visible)| visible)
}

fn screen_pair_containing(x: f64, y: f64) -> Option<(CocoaRect, CocoaRect)> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let mut fallback = None;
    for screen in screens.iter() {
        let frame = ns_rect_to_cocoa(screen.frame());
        let visible = ns_rect_to_cocoa(screen.visibleFrame());
        if fallback.is_none() {
            fallback = Some((frame, visible));
        }
        if frame.contains(x, y) {
            return Some((frame, visible));
        }
    }
    fallback
}

/// Bottom edge of the menu bar on `screen`, in Cocoa Y (the popover hangs under this).
fn status_bar_bottom_y(screen: CocoaRect) -> Option<f64> {
    let mtm = MainThreadMarker::new()?;
    let expected = AnyClass::get(c"NSStatusBarWindow")?;
    let app = NSApplication::sharedApplication(mtm);
    let mut best: Option<f64> = None;
    for window in app.windows().iter() {
        if window.class() != expected {
            continue;
        }
        let frame = ns_rect_to_cocoa(window.frame());
        let cx = frame.x + frame.w / 2.0;
        let cy = frame.y + frame.h / 2.0;
        if screen.contains(cx, cy) || (frame.max_y() - screen.max_y()).abs() < 2.0 {
            best = Some(frame.y);
        }
    }
    best
}

/// Match Windows 11 `DWMWCP_ROUND` / OpenUsage's 13pt continuous corners.
/// The React shell is shared; this is the native window clip WKWebView won't
/// get from CSS alone.
fn round_corners(window: &Window) {
    let ptr = window.ns_window() as *mut NSWindow;
    if ptr.is_null() {
        return;
    }
    let ns_window = unsafe { &*ptr };
    // This reused popover owns its content animation. AppKit's inferred
    // orderFront/orderOut animation can expose the previous position/surface.
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    ns_window.setOpaque(false);
    ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
    if let Some(view) = ns_window.contentView() {
        round_view(&view);
        for sub in view.subviews().iter() {
            round_view(&sub);
        }
    }
    let ns_view = window.ns_view() as *mut NSView;
    if !ns_view.is_null() {
        round_view(unsafe { &*ns_view });
    }
}

/// Put AppKit's material behind WKWebView. On systems with Liquid Glass, use
/// NSGlassEffectView; older macOS versions use the semantic popover material.
fn install_glass_background(window: &Window) {
    let ptr = window.ns_window() as *mut NSWindow;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(ns_window) = (unsafe { ptr.as_ref() }) else {
        return;
    };
    let Some(content) = ns_window.contentView() else {
        return;
    };
    let sizing =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;

    if AnyClass::get(c"NSGlassEffectView").is_some() {
        let glass = NSGlassEffectView::new(mtm);
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setFrame(content.bounds());
        glass.setAutoresizingMask(sizing);
        round_view(&glass);
        content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, None);
    } else {
        let material = NSVisualEffectView::new(mtm);
        material.setMaterial(NSVisualEffectMaterial::Popover);
        material.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        material.setState(NSVisualEffectState::Active);
        material.setFrame(content.bounds());
        material.setAutoresizingMask(sizing);
        round_view(&material);
        content.addSubview_positioned_relativeTo(&material, NSWindowOrderingMode::Below, None);
    }
}

fn round_view(view: &NSView) {
    view.setWantsLayer(true);
    let Some(layer) = view.layer() else {
        return;
    };
    layer.setCornerRadius(CORNER_RADIUS);
    layer.setMasksToBounds(true);
    // SAFETY: `kCACornerCurveContinuous` is a process-lifetime CFString.
    unsafe { layer.setCornerCurve(kCACornerCurveContinuous) };
}

fn apply_cocoa_frame(window: &Window, frame: CocoaRect) {
    let ptr = window.ns_window() as *mut NSWindow;
    if ptr.is_null() {
        return;
    }
    let ns_window = unsafe { &*ptr };
    let rect = NSRect {
        origin: NSPoint::new(frame.x, frame.y),
        size: NSSize::new(frame.w, frame.h),
    };
    ns_window.setFrame_display(rect, true);
    sync_screen_properties(ns_window);
}

/// Match the destination display's backing scale and color space so WKWebView
/// text and the status-item glyph aren't painted at the primary's scale.
fn sync_screen_properties(ns_window: &NSWindow) {
    let scale = ns_window
        .screen()
        .map(|screen| {
            if let Some(space) = screen.colorSpace() {
                ns_window.setColorSpace(Some(&space));
            }
            let scale = screen.backingScaleFactor();
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                ns_window.backingScaleFactor()
            }
        })
        .unwrap_or_else(|| ns_window.backingScaleFactor());
    if let Some(view) = ns_window.contentView() {
        apply_contents_scale(&view, scale);
        for sub in view.subviews().iter() {
            apply_contents_scale(&sub, scale);
            for nested in sub.subviews().iter() {
                apply_contents_scale(&nested, scale);
            }
        }
    }
}

fn apply_contents_scale(view: &NSView, scale: f64) {
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setContentsScale(scale);
        layer.setNeedsDisplay();
    }
    view.setNeedsDisplay(true);
}

/// Optional usage chart, rasterized by AppKit at the display's native scale.
fn template_bars_image(fractions: &[f64]) -> Option<Retained<NSImage>> {
    if fractions.is_empty() {
        return None;
    }
    let fractions = fractions.to_vec();
    let point = f64::from(BARS_POINT_SIDE);
    let block = RcBlock::new(move |dst: NSRect| {
        draw_template_bars(dst, &fractions);
        Bool::from(true)
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(point, point), true, &block);
    image.setTemplate(true);
    Some(image)
}

fn draw_template_bars(dst: NSRect, fractions: &[f64]) {
    let side = dst.size.width.min(dst.size.height).max(1.0);
    let layout = bars_layout(fractions.len(), side);
    let ox = dst.origin.x;
    let oy = dst.origin.y;
    for (i, fraction) in fractions.iter().copied().take(layout.n).enumerate() {
        let y = oy + layout.y_offset + i as f64 * (layout.track_h + layout.gap) + 1.0;
        fill_round_rect(
            ox + layout.track_x,
            y,
            layout.track_w,
            layout.track_h,
            layout.rx,
            0.16,
        );
        let fill = bar_fill(layout.track_w, fraction);
        if fill.fill_w > 0.0 {
            let trailing = if fill.fill_w >= layout.track_w {
                layout.rx
            } else {
                (layout.rx * 0.35).floor().max(0.0)
            };
            fill_round_rect(
                ox + layout.track_x,
                y,
                fill.fill_w,
                layout.track_h,
                trailing.min(layout.rx),
                1.0,
            );
        }
        if fill.fill_w > 0.0
            && fill.remainder_w > 0.0
            && let Some(divider_x) = fill.divider_x
        {
            fill_round_rect(
                ox + layout.track_x + divider_x,
                y,
                fill.remainder_w,
                layout.track_h,
                layout.rx,
                0.24,
            );
        }
    }
}

fn fill_round_rect(x: f64, y: f64, w: f64, h: f64, radius: f64, alpha: f64) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let radius = radius.max(0.0).min(h * 0.5).min(w * 0.5);
    let rect = NSRect {
        origin: NSPoint::new(x, y),
        size: NSSize::new(w, h),
    };
    NSColor::colorWithWhite_alpha(0.0, alpha).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
}

/// The chart glyph on tray-icon's own item. Looking the button up among the
/// app's windows would now find a provider item as easily as this one.
/// Keep the item that opened the popover highlighted while it is open, like a
/// native status item's menu: the provider's own item, or the chart glyph.
fn mark_open_item(state: &TrayState) {
    // A pending provider counts as open, as the chart does: a redraw before
    // its tab is measured must not end the session its click began.
    let provider = (state.popover_open || state.show_pending)
        .then_some(state.focused_provider.as_deref())
        .flatten();
    let chart = (state.popover_open || state.show_pending) && state.focused_provider.is_none();
    if let Some(button) = MainThreadMarker::new().and_then(|mtm| {
        state
            .tray
            .ns_status_item()
            .and_then(|item| item.button(mtm))
    }) {
        let proxy = state.proxy.clone();
        status_items::install_chart_button(&button, move |action| {
            let _ = proxy.send_event(UserEvent::Chart(action));
        });
        status_items::fit_chart_button(&button);
        // With a session the menu bar draws the capsule; this app draws the
        // same shape only when it opens without one (the keyboard shortcut).
        let system = state.chart_session.as_ref().is_some_and(|session| {
            status_items::use_system_highlight(&button);
            session.active()
        });
        status_items::mark_open(&button, chart && !system);
    }
    if let Some(session) = &state.chart_session {
        session.sync(chart);
    }
    state
        .provider_items
        .highlight(provider.and_then(|id| state.provider_items.index_of(id)));
}

fn cancel_chart_press(state: &TrayState) {
    if let Some(button) =
        MainThreadMarker::new().and_then(|mtm| state.tray.ns_status_item()?.button(mtm))
    {
        status_items::cancel_chart_press(&button);
    }
}

fn set_status_button_image(tray: &TrayIcon, image: &NSImage) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(button) = tray.ns_status_item().and_then(|item| item.button(mtm)) else {
        return;
    };
    button.setImageScaling(NSImageScaling::ScaleNone);
    button.setImage(Some(image));
}

struct SingleInstance {
    // Held for the process lifetime so the exclusive flock stays taken.
    _lock: File,
}

impl SingleInstance {
    fn acquire() -> Option<Self> {
        let dir = crate::cache::xdg_cache_dir().ok()?.join("ai-usagebar");
        std::fs::create_dir_all(&dir).ok()?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dir.join("tray.lock"))
            .ok()?;
        file.try_lock_exclusive().ok()?;
        let _ = writeln!(&file, "{}", std::process::id());
        Some(Self { _lock: file })
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;

    #[tokio::test]
    async fn targeted_refresh_omits_identity_changed_while_collecting_usage() {
        let facts = Arc::new(Mutex::new(HostFacts::default()));
        with_facts(&facts, |f| {
            f.account_emails
                .insert("openai".into(), "old@example.test".into());
        });
        let mut reads = 0;
        let entry = refreshed_entry_with(
            "openai",
            || {
                if reads == 0 {
                    with_facts(&facts, |f| {
                        f.account_emails
                            .insert("openai".into(), "new@example.test".into());
                    });
                }
                reads += 1;
                facts_snapshot(&facts)
            },
            async {
                assert_eq!(
                    facts_snapshot(&facts).account_emails["openai"],
                    "new@example.test"
                );
                // Later UI facts must not relabel this in-flight result.
                with_facts(&facts, |f| {
                    f.account_emails
                        .insert("openai".into(), "later@example.test".into());
                });
                Ok(r#"{"entries":[{"id":"openai","status":"ready"}]}"#.into())
            },
        )
        .await
        .unwrap();
        assert!(entry.get("email").is_none());
        assert_eq!(reads, 2);
    }

    #[tokio::test]
    async fn targeted_refresh_without_a_current_identity_omits_email() {
        let entry = refreshed_entry_with("openai", HostFacts::default, async {
            Err("signed out".into())
        })
        .await
        .unwrap();
        assert_eq!(entry["id"], "openai");
        assert_eq!(entry["status"], "error");
        assert!(entry.get("email").is_none());
    }

    fn measurement(revision: u64, provider: &str, screen: &str) -> Measurement {
        let body = json!({
            "cmd": "resize", "height": 317, "compact": false, "theme": "dark",
            "revision": revision, "provider": provider, "screen": screen
        });
        match ipc::accept("aiub://localhost/index.html", &body.to_string()) {
            Ok(Command::Resize(measurement)) => measurement,
            other => panic!("resize fixture must parse: {other:?}"),
        }
    }

    #[test]
    fn opening_rejects_the_previous_provider_height() {
        assert!(!current_panel_measurement(
            &measurement(2, "anthropic", "dashboard"),
            2,
            Some("openai"),
            true,
            "dashboard"
        ));
    }

    #[test]
    fn reopening_rejects_a_delayed_measurement_of_the_same_provider() {
        assert!(!current_panel_measurement(
            &measurement(1, "openai", "dashboard"),
            3,
            Some("openai"),
            true,
            "dashboard"
        ));
    }

    #[test]
    fn opening_waits_for_dashboard_but_navigation_can_resize_afterward() {
        let settings = measurement(2, "openai", "provider");
        assert!(!current_panel_measurement(
            &settings,
            2,
            Some("openai"),
            true,
            "dashboard"
        ));
        assert!(current_panel_measurement(
            &settings,
            2,
            Some("openai"),
            false,
            "dashboard"
        ));
        assert!(current_panel_measurement(
            &measurement(2, "openai", "dashboard"),
            2,
            Some("openai"),
            true,
            "dashboard"
        ));
        assert!(current_panel_measurement(
            &measurement(4, "", "dashboard"),
            4,
            None,
            true,
            "dashboard"
        ));
    }

    #[test]
    fn context_menu_settings_waits_for_its_own_screen() {
        assert!(!current_panel_measurement(
            &measurement(5, "", "dashboard"),
            5,
            None,
            true,
            "settings"
        ));
        assert!(current_panel_measurement(
            &measurement(5, "", "settings"),
            5,
            None,
            true,
            "settings"
        ));
        assert_eq!(
            chart_menu_command(MENU_CHART_REFRESH),
            Some(Command::Refresh {})
        );
        assert_eq!(
            chart_menu_command(MENU_CHART_SETTINGS),
            Some(Command::OpenSettings {})
        );
        assert_eq!(chart_menu_command(MENU_CHART_QUIT), Some(Command::Quit {}));
        for tag in [1, 2, 3, 4, MENU_OPEN, MENU_CENTERED, MENU_HIDE] {
            assert_eq!(
                chart_menu_command(tag),
                None,
                "provider actions keep their routing"
            );
        }
    }

    #[test]
    fn each_menu_line_keeps_its_own_tag() {
        let mut tags = vec![
            MENU_CHART_REFRESH,
            MENU_CHART_SETTINGS,
            MENU_CHART_QUIT,
            MENU_CHART_TOGGLE_VALUE,
            MENU_CHART_TOGGLE_COLOR,
            MENU_CHART_ACTIVE_ACCOUNT_ONLY,
            MENU_CHART_CENTERED,
            MENU_TOGGLE_VALUE,
            MENU_HIDE,
            MENU_ACTIVE_ACCOUNT_ONLY,
            MENU_OPEN,
            MENU_TOGGLE_COLOR,
            MENU_CENTERED,
            MENU_ACCESSIBILITY,
        ];
        tags.extend(MENU_CHART_WINDOWS.iter().map(|(tag, _)| *tag));
        tags.extend(MENU_WINDOWS.iter().map(|(tag, _)| *tag));
        let count = tags.len();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(
            tags.len(),
            count,
            "the chart and provider menus share one target"
        );
    }

    #[test]
    fn a_global_option_changes_what_the_menu_bar_shows() {
        let bar = ChartMenuState {
            all_show: true,
            all_colored: false,
            window: None,
            active_only: false,
            centered: true,
        };
        assert_eq!(
            chart_global_pick(MENU_CHART_TOGGLE_VALUE, bar),
            Some((
                Command::SetMenuBarHideValue { value: true },
                Some("hide_value")
            ))
        );
        // Some provider hides its value: the pick shows it everywhere.
        assert_eq!(
            chart_global_pick(
                MENU_CHART_TOGGLE_VALUE,
                ChartMenuState {
                    all_show: false,
                    ..bar
                }
            ),
            Some((
                Command::SetMenuBarHideValue { value: false },
                Some("hide_value")
            ))
        );
        assert_eq!(
            chart_global_pick(MENU_CHART_TOGGLE_COLOR, bar),
            Some((
                Command::SetMenuBarColorValue { value: true },
                Some("color_value")
            ))
        );
        assert_eq!(
            chart_global_pick(MENU_CHART_ACTIVE_ACCOUNT_ONLY, bar),
            Some((Command::SetMenuBarActiveAccountOnly { value: true }, None))
        );
        assert_eq!(
            chart_global_pick(MENU_CHART_CENTERED, bar),
            Some((Command::SetMenuBarCentered { value: false }, None))
        );
        for (tag, value) in MENU_CHART_WINDOWS {
            assert_eq!(
                chart_global_pick(tag, bar),
                Some((Command::SetMenuBarWindow { value }, Some("window")))
            );
        }
        for tag in (1..=16).chain(MENU_CHART_REFRESH..=MENU_CHART_QUIT) {
            assert_eq!(chart_global_pick(tag, bar), None, "tag {tag}");
        }
    }

    #[test]
    fn an_option_is_on_only_when_every_provider_shown_has_it() {
        let hidden = MenuBarItemConfig {
            hide_value: Some(true),
            ..Default::default()
        };
        let plain = MenuBarItemConfig::default();
        let shows = |item: &MenuBarItemConfig| item.hide_value.map(|hide| !hide);
        // A provider that hid its value keeps the option off.
        assert!(!all_on(&[Some(&hidden), None], true, shows));
        assert!(all_on(&[Some(&plain), None], true, shows));
        assert!(!all_on(&[Some(&plain)], false, shows));
        // With no provider in the menu bar, the global value stands.
        assert!(all_on(&[], true, shows));
        assert!(!all_on(&[], false, shows));
    }

    #[test]
    fn a_provider_following_the_menu_bar_checks_that_option() {
        let own = |window: &str| {
            menu_bar::own_window(&MenuBarItemConfig {
                window: Some(window.into()),
                ..Default::default()
            })
        };
        // "auto" written by hand follows the menu bar, as no window does.
        assert!(window_checked(own("auto"), UsageWindow::Auto));
        assert!(window_checked(
            menu_bar::own_window(&MenuBarItemConfig::default()),
            UsageWindow::Auto
        ));
        assert!(window_checked(own("weekly"), UsageWindow::Weekly));
        assert!(!window_checked(own("weekly"), UsageWindow::Auto));
        assert!(!window_checked(own("auto"), UsageWindow::Weekly));
    }

    #[test]
    fn a_window_is_checked_when_every_provider_shown_reads_it() {
        let weekly = MenuBarItemConfig {
            window: Some("weekly".into()),
            ..Default::default()
        };
        let global = MenuBarItemConfig {
            window: Some("auto".into()),
            ..Default::default()
        };
        // A provider's own window that matches the global one.
        assert_eq!(
            same_window(&[Some(&weekly), None], UsageWindow::Weekly),
            Some(UsageWindow::Weekly)
        );
        assert_eq!(
            same_window(&[Some(&weekly), Some(&global)], UsageWindow::Session),
            None
        );
        assert_eq!(
            same_window(&[Some(&global)], UsageWindow::Session),
            Some(UsageWindow::Session)
        );
        assert_eq!(
            same_window(&[], UsageWindow::Monthly),
            Some(UsageWindow::Monthly)
        );
    }

    #[test]
    fn a_global_pick_keeps_the_choices_it_cannot_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let command = Command::SetMenuBarWindow {
            value: WindowChoice::Session,
        };
        let own = MenuBarItemConfig {
            window: Some("monthly".into()),
            ..Default::default()
        };
        let mut items = std::collections::BTreeMap::from([("zai".to_owned(), own.clone())]);
        // A config that cannot be read: nothing is saved or dropped.
        let text = "[tray\nmenu_bar_window = \"weekly\"\n";
        std::fs::write(&path, text).unwrap();
        assert!(!replace_own_choices(
            &mut items,
            "window",
            &command,
            Some(&path)
        ));
        assert_eq!(items["zai"], own);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        // Saved: the choice goes from memory and from the file.
        std::fs::write(&path, "[tray.menu_bar_items.zai]\nwindow = \"monthly\"\n").unwrap();
        assert!(replace_own_choices(
            &mut items,
            "window",
            &command,
            Some(&path)
        ));
        assert!(items.is_empty());
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("menu_bar_window = \"session\""), "{saved}");
        assert!(!saved.contains("monthly"), "{saved}");
    }

    #[test]
    fn a_global_option_drops_only_the_providers_own_choice_for_it() {
        let mut items = std::collections::BTreeMap::new();
        items.insert(
            "openai@work".to_owned(),
            MenuBarItemConfig {
                window: Some("weekly".into()),
                ..Default::default()
            },
        );
        items.insert(
            "zai".to_owned(),
            MenuBarItemConfig {
                hide_value: Some(true),
                color_value: Some(false),
                ..Default::default()
            },
        );
        items.insert(
            "kimi".to_owned(),
            MenuBarItemConfig {
                hidden: true,
                ..Default::default()
            },
        );
        clear_own_choice(&mut items, "window");
        // openai@work had only a window of its own.
        assert_eq!(items.keys().collect::<Vec<_>>(), ["kimi", "zai"]);
        clear_own_choice(&mut items, "hide_value");
        assert_eq!(items["zai"].hide_value, None);
        assert_eq!(items["zai"].color_value, Some(false));
        clear_own_choice(&mut items, "hidden");
        assert!(
            items["kimi"].hidden,
            "hiding a provider is not a global option"
        );
    }

    fn report() -> Value {
        json!({"entries": [
            {"id": "anthropic", "status": "ready"},
            {"id": "openai:work", "status": "error"}
        ]})
    }

    #[test]
    fn entry_commands_act_only_on_entries_in_the_report() {
        let report = report();
        assert!(report_has_entry(&report, "anthropic"));
        assert!(report_has_entry(&report, "openai:work"));
        for id in ["openai", "anthropic ", "zai", ""] {
            assert!(!report_has_entry(&report, id), "{id}");
        }
        assert!(!report_has_entry(&json!({}), "anthropic"));
        assert!(!report_has_entry(
            &json!({"entries": "anthropic"}),
            "anthropic"
        ));
    }

    #[test]
    fn menu_bar_provider_keeps_its_eligibility_rules() {
        let report = report();
        let shown = ["openai:work".to_string()];
        assert!(menu_bar_provider_eligible(&report, None, "highest"));
        assert!(menu_bar_provider_eligible(&report, Some(&[]), "highest"));
        // Before the popover sends its layout, any report entry is eligible.
        assert!(menu_bar_provider_eligible(&report, None, "anthropic"));
        assert!(menu_bar_provider_eligible(
            &report,
            Some(&shown),
            "openai:work"
        ));
        // A card hidden in the popover, or one missing from the report, is not.
        assert!(!menu_bar_provider_eligible(
            &report,
            Some(&shown),
            "anthropic"
        ));
        assert!(!menu_bar_provider_eligible(&report, None, "zai"));
    }

    #[test]
    fn menu_bar_item_values_match_the_previous_config_writes() {
        use menu_bar::UsageWindow;
        assert!(menu_bar_item_value(ItemSetting::Window(None)).is_none());
        assert_eq!(
            menu_bar_item_value(ItemSetting::Window(Some(UsageWindow::Weekly)))
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("weekly".to_string())
        );
        for setting in [
            ItemSetting::Hidden(false),
            ItemSetting::HideValue(true),
            ItemSetting::ColorValue(false),
        ] {
            let flag = match setting {
                ItemSetting::Hidden(flag)
                | ItemSetting::HideValue(flag)
                | ItemSetting::ColorValue(flag) => flag,
                ItemSetting::Window(_) => unreachable!(),
            };
            assert_eq!(
                menu_bar_item_value(setting).and_then(|v| v.as_bool()),
                Some(flag)
            );
        }
    }

    #[test]
    fn ipc_request_uri_of_the_embedded_page_is_trusted() {
        // wry hands the frame URL to the IPC handler through `http::Uri`; the
        // round trip must keep the origin `ipc::accept` compares against.
        let request = Request::builder()
            .uri("aiub://localhost/index.html")
            .body(String::new())
            .unwrap();
        assert!(ipc::trusted_origin(&request.uri().to_string()));
        let foreign = Request::builder()
            .uri("https://aiub.localhost/index.html")
            .body(String::new())
            .unwrap();
        assert!(!ipc::trusted_origin(&foreign.uri().to_string()));
    }
}
