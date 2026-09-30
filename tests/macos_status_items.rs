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
#[allow(dead_code)]
#[path = "../src/tray/center_bar.rs"]
mod center_bar;
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
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    if !std::env::args().any(|arg| arg == "--run-native") {
        println!("SKIP: AppKit requires an interactive desktop; opt in with --run-native");
        return;
    }

    let mtm = MainThreadMarker::new().expect("AppKit test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    // Permission state is injected: this guide test never requests access,
    // reads the real TCC state or registers an app in System Settings.
    let granted = Rc::new(Cell::new(false));
    let checks = Rc::new(Cell::new(0));
    let changes = Rc::new(Cell::new(0));
    let notified = changes.clone();
    let permission = granted.clone();
    let observed = checks.clone();
    let guide = menu_space::accessibility_prompt::Guide::new(
        mtm,
        std::path::Path::new("/nonexistent/AI Usage Fixture.app"),
        Box::new(move || {
            observed.set(observed.get() + 1);
            permission.get()
        }),
        Box::new(move || notified.set(notified.get() + 1)),
    );
    guide.present(mtm);
    assert!(
        guide.alert.window().isVisible(),
        "the guide must return without a modal loop"
    );
    let done = guide.alert.buttons().firstObject().unwrap();
    assert!(
        !done.isEnabled(),
        "a configured switch is not proof of access"
    );
    for expected in [true, false] {
        granted.set(expected);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while done.isEnabled() != expected && std::time::Instant::now() < deadline {
            NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
        assert_eq!(
            done.isEnabled(),
            expected,
            "permission changes must update the guide"
        );
    }
    assert_eq!(
        changes.get(),
        2,
        "each permission transition must refresh the bar once"
    );
    // SAFETY: the guide retains the target for its native Cancel action.
    unsafe { guide.alert.buttons().objectAtIndex(1).performClick(None) };
    assert!(
        !guide.alert.window().isVisible(),
        "Cancel must close the modeless guide"
    );
    let stopped = checks.get();
    NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.6));
    assert_eq!(
        checks.get(),
        stopped,
        "closing the guide must stop its timer"
    );
    drop(guide);
    let chart = tray_icon::TrayIconBuilder::new()
        .with_icon(tray_icon::Icon::from_rgba(vec![255; 16 * 16 * 4], 16, 16).unwrap())
        .with_menu_on_left_click(false)
        .build()
        .unwrap();
    let chart_button = chart.ns_status_item().unwrap().button(mtm).unwrap();
    let chart_actions = Rc::new(RefCell::new(Vec::new()));
    let received_chart = chart_actions.clone();
    status_items::install_chart_button(&chart_button, move |action| {
        received_chart.borrow_mut().push(action)
    });
    let real_icon = status_items::template_main_image(18.0);
    chart_button.setImage(Some(&real_icon));
    assert!(chart_button.image().unwrap().isTemplate());
    let native_cell = chart_button.cell().unwrap();
    status_items::mark_open(&chart_button, true);
    let chart_window = chart_button.window().unwrap();
    let chart_content = chart_window.contentView().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while chart_window.frame().size.height == 0.0 && std::time::Instant::now() < deadline {
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    chart_content.layoutSubtreeIfNeeded();
    status_items::fit_chart_button(&chart_button);
    assert!(
        !chart_button.isTransparent(),
        "the real template icon must use the native status-bar renderer"
    );
    assert_eq!(
        chart_button.cell().as_deref(),
        Some(native_cell.as_ref()),
        "resizing must preserve the native status cell and its interaction state"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while chart_window.screen().is_none() && std::time::Instant::now() < deadline {
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    let screen = chart_window
        .screen()
        .expect("status item attached to screen");
    let menu_height = (screen.frame().max().y - screen.visibleFrame().max().y)
        .max(NSStatusBar::systemStatusBar().thickness());
    NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
    chart_content.layoutSubtreeIfNeeded();
    assert!(
        (chart_content.bounds().size.height - menu_height).abs() <= 1.0,
        "native status window must cover the menu bar, excluding its one-point separator"
    );
    assert_eq!(
        chart_button.bounds().size.height,
        chart_content.bounds().size.height,
        "main button must fill the native menu bar after AppKit layout"
    );
    assert_eq!(
        chart_button.bounds().size.height,
        chart_window.frame().size.height,
        "main button highlight must fill the menu bar height like provider buttons"
    );
    let original_target = chart_button
        .subviews()
        .into_iter()
        .find(|view| view.class().name().to_bytes() == b"TaoTrayTarget")
        .unwrap();
    assert!(
        original_target.isHidden(),
        "tray-icon must not receive the same press"
    );
    let chart_target = chart_button
        .subviews()
        .into_iter()
        .find(|view| view.class().name().to_bytes() == b"AiubChartTarget")
        .unwrap();
    let chart_presentation = chart_content
        .subviews()
        .into_iter()
        .find(|view| view.class().name().to_bytes() == b"AiubChartFill")
        .unwrap();
    assert_eq!(
        chart_content.subviews().firstObject().as_deref(),
        Some(chart_presentation.as_ref()),
        "fill must be behind the native glyph"
    );
    assert_eq!(chart_presentation.frame(), chart_button.bounds());
    let image_rect = chart_button
        .cell()
        .unwrap()
        .imageRectForBounds(chart_button.bounds());
    assert!(
        (image_rect.mid().x - chart_presentation.bounds().mid().x).abs() <= 1.0,
        "the main glyph must remain horizontally centered"
    );
    assert_eq!(
        chart_target.frame(),
        chart_button.bounds(),
        "tray-icon's event receiver must fill the expanded main button"
    );
    // tray-icon positions this child with the parent's frame. Reproduce a
    // refresh while AppKit has a four-point native inset.
    chart_target.setFrame(objc2_foundation::NSRect::new(
        NSPoint::new(0.0, 4.0),
        objc2_foundation::NSSize::new(chart_button.bounds().size.width, 22.0),
    ));
    status_items::fit_chart_button(&chart_button);
    assert_eq!(
        chart_target.frame(),
        chart_button.bounds(),
        "a displaced event receiver must recover after refresh"
    );
    let constraint_count = chart_content.constraints().len();
    for side in [16, 18, 20, 18] {
        chart
            .set_icon(Some(
                tray_icon::Icon::from_rgba(vec![255; (side * side * 4) as usize], side, side)
                    .unwrap(),
            ))
            .unwrap();
        chart.set_icon_as_template(true);
        chart.set_title(Some(""));
        chart.set_tooltip(Some("AI Usage test")).unwrap();
        chart_button.setImage(Some(&real_icon));
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        chart_content.layoutSubtreeIfNeeded();
        assert_eq!(
            chart_button.bounds().size.height,
            chart_content.bounds().size.height,
            "changing the icon must not restore AppKit's vertical inset"
        );
        assert_eq!(
            chart_target.frame(),
            chart_button.bounds(),
            "event receiver must follow the full button after an icon update"
        );
        status_items::fit_chart_button(&chart_button);
        status_items::install_chart_button(&chart_button, |_| panic!("receiver installed twice"));
        assert!(
            original_target.isHidden(),
            "refresh must not reactivate the old receiver"
        );
        assert_eq!(chart_button.image().as_deref(), Some(real_icon.as_ref()));
        assert_eq!(
            chart_button.cell().as_deref(),
            Some(native_cell.as_ref()),
            "icon, title and tooltip refreshes must preserve the system cell"
        );
        assert_eq!(
            chart_content.constraints().len(),
            constraint_count,
            "repeated updates must not duplicate layout constraints"
        );
    }
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    use status_items::ChartAction;
    use tray_icon::MouseButton;
    let check_fill = |active: bool| {
        let bitmap = chart_presentation
            .bitmapImageRepForCachingDisplayInRect(chart_presentation.bounds())
            .unwrap();
        chart_presentation
            .cacheDisplayInRect_toBitmapImageRep(chart_presentation.bounds(), &bitmap);
        // The menu bar's own capsule on macOS 27: 24 points tall, centered.
        let scale = bitmap.pixelsHigh() as f64 / chart_presentation.bounds().size.height;
        let mid = chart_presentation.bounds().size.height / 2.0;
        let alpha_at = |y: f64| {
            bitmap
                .colorAtX_y(bitmap.pixelsWide() / 2, (y * scale) as isize)
                .unwrap()
                .alphaComponent()
        };
        for y in [mid - 11.0, mid, mid + 11.0] {
            assert_eq!(alpha_at(y) > 0.05, active, "capsule fill at {y}");
        }
        for y in [mid - 13.5, mid + 13.5] {
            assert!(alpha_at(y) < 0.05, "no fill outside the capsule at {y}");
        }
        assert!(
            !chart_button.isHighlighted(),
            "no independent native pressed state"
        );
    };
    let event = |kind, x, y| {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind, NSPoint::new(x, y), NSEventModifierFlags::empty(), 0.0,
        chart_window.windowNumber(), None, 0, 1, 1.0,
    ).unwrap()
    };
    let center = chart_button.bounds().mid();
    let old_events = tray_icon::TrayIconEvent::receiver();
    while old_events.try_recv().is_ok() {}
    for y in [1.0, center.y, chart_window.frame().size.height - 1.0] {
        let hit = chart_content
            .hitTest(NSPoint::new(center.x, y))
            .expect("main button edge");
        assert_eq!(
            hit, chart_target,
            "all edges must route to the single main receiver"
        );
        for (down, up, mouse_button) in [
            (
                NSEventType::LeftMouseDown,
                NSEventType::LeftMouseUp,
                MouseButton::Left,
            ),
            (
                NSEventType::RightMouseDown,
                NSEventType::RightMouseUp,
                MouseButton::Right,
            ),
        ] {
            chart_actions.borrow_mut().clear();
            status_items::mark_open(&chart_button, false);
            check_fill(false);
            let down_event = event(down, center.x, y);
            let up_event = event(up, center.x, y);
            match mouse_button {
                MouseButton::Left => hit.mouseDown(&down_event),
                MouseButton::Right => hit.rightMouseDown(&down_event),
                MouseButton::Middle => hit.otherMouseDown(&down_event),
            }
            assert_eq!(*chart_actions.borrow(), vec![ChartAction::Pressed]);
            check_fill(true); // held down, before any action or host round trip
            status_items::fit_chart_button(&chart_button);
            status_items::mark_open(&chart_button, false); // data refresh while held
            check_fill(true);
            match mouse_button {
                MouseButton::Left => hit.mouseUp(&up_event),
                MouseButton::Right => hit.rightMouseUp(&up_event),
                MouseButton::Middle => hit.otherMouseUp(&up_event),
            }
            check_fill(true); // no blank frame while the host handles release
            status_items::mark_open(&chart_button, false); // refresh before the host sees Up
            check_fill(true);
            assert_eq!(
                *chart_actions.borrow(),
                vec![ChartAction::Pressed, ChartAction::Released(mouse_button)]
            );
            assert!(
                old_events.try_recv().is_err(),
                "the original receiver must not duplicate clicks"
            );
            status_items::mark_open(&chart_button, true);
            status_items::acknowledge_chart_click(&chart_button);
            check_fill(true);
            status_items::mark_open(&chart_button, false);
            check_fill(false);
        }
    }
    // A release without a matching press must not open anything.
    chart_actions.borrow_mut().clear();
    chart_target.mouseUp(&event(NSEventType::LeftMouseUp, center.x, center.y));
    assert!(chart_actions.borrow().is_empty());
    // A lost up must not prevent the next gesture, including another button.
    chart_target.mouseDown(&event(NSEventType::LeftMouseDown, center.x, center.y));
    chart_target.rightMouseDown(&event(NSEventType::RightMouseDown, center.x, center.y));
    chart_target.mouseUp(&event(NSEventType::LeftMouseUp, center.x, center.y));
    chart_target.rightMouseUp(&event(NSEventType::RightMouseUp, center.x, center.y));
    assert_eq!(
        *chart_actions.borrow(),
        vec![
            ChartAction::Pressed,
            ChartAction::Pressed,
            ChartAction::Released(MouseButton::Right)
        ]
    );
    status_items::acknowledge_chart_click(&chart_button);
    check_fill(false);
    chart_actions.borrow_mut().clear();
    // Leaving the button cancels its pressed fill; re-entering restores it.
    chart_target.mouseDown(&event(NSEventType::LeftMouseDown, center.x, center.y));
    chart_target.mouseDragged(&event(NSEventType::LeftMouseDragged, -5.0, center.y));
    check_fill(false);
    chart_target.mouseDragged(&event(NSEventType::LeftMouseDragged, center.x, center.y));
    check_fill(true);
    chart_target.mouseUp(&event(NSEventType::LeftMouseUp, -5.0, center.y));
    check_fill(false);
    assert_eq!(
        *chart_actions.borrow(),
        vec![ChartAction::Pressed, ChartAction::Cancelled]
    );
    // An interrupted press clears on explicit host cancellation, and its late
    // release cannot toggle the menu. The open fill survives cancelling a press.
    for open in [false, true] {
        status_items::mark_open(&chart_button, open);
        chart_actions.borrow_mut().clear();
        chart_target.mouseDown(&event(NSEventType::LeftMouseDown, center.x, center.y));
        check_fill(true);
        status_items::cancel_chart_press(&chart_button);
        check_fill(open);
        chart_target.mouseUp(&event(NSEventType::LeftMouseUp, center.x, center.y));
        assert_eq!(
            *chart_actions.borrow(),
            vec![ChartAction::Pressed],
            "host cancellation must not echo and erase a subsequent provider press"
        );
    }
    status_items::mark_open(&chart_button, false);
    check_fill(false);
    chart_presentation.removeFromSuperview();
    status_items::fit_chart_button(&chart_button);
    assert_eq!(
        chart_content.subviews().firstObject().as_deref(),
        Some(chart_presentation.as_ref()),
        "detached fill must be reattached before constraints are activated"
    );
    status_items::mark_open(&chart_button, true);
    check_fill(true);
    status_items::mark_open(&chart_button, false);
    assert!(
        chart_button.layer().unwrap().backgroundColor().is_none(),
        "only the sibling fill owns the background"
    );
    // macOS 27: the menu bar draws the press capsule itself. A press drawn
    // here too stacks a second, differently sized highlight on it.
    status_items::use_system_highlight(&chart_button);
    chart_actions.borrow_mut().clear();
    chart_target.mouseDown(&event(NSEventType::LeftMouseDown, center.x, center.y));
    check_fill(false);
    chart_target.mouseUp(&event(NSEventType::LeftMouseUp, center.x, center.y));
    check_fill(false);
    status_items::acknowledge_chart_click(&chart_button);
    // Opened without a session (the keyboard shortcut): the same capsule.
    status_items::mark_open(&chart_button, true);
    check_fill(true);
    status_items::mark_open(&chart_button, false);
    let session_item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    let session = status_items::ExpandedSession::attach(&session_item, |_| {});
    let supported = objc2_foundation::NSObjectProtocol::respondsToSelector(
        &*session_item,
        objc2::sel!(expandedInterfaceSession),
    );
    assert_eq!(
        session.is_some(),
        supported,
        "session only where the OS has it"
    );
    if let Some(session) = &session {
        assert!(!session.active() && !session.open_before(f64::MAX));
        session.sync(false);
    }
    drop(session);
    NSStatusBar::systemStatusBar().removeStatusItem(&session_item);
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
            let capsule = button
                .layer()
                .and_then(|layer| unsafe { layer.sublayers() })
                .and_then(|layers| layers.firstObject())
                .expect("open capsule layer");
            let bounds = button.bounds();
            assert_eq!(
                capsule.frame().size.height,
                24.0_f64.min(bounds.size.height)
            );
            assert_eq!(capsule.frame().size.width, bounds.size.width);
            assert_eq!(capsule.cornerRadius(), capsule.frame().size.height / 2.0);
            assert!(capsule.backgroundColor().is_some(), "open provider capsule");
        }
    }
    // An open provider holds the panel: a wider title moves it only once the
    // provider closes, so its popover stays where it opened.
    let panel = app
        .windows()
        .into_iter()
        .find(|window| {
            window.level() == objc2_app_kit::NSStatusWindowLevel
                && window
                    .contentView()
                    .is_some_and(|view| view.downcast_ref::<objc2_app_kit::NSStackView>().is_some())
        })
        .expect("centered providers' panel");
    items.highlight(Some(0));
    let held = panel.frame();
    let wider: Vec<_> = chips
        .iter()
        .map(|chip| menu_bar::Chip {
            id: chip.id.clone(),
            name: chip.name.clone(),
            value: Some("100% · 100%".into()),
            stale: false,
            level: None,
            active_account: false,
            mark: None,
        })
        .collect();
    let wider_tips = wider
        .iter()
        .map(status_items::tooltip_line)
        .collect::<Vec<_>>();
    items.sync(&wider, &wider_tips, true, None);
    assert_eq!(
        panel.frame(),
        held,
        "an open provider's panel must stay put"
    );
    // Nor does a rebuild, as when a provider comes or goes.
    let more: Vec<_> = ["anthropic", "openai", "zai"]
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
    let more_tips = more
        .iter()
        .map(status_items::tooltip_line)
        .collect::<Vec<_>>();
    items.sync(&more, &more_tips, true, None);
    assert_eq!(
        panel.frame(),
        held,
        "a rebuild must not move an open provider's panel"
    );
    assert!(
        panel.isVisible(),
        "the panel stays on screen through a rebuild"
    );
    items.highlight(None);
    assert!(
        panel.frame().size.width > held.size.width,
        "the panel fits its providers again once none is open"
    );
    items.clear();
    // Providers in their own status items: where the menu bar tracks their
    // sessions, highlighting one must neither fail nor invent an open session.
    items.sync(&chips, &tips, false, None);
    assert_eq!(items.frames().len(), 2);
    for index in 0..2 {
        items.highlight(Some(index));
        assert!(
            !items.session_open_before(index, f64::MAX),
            "no session until the menu bar begins one"
        );
        items.acknowledge(index);
    }
    items.highlight(None);
    items.clear();
    println!("PASS: main button and both providers accept top, middle and bottom clicks");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS AppKit test: skipped on this platform");
}
