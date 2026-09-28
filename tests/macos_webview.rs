//! Native smoke test of the actual embedded frontend and its privileged boundary.
//! Uses a hidden, nonpersistent WKWebView and synthetic report data only.
//! Opt in from a macOS graphical session with `--run-native`.

#[cfg(target_os = "macos")]
use ai_usagebar::config;

// Include the production boundary, without copying its types or validators.
// These modules contain other helpers the standalone harness does not call.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/tray/assets.rs"]
mod assets;
#[cfg(target_os = "macos")]
// The source's #[test] functions are omitted with harness=false, leaving
// their imports unused. Production lints remain unchanged.
#[allow(dead_code, unused_imports)]
#[path = "../src/tray/browse.rs"]
mod browse;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/tray/ipc.rs"]
mod ipc;
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/tray/menu_bar.rs"]
mod menu_bar;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/tray/strip.rs"]
mod strip;

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use tao::{
        dpi::LogicalSize,
        event::Event,
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::{
            macos::{ActivationPolicy, EventLoopExtMacOS},
            run_return::EventLoopExtRunReturn,
        },
        window::WindowBuilder,
    };
    use wry::{NewWindowResponse, WebViewBuilder};

    if !std::env::args().any(|arg| arg == "--run-native") {
        println!("SKIP: WKWebView requires a macOS graphical session; opt in with --run-native");
        return;
    }

    #[derive(Debug)]
    enum Signal {
        Message(Result<ipc::Command, ipc::Rejection>),
        Snapshot(String),
        PolicyProbe(String),
        Navigation { allowed: bool },
        CogProbe,
        CogState(String),
    }

    let mut event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let window = WindowBuilder::new()
        .with_title("AI Usage test fixture")
        .with_inner_size(LogicalSize::new(420.0, 600.0))
        .with_visible(false)
        .with_focused(false)
        .build(&event_loop)
        .expect("fixture window");
    let messages = event_loop.create_proxy();
    let navigation = event_loop.create_proxy();
    let webview = WebViewBuilder::new()
        .with_incognito(true)
        .with_custom_protocol("aiub".into(), |_, request| assets::response(request))
        .with_url("aiub://localhost/index.html")
        .with_initialization_script(
            "window.__testViolations = [];\
             addEventListener('securitypolicyviolation', e => window.__testViolations.push(e.effectiveDirective));\
             window.__testErrors = [];\
             addEventListener('error', e => window.__testErrors.push('error: ' + e.message));\
             addEventListener('unhandledrejection', e => window.__testErrors.push('rejection: ' + String(e.reason)));\
             const consoleError = console.error.bind(console);\
             console.error = (...args) => { window.__testErrors.push('console: ' + args.map(String).join(' ')); consoleError(...args); };",
        )
        .with_ipc_handler(move |request| {
            let result = ipc::accept(&request.uri().to_string(), request.body());
            let _ = messages.send_event(Signal::Message(result));
        })
        .with_navigation_handler(move |url| {
            let allowed = ipc::trusted_origin(&url);
            let _ = navigation.send_event(Signal::Navigation { allowed });
            allowed
        })
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .build(&window)
        .expect("nonpersistent fixture WebView");

    let payload = json!({
        "os": "macos", "version": "test", "primary": "openai@fixture",
        "entries": [{
            "id": "openai@fixture", "name": "Codex", "short_name": "cdx",
            "email": "fixture@example.test", "status": "ready",
            "sections": [{"type": "metric", "label": "Weekly", "percent": 37,
                "value": "37%", "headline": "percent", "severity": "low"}]
        }, {
            // Shaped like `zai_sections`: plan, rolling windows and a monthly one.
            "id": "zai", "name": "Z.AI", "display_name": "Z.AI", "short_name": "zai",
            "brand": "zai", "plan": "GLM Coding Pro", "status": "ready", "stale": false,
            "sections": [
                {"type": "spacer"},
                {"type": "metric", "label": "Session (5h)", "percent": 12, "value": "12%",
                    "detail": "Resets in 3h · 40% elapsed · under", "headline": "percent",
                    "severity": "low", "reset_at": "2099-01-01T03:00:00Z", "window_secs": 18000},
                {"type": "spacer"},
                {"type": "metric", "label": "Weekly", "percent": 55, "value": "55%",
                    "detail": "Resets in 4d · 43% elapsed · over", "headline": "percent",
                    "severity": "medium", "reset_at": "2099-01-05T00:00:00Z", "window_secs": 604800},
                {"type": "spacer"},
                {"type": "metric", "label": "MCP tools (monthly)", "percent": 3, "value": "3%",
                    "detail": "Resets in 20d", "headline": "percent", "severity": "low",
                    "reset_at": "2099-01-20T00:00:00Z"}
            ]
        }]
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut initialized = false;
    let mut measured = false;
    let mut strip_received = false;
    let mut snapshot_sent = false;
    let mut snapshot = None;
    let mut policy_probe = None;
    let mut refreshed = false;
    let mut invalid_rejected = 0;
    let mut external_blocked = false;
    let mut zai_focused = false;
    let mut cog_clicked = false;
    let mut cog_state = None;
    let mut refocus_measured = false;
    let mut failures = Vec::new();
    let snapshots = event_loop.create_proxy();

    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(deadline);
        match event {
            Event::UserEvent(Signal::Message(Ok(ipc::Command::Ready {}))) if !initialized => {
                initialized = true;
                let script = format!(
                    "window.__AIUB_APPLY__({payload});\
                     window.__AIUB_FOCUS__('openai@fixture', 7);\
                     window.__AIUB_VISIBLE__(true, 'openai@fixture');"
                );
                if let Err(error) = webview.evaluate_script(&script) {
                    failures.push(error.to_string());
                }
            }
            Event::UserEvent(Signal::Message(Ok(ipc::Command::Strip(layout)))) => {
                strip_received |= layout.order.iter().any(|id| id == "openai@fixture");
            }
            Event::UserEvent(Signal::Message(Ok(ipc::Command::Resize(measurement)))) => {
                measured |= measurement.revision == 7
                    && measurement.provider() == "openai@fixture"
                    && measurement.compact;
                if measurement.revision == 8 && measurement.provider() == "zai" && !cog_clicked {
                    // The focused panel's own Settings button (MacPanelHeader),
                    // which opens the provider's customization in the WebView.
                    cog_clicked = true;
                    if let Err(error) = webview.evaluate_script(
                        "document.querySelector('.mac-icon-button[aria-label=\"Settings\"]').click();",
                    ) {
                        failures.push(error.to_string());
                    }
                    let probe = snapshots.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(700));
                        let _ = probe.send_event(Signal::CogProbe);
                    });
                }
                refocus_measured |= cog_state.is_some()
                    && measurement.revision == 9
                    && measurement.provider() == "openai@fixture"
                    && measurement.screen.as_str() == "dashboard";
            }
            Event::UserEvent(Signal::Message(Ok(ipc::Command::RefreshEntry { id }))) => {
                refreshed |= id.as_str() == "openai@fixture";
            }
            Event::UserEvent(Signal::Message(Err(ipc::Rejection::Malformed))) => {
                invalid_rejected += 1;
            }
            Event::UserEvent(Signal::Message(Err(rejection))) => {
                failures.push(format!("unexpected rejection: {}", rejection.reason()));
            }
            Event::UserEvent(Signal::Snapshot(result)) => {
                snapshot = Some(result);
                let policy_result = snapshots.clone();
                if let Err(error) = webview.evaluate_script_with_callback(
                    "(() => {\
                       const script = document.createElement('script');\
                       script.textContent = 'window.__injectedScriptRan = true';\
                       document.head.append(script);\
                       return JSON.stringify({injected: window.__injectedScriptRan === true});\
                     })()",
                    move |result_json| {
                        let _ = policy_result.send_event(Signal::PolicyProbe(result_json));
                    },
                ) {
                    failures.push(error.to_string());
                }
                if let Err(error) = webview.evaluate_script(
                    "document.querySelector('button[aria-label=\"Refresh\"]').click();\
                     window.ipc.postMessage('[\"quit\"]');\
                     window.ipc.postMessage('{\"cmd\":\"set-pinned\",\"value\":\"false\"}');\
                     window.location.href = 'https://blocked.example.test/';",
                ) {
                    failures.push(error.to_string());
                }
            }
            Event::UserEvent(Signal::PolicyProbe(result)) => policy_probe = Some(result),
            Event::UserEvent(Signal::Navigation { allowed: false }) => external_blocked = true,
            Event::UserEvent(Signal::CogProbe) => {
                let state = snapshots.clone();
                if let Err(error) = webview.evaluate_script_with_callback(
                    "JSON.stringify({\
                        errors: window.__testErrors,\
                        focusHook: typeof window.__AIUB_FOCUS__,\
                        visibleHook: typeof window.__AIUB_VISIBLE__,\
                        mounted: document.getElementById('root')?.childElementCount ?? -1,\
                        text: document.body.innerText.slice(0, 200)\
                     })",
                    move |result_json| {
                        let _ = state.send_event(Signal::CogState(result_json));
                    },
                ) {
                    failures.push(error.to_string());
                }
            }
            Event::UserEvent(Signal::CogState(result)) => {
                println!("cog state: {result}");
                cog_state = Some(result);
                // The host's next provider click: a new revision on the dashboard.
                if let Err(error) = webview.evaluate_script(
                    "window.__AIUB_FOCUS__ && window.__AIUB_FOCUS__('openai@fixture', 9, 'dashboard');\
                     window.__AIUB_VISIBLE__ && window.__AIUB_VISIBLE__(true, 'openai@fixture', 'dashboard');",
                ) {
                    failures.push(error.to_string());
                }
            }
            _ => {}
        }
        if measured && strip_received && !snapshot_sent {
            snapshot_sent = true;
            let result = snapshots.clone();
            if let Err(error) = webview.evaluate_script_with_callback(
                "JSON.stringify({\
                    identity: document.body.innerText.includes('fixture@example.test'),\
                    provider: document.body.innerText.includes('Codex'),\
                    overviewTabs: !!document.querySelector('.mac-provider-tabs'),\
                    styles: document.styleSheets.length,\
                    cardRadius: getComputedStyle(document.querySelector('.mac-provider-card')).borderRadius,\
                    violations: window.__testViolations\
                 })",
                move |result_json| { let _ = result.send_event(Signal::Snapshot(result_json)); },
            ) {
                failures.push(error.to_string());
            }
        }
        let boundary_done = snapshot.is_some()
            && policy_probe.is_some()
            && refreshed
            && invalid_rejected == 2
            && external_blocked;
        if boundary_done && !zai_focused {
            // A provider click on Z.AI, as `prepare_popover` then `show_popover` send it.
            zai_focused = true;
            if let Err(error) = webview.evaluate_script(
                "window.__AIUB_FOCUS__('zai', 8, 'dashboard');\
                 window.__AIUB_VISIBLE__(true, 'zai', 'dashboard');",
            ) {
                failures.push(error.to_string());
            }
        }
        if !failures.is_empty()
            || (boundary_done && refocus_measured)
            || Instant::now() >= deadline
        {
            *control_flow = ControlFlow::Exit;
        }
    });

    assert!(failures.is_empty(), "native boundary errors: {failures:?}");
    assert!(initialized, "production frontend never sent ready");
    assert!(strip_received, "fixture strip was not accepted");
    assert!(measured, "focused provider measurement was not accepted");
    let encoded: String = serde_json::from_str(&snapshot.expect("DOM proof returned")).unwrap();
    let dom: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(dom["identity"], true, "fixture identity rendered");
    assert_eq!(dom["provider"], true, "fixture provider rendered");
    assert_eq!(dom["overviewTabs"], false, "individual panel stays scoped");
    assert!(
        dom["styles"].as_u64().unwrap() > 0,
        "bundled stylesheet loaded"
    );
    assert_ne!(dom["cardRadius"], "0px", "bundled card styles applied");
    assert_eq!(
        dom["violations"],
        json!([]),
        "CSP permits the actual frontend"
    );
    let encoded: String = serde_json::from_str(&policy_probe.expect("CSP probe returned")).unwrap();
    let probe: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(probe["injected"], false, "CSP blocks injected inline code");
    assert!(refreshed, "real DOM button reaches typed refresh command");
    assert_eq!(
        invalid_rejected, 2,
        "malformed commands rejected in WKWebView"
    );
    assert!(
        external_blocked,
        "external navigation blocked before loading"
    );
    let encoded: String =
        serde_json::from_str(&cog_state.expect("state after the panel's Settings button")).unwrap();
    let cog: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        cog["errors"],
        json!([]),
        "provider customization renders without errors"
    );
    assert_eq!(cog["focusHook"], "function", "renderer still accepts focus");
    assert!(
        refocus_measured,
        "a later provider focus is measured after customization"
    );
    assert_eq!(webview.url().unwrap(), "aiub://localhost/index.html");
    println!(
        "PASS: real WKWebView frontend, styles, identity, focused resize, strip, refresh, IPC rejection, navigation boundary and focus after provider customization"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIP: this native WebView harness requires macOS");
}
