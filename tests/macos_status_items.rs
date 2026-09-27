//! AppKit layout and hit-testing need the process's main thread.
//! Run with `cargo test --test macos_status_items -- --run-native` in a macOS GUI session.

// Supply only the view's input data, without linking the full app: linking
// it alongside status_items would register the same Objective-C classes twice.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod menu_bar {
    #[derive(Clone, Copy)]
    pub enum Level {
        Green,
        Yellow,
        Red,
    }

    pub struct Chip {
        pub id: String,
        pub stale: bool,
        pub level: Option<Level>,
        pub active_account: bool,
        pub mark: Option<&'static str>,
        pub name: String,
        pub value: Option<String>,
    }
}
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/tray/menu_space.rs"]
mod menu_space;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/tray/status_items.rs"]
mod status_items;

#[cfg(target_os = "macos")]
fn main() {
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSButton, NSStatusBar, NSWindow,
    };
    use objc2_foundation::{MainThreadMarker, NSDate, NSPoint, NSRunLoop};
    use std::{cell::RefCell, rc::Rc};

    if !std::env::args().any(|arg| arg == "--run-native") {
        println!("SKIP: AppKit requires an interactive desktop; opt in with --run-native");
        return;
    }

    let mtm = MainThreadMarker::new().expect("AppKit test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let chart = tray_icon::TrayIconBuilder::new()
        .with_icon(tray_icon::Icon::from_rgba(vec![255; 16 * 16 * 4], 16, 16).unwrap())
        .with_menu_on_left_click(false)
        .build()
        .unwrap();
    let chart_button = chart.ns_status_item().unwrap().button(mtm).unwrap();
    status_items::mark_open(&chart_button, true);
    let chart_window = chart_button.window().unwrap();
    let chart_content = chart_window.contentView().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while chart_window.frame().size.height == 0.0 && std::time::Instant::now() < deadline {
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    chart_content.layoutSubtreeIfNeeded();
    status_items::fit_chart_button(&chart_button);
    assert_eq!(
        chart_button.bounds().size.height,
        chart_window.frame().size.height,
        "main button highlight must fill the menu bar height like provider buttons"
    );
    let chart_target = chart_button.subviews().firstObject().unwrap();
    assert_eq!(
        chart_target.frame(),
        chart_button.bounds(),
        "tray-icon's event receiver must fill the expanded main button"
    );
    for y in [
        1.0,
        chart_window.frame().size.height / 2.0,
        chart_window.frame().size.height - 1.0,
    ] {
        let hit = chart_content
            .hitTest(NSPoint::new(chart_button.frame().size.width / 2.0, y))
            .expect("main button edge");
        assert_eq!(
            hit, chart_target,
            "top, middle and bottom must route to the main tray event receiver"
        );
    }
    let actions = Rc::new(RefCell::new(Vec::new()));
    let received = actions.clone();
    let mut items = status_items::ProviderItems::new(mtm, move |action| {
        received.borrow_mut().push(action);
    });
    let chips: Vec<_> = ["anthropic", "openai"]
        .into_iter()
        .map(|id| menu_bar::Chip {
            id: id.into(),
            name: id.into(),
            value: Some("0%".into()),
            stale: false,
            level: None,
            active_account: false,
            mark: None,
        })
        .collect();
    let tips = chips
        .iter()
        .map(status_items::tooltip_line)
        .collect::<Vec<_>>();
    items.sync(&chips, &tips, true, None);
    items.highlight(None);
    for window in app.windows() {
        if let Some(content) = window.contentView() {
            content.layoutSubtreeIfNeeded();
        }
        window.displayIfNeeded();
    }
    objc2_quartz_core::CATransaction::flush();
    let frames = items.frames();
    assert_eq!(frames.len(), 2);
    for (index, (_, frame)) in frames.iter().enumerate() {
        let window = app
            .windows()
            .into_iter()
            .find(|window| {
                let bounds = window.frame();
                frame.origin.x >= bounds.origin.x
                    && frame.origin.x + frame.size.width <= bounds.origin.x + bounds.size.width
                    && frame.origin.y >= bounds.origin.y
                    && frame.origin.y + frame.size.height <= bounds.origin.y + bounds.size.height
            })
            .expect("provider panel");
        assert!(frame.size.height >= NSStatusBar::systemStatusBar().thickness());
        assert_eq!(
            frame.size.height,
            window.frame().size.height,
            "provider {index} must accept clicks across the entire menu bar height"
        );
        let content = window.contentView().expect("provider content");
        for y in [
            1.0,
            window.frame().size.height / 2.0,
            window.frame().size.height - 1.0,
        ] {
            // Switching providers must keep the inactive sibling clickable.
            items.highlight(Some(1 - index));
            let screen_point = || {
                // App activation can recenter the strip between two clicks.
                let current = items.frames()[index].1;
                NSPoint::new(
                    current.origin.x + current.size.width / 2.0,
                    current.origin.y + y,
                )
            };
            // WindowServer receives AppKit/CoreAnimation updates asynchronously.
            // Pump until the observable hit region arrives, with a bounded failure.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(screen_point(), 0, mtm)
                != window.windowNumber()
                && std::time::Instant::now() < deadline
            {
                NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
            }
            assert_eq!(
                NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(screen_point(), 0, mtm),
                window.windowNumber(),
                "WindowServer must route physical clicks to provider {index} at height {y}",
            );
            let screen_point = screen_point();
            let point = NSPoint::new(
                screen_point.x - window.frame().origin.x,
                screen_point.y - window.frame().origin.y,
            );
            let hit = content.hitTest(point).expect("hit at menu bar edge");
            let button = hit
                .downcast_ref::<NSButton>()
                .expect("edge hits provider button");
            assert_eq!(button.tag(), index as isize);
            // SAFETY: ProviderItems owns the live action target. Hit-testing
            // above covers routing; do not make control tracking depend on
            // where the user's physical pointer happens to be during the test.
            unsafe { button.performClick(None) };
            assert_eq!(
                *actions.borrow(),
                vec![status_items::ItemAction::Click {
                    index,
                    right: false
                },],
                "each hit-tested button must dispatch exactly one provider click"
            );
            actions.borrow_mut().clear();
            items.highlight(Some(index));
        }
    }
    items.clear();
    println!("PASS: main button and both providers accept top, middle and bottom clicks");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS AppKit test: skipped on this platform");
}
