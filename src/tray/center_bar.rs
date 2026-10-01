//! The centered providers' panel: a borderless window over the free stretch
//! of the menu bar, measured again as apps come to the front.

use block2::RcBlock;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSColor,
    NSLayoutAttribute, NSPanel, NSRunningApplication, NSScreen, NSStackView, NSStatusBar,
    NSStatusItem, NSStatusWindowLevel, NSUserInterfaceLayoutOrientation,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
    NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidWakeNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSDefaultRunLoopMode, NSNotification, NSNotificationCenter, NSObjectProtocol,
    NSOperationQueue, NSPoint, NSRect, NSRunLoop, NSSize, NSTimer,
};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::menu_space::{self, MenuEnd};

/// The centered row's gap between two accounts, and around a rule.
pub(super) const CENTER_ACCOUNT_GAP: f64 = 20.0;
const CENTER_RULE_GAP: f64 = 28.0;
/// Room a centered button keeps on each side of its content, where the
/// highlight of an open item shows; taken back out of the row's spacing.
pub(super) const CENTER_ROOM: f64 = 6.0;
/// Right after an app comes to the front, the menu bar can still be laying
/// out its menus, or the app not answering Accessibility yet. They are
/// measured again this often until the last one is laid out, at most this
/// many times.
const REMEASURE_INTERVAL: f64 = 0.2;
const REMEASURE_TRIES: u32 = 5;
/// Displays and status items can settle after the menus are already readable.
/// Keep placing for three seconds after a display transition, even without AX.
const DISPLAY_REMEASURE_INTERVAL: f64 = 0.5;
const DISPLAY_REMEASURE_TRIES: u32 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MeasureReason {
    Initial,
    Application,
    Display,
}

struct Remeasure {
    timer: Retained<NSTimer>,
    /// An application notification may replace the menu reading, but must not
    /// end (or extend) the display's bounded settling period.
    display_until: Option<Instant>,
}

/// A borderless panel over the middle of the menu bar holding the centered
/// providers. AppKit only places status items at the right, so centering
/// needs a window of its own.
pub(super) struct CenterBar {
    pub(super) panel: Retained<NSPanel>,
    pub(super) stack: Retained<NSStackView>,
    /// Where the menus shown end, as last read, or `None` when unknown; kept
    /// while this app is in front, so opening the popover does not move the
    /// providers.
    menu_end: Rc<Cell<Option<f64>>>,
    /// Resolve the chart's current window each time: AppKit can move or replace
    /// it during a display transition, making a saved coordinate obsolete.
    pub(super) chart_item: Rc<RefCell<Option<Retained<NSStatusItem>>>>,
    /// Measures the menus again while the menu bar lays them out or, after
    /// an app comes to the front, while they cannot be read.
    remeasure: Rc<Cell<Option<Remeasure>>>,
    /// One of the providers is open: its popover stays where it opened, so
    /// the panel moves only once it closes, unless it is off screen.
    held: Rc<Cell<bool>>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl CenterBar {
    pub(super) fn new(mtm: MainThreadMarker) -> Self {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::ZERO,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: the panel is owned here and never closed through AppKit.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSStatusWindowLevel);
        // Not FullScreenAuxiliary: a fullscreen app hides the menu bar, so
        // the providers stay off its space too. Transient, not Stationary:
        // Mission Control hides the menu bar, and the providers with it.
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setOpaque(false);
        panel.setHasShadow(false);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        let stack = NSStackView::new(mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setAlignment(NSLayoutAttribute::CenterY);
        stack.setSpacing(CENTER_RULE_GAP - CENTER_ROOM);
        panel.setContentView(Some(&stack));
        eprintln!(
            "centered providers: accessibility {}",
            if menu_space::trusted() {
                "granted"
            } else {
                "not granted"
            }
        );
        // Measured when the panel is first placed, with the providers in it.
        let menu_end = Rc::new(Cell::new(None));
        let chart_item = Rc::new(RefCell::new(None));
        let remeasure: Rc<Cell<Option<Remeasure>>> = Rc::default();
        let held = Rc::new(Cell::new(false));
        let block = {
            let (panel, stack) = (panel.clone(), stack.clone());
            let (menu_end, chart_item) = (menu_end.clone(), chart_item.clone());
            let (remeasure, held) = (remeasure.clone(), held.clone());
            RcBlock::new(move |notification: NonNull<NSNotification>| {
                // SAFETY: AppKit passes a live notification for the call.
                let notification = unsafe { notification.as_ref() };
                if activates_this_app(notification) {
                    if movable(&panel, &held) {
                        place(&panel, &stack, menu_end.get(), &chart_item);
                    }
                } else if !quits_another_app(notification) {
                    measure(
                        &panel,
                        &stack,
                        &menu_end,
                        &chart_item,
                        &held,
                        &remeasure,
                        if display_changed(notification) {
                            MeasureReason::Display
                        } else {
                            MeasureReason::Application
                        },
                    );
                }
            })
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let queue = NSOperationQueue::mainQueue();
        // SAFETY: the queue confines every callback to the main thread, where
        // the block's AppKit objects and Rc state are owned.
        let observers = unsafe {
            vec![
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                    Some(&queue),
                    &block,
                ),
                workspace.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidActivateApplicationNotification),
                    None,
                    Some(&queue),
                    &block,
                ),
                // The app that owns the menu bar can quit while this one is in
                // front, and then no other app comes to the front.
                workspace.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidTerminateApplicationNotification),
                    None,
                    Some(&queue),
                    &block,
                ),
                workspace.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceScreensDidWakeNotification),
                    None,
                    Some(&queue),
                    &block,
                ),
                workspace.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidWakeNotification),
                    None,
                    Some(&queue),
                    &block,
                ),
            ]
        };
        Self {
            panel,
            stack,
            menu_end,
            chart_item,
            remeasure,
            held,
            observers,
        }
    }

    pub(super) fn place(&self) {
        // Not measured yet, as when centering has just been turned on, or
        // not readable when last measured, as without the permission.
        if self.menu_end.get().is_none() {
            measure(
                &self.panel,
                &self.stack,
                &self.menu_end,
                &self.chart_item,
                &self.held,
                &self.remeasure,
                MeasureReason::Initial,
            );
        } else if movable(&self.panel, &self.held) {
            place(
                &self.panel,
                &self.stack,
                self.menu_end.get(),
                &self.chart_item,
            );
        }
    }

    /// Hold the panel while one of its providers is open, and place it once
    /// none is, in case the menus were measured again meanwhile.
    pub(super) fn hold(&self, open: bool) {
        if self.held.replace(open) && !open {
            self.place();
        }
    }
}

impl Drop for CenterBar {
    fn drop(&mut self) {
        // A later measurement would order the panel front again.
        if let Some(timer) = self.remeasure.take() {
            timer.timer.invalidate();
        }
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        for observer in &self.observers {
            // SAFETY: each observer came from one of these two centers, and
            // removing it from the other is a no-op.
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref());
                workspace.removeObserver(observer.as_ref());
            }
        }
        self.panel.orderOut(None);
    }
}

/// Whether the panel may move: not while one of its providers is open, unless
/// it is off screen and has to be shown.
fn movable(panel: &NSPanel, held: &Cell<bool>) -> bool {
    if !held.get() || !panel.isVisible() {
        return true;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(screen) = NSScreen::screens(mtm).firstObject() else {
        return false;
    };
    let bounds = panel.frame();
    let primary = screen.frame();
    // A visible panel on the previous display is no longer a valid popover
    // anchor. Hold it only while it still fits in the primary menu bar.
    bounds.origin.x < primary.origin.x
        || bounds.max().x > primary.max().x
        || (bounds.max().y - primary.max().y).abs() > 1.0
}

/// Size the panel to its buttons and center it in the free stretch of the
/// primary display's menu bar, the one the status items live in.
fn place(
    panel: &NSPanel,
    stack: &NSStackView,
    menu_end: Option<f64>,
    chart_item: &RefCell<Option<Retained<NSStatusItem>>>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(screen) = NSScreen::screens(mtm).firstObject() else {
        return;
    };
    let frame = screen.frame();
    let visible = screen.visibleFrame();
    let top = frame.origin.y + frame.size.height;
    let bar = (top - (visible.origin.y + visible.size.height))
        .max(NSStatusBar::systemStatusBar().thickness());
    let width = stack.fittingSize().width;
    let chart = chart_item.borrow().clone();
    let chart_left = chart
        .as_deref()
        .and_then(|item| item.button(mtm))
        .and_then(|button| button.window())
        .map(|window| window.frame().origin.x);
    let items = menu_space::status_items_start(chart_left, bar);
    let x = menu_space::centered_x(width, (frame.origin.x, frame.size.width), menu_end, items);
    let target = NSRect::new(NSPoint::new(x, top - bar), NSSize::new(width, bar));
    if panel.frame() != target {
        panel.setFrame_display(target, true);
    }
    if !panel.isVisible() {
        panel.orderFrontRegardless();
    }
}

/// A display transition needs a settling round even with unchanged menus.
fn display_changed(notification: &NSNotification) -> bool {
    // SAFETY: immutable AppKit notification names.
    unsafe {
        [
            NSApplicationDidChangeScreenParametersNotification,
            NSWorkspaceScreensDidWakeNotification,
            NSWorkspaceDidWakeNotification,
        ]
        .iter()
        .any(|name| notification.name().isEqualToString(name))
    }
}

/// The app `notification` reports on.
fn app_of(notification: &NSNotification) -> Option<Retained<NSRunningApplication>> {
    // SAFETY: an immutable AppKit constant.
    let key: &AnyObject = unsafe { NSWorkspaceApplicationKey }.as_ref();
    notification
        .userInfo()
        .and_then(|info| info.objectForKey(key))
        .and_then(|app| app.downcast::<NSRunningApplication>().ok())
}

/// Whether `notification` reports this app coming to the front. It shows no
/// menus, so the providers stay where they are while its popover opens.
fn activates_this_app(notification: &NSNotification) -> bool {
    app_of(notification).is_some_and(|app| app.processIdentifier() == std::process::id() as i32)
}

/// Whether `notification` reports an app quitting that does not own the menu
/// bar: its menus were not the ones shown. The one that does is still named
/// its owner when it quits, until another app comes to the front.
fn quits_another_app(notification: &NSNotification) -> bool {
    // SAFETY: an immutable AppKit constant.
    let quit = unsafe { NSWorkspaceDidTerminateApplicationNotification };
    if !notification.name().isEqualToString(quit) {
        return false;
    }
    match (
        app_of(notification),
        NSWorkspace::sharedWorkspace().menuBarOwningApplication(),
    ) {
        (Some(app), Some(owner)) => app.processIdentifier() != owner.processIdentifier(),
        _ => false,
    }
}

/// Where the menus end over one round of measurements, until the menu bar
/// has laid them out or the tries run out.
#[derive(Clone, Copy)]
struct Reading {
    /// Where the panel is placed by.
    x: Option<f64>,
    /// Whether `x` was read in this round, from the menus shown now, not
    /// kept from before.
    read: bool,
    /// Stop querying menus once this round has a complete answer.
    pending: bool,
}

impl Reading {
    /// A round that starts from the end kept until now.
    fn from_kept(x: Option<f64>) -> Self {
        Self {
            x,
            read: false,
            pending: true,
        }
    }

    /// Take in a measurement and tell whether to measure again. Menus
    /// Accessibility cannot read may be another app's by now, unless this
    /// round read them; menus still being laid out keep the end until theirs
    /// is read.
    fn take(&mut self, measured: Option<MenuEnd>) -> bool {
        match measured {
            None if !self.read => self.x = None,
            Some(MenuEnd { x: Some(x), .. }) => {
                self.x = Some(x);
                self.read = true;
            }
            _ => {}
        }
        self.pending = measured.is_none_or(|end| !end.laid_out);
        self.pending
    }

    /// Take in a later try, the `last` one or not, and tell whether to
    /// measure again. Once none is left, an end not read in this round is
    /// another app's.
    fn retake(&mut self, measured: Option<MenuEnd>, last: bool) -> bool {
        self.pending = self.take(measured) && !last;
        if !self.pending && !self.read {
            self.x = None;
        }
        self.pending
    }
}

/// Measure where the menus end and place the panel, measuring again while
/// the menu bar lays them out or, after activation and with the permission,
/// while Accessibility cannot read them. Right after an app comes to the
/// front, it may not answer yet; later on, a hung app would cost each try
/// the timeout.
fn measure(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_item: &Rc<RefCell<Option<Retained<NSStatusItem>>>>,
    held: &Rc<Cell<bool>>,
    remeasure: &Cell<Option<Remeasure>>,
    reason: MeasureReason,
) {
    let previous_display = remeasure.take().and_then(|round| {
        round.timer.invalidate();
        round.display_until
    });
    let now = Instant::now();
    let display_until = if reason == MeasureReason::Display {
        Some(
            now + Duration::from_secs_f64(
                DISPLAY_REMEASURE_INTERVAL * f64::from(DISPLAY_REMEASURE_TRIES),
            ),
        )
    } else {
        previous_display.filter(|until| *until > now)
    };
    let measured = menu_space::app_menu_end();
    let mut reading = Reading::from_kept(menu_end.get());
    let again = reading.take(measured);
    if measured.is_none() && !menu_space::trusted() {
        // Display recovery still runs without AX, but cannot gain anything
        // from retrying the unavailable menu reading.
        reading.pending = false;
    }
    menu_end.set(reading.x);
    if movable(panel, held) {
        place(panel, stack, menu_end.get(), chart_item);
    }
    if display_until.is_some()
        || (again
            && (measured.is_some()
                || (reason == MeasureReason::Application && menu_space::trusted())))
    {
        let timer = remeasure_menus(
            panel,
            stack,
            menu_end,
            chart_item,
            held,
            reading,
            display_until,
        );
        remeasure.set(Some(timer));
    }
}

/// Measure the menus again until the last one is laid out or the tries run
/// out, and place the panel whenever their end moves. Display rounds keep
/// placing after that until status items and display geometry have settled.
/// A last menu that stays hidden leaves the end of those drawn; an app that
/// does not answer costs each try one Accessibility timeout.
fn remeasure_menus(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_item: &Rc<RefCell<Option<Retained<NSStatusItem>>>>,
    held: &Rc<Cell<bool>>,
    reading: Reading,
    display_until: Option<Instant>,
) -> Remeasure {
    let (panel, stack) = (panel.clone(), stack.clone());
    let (menu_end, chart_item, held) = (menu_end.clone(), chart_item.clone(), held.clone());
    let reading = Cell::new(reading);
    let menu_tries = Cell::new(0);
    let display_tries = Cell::new(0);
    let display = display_until.is_some();
    let display_pending = Cell::new(display);
    let next_display =
        Cell::new(Instant::now() + Duration::from_secs_f64(DISPLAY_REMEASURE_INTERVAL));
    // A late activation still gets the normal menu cadence and retry budget.
    // Display placement keeps its own cadence and the original deadline.
    let interval = if reading.get().pending || !display {
        REMEASURE_INTERVAL
    } else {
        DISPLAY_REMEASURE_INTERVAL
    };
    let block = RcBlock::new(move |timer: NonNull<NSTimer>| {
        let tick = Instant::now();
        let display_last = display_until.is_some_and(|until| tick >= until);
        let place_display = display_pending.get() && (tick >= next_display.get() || display_last);
        let mut now = reading.get();
        if now.pending {
            menu_tries.set(menu_tries.get() + 1);
            now.retake(
                menu_space::app_menu_end(),
                menu_tries.get() == REMEASURE_TRIES,
            );
        } else if place_display {
            // A complete menu boundary can still move while the display settles.
            // Read at the display cadence without reopening fast menu retries.
            now.retake(menu_space::app_menu_end(), true);
        }
        reading.set(now);
        let changed = menu_end.get() != now.x;
        if changed {
            menu_end.set(now.x);
        }
        if (changed || place_display) && movable(&panel, &held) {
            place(&panel, &stack, now.x, &chart_item);
        }
        if place_display {
            display_tries.set(display_tries.get() + 1);
            next_display
                .set(next_display.get() + Duration::from_secs_f64(DISPLAY_REMEASURE_INTERVAL));
            if display_last || display_tries.get() == DISPLAY_REMEASURE_TRIES {
                display_pending.set(false);
            }
        }
        if !now.pending && !display_pending.get() {
            // SAFETY: the run loop passes the timer that fired, alive for the call.
            unsafe { timer.as_ref() }.invalidate();
        }
    });
    // SAFETY: the timer is added to the main run loop only, so the block never
    // leaves this thread. In the default mode it waits while a menu is open,
    // so the panel does not move under it. NSDefaultRunLoopMode is an
    // immutable constant.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(interval, true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSDefaultRunLoopMode);
        Remeasure {
            timer,
            display_until,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(x: Option<f64>, laid_out: bool) -> Option<MenuEnd> {
        Some(MenuEnd { x, laid_out })
    }

    #[test]
    fn menus_accessibility_cannot_read_have_no_end() {
        // The previous app's end, while this one's menus do not answer.
        let mut reading = Reading::from_kept(Some(372.0));
        assert!(reading.take(None), "measured again");
        assert_eq!(reading.x, None);
        // Unless this round read them already.
        let mut reading = Reading::from_kept(None);
        assert!(reading.take(end(Some(44.0), false)));
        assert!(reading.retake(None, false));
        assert_eq!(reading.x, Some(44.0));
    }

    #[test]
    fn a_reading_keeps_the_end_until_the_menus_are_laid_out() {
        let mut reading = Reading::from_kept(Some(372.0));
        // No menu drawn yet: the previous app's end stays for now...
        assert!(reading.take(end(None, false)));
        assert_eq!(reading.x, Some(372.0));
        // ...but not once the tries run out.
        let mut out = reading;
        assert!(!out.retake(end(None, false), true));
        assert_eq!(out.x, None);
        // The first ones drawn are this app's, even with no try left.
        assert!(reading.retake(end(Some(44.0), false), false));
        let mut out = reading;
        assert!(!out.retake(end(None, false), true));
        assert_eq!(out.x, Some(44.0));
        // The last one ends the round.
        assert!(!reading.retake(end(Some(372.0), true), false));
        assert_eq!(reading.x, Some(372.0));
    }
}
