//! The centered providers' panel: a borderless window over the free stretch
//! of the menu bar, measured again as apps come to the front.

use block2::RcBlock;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSColor,
    NSLayoutAttribute, NSPanel, NSRunningApplication, NSScreen, NSStackView, NSStatusBar,
    NSStatusWindowLevel, NSUserInterfaceLayoutOrientation, NSWindowCollectionBehavior,
    NSWindowStyleMask, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSDefaultRunLoopMode, NSNotification, NSNotificationCenter, NSObjectProtocol,
    NSPoint, NSRect, NSRunLoop, NSSize, NSTimer,
};
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

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
    /// The left edge of this app's chart item.
    pub(super) chart_left: Rc<Cell<Option<f64>>>,
    /// Measures the menus again while the menu bar lays them out or, after
    /// an app comes to the front, while they cannot be read.
    remeasure: Rc<Cell<Option<Retained<NSTimer>>>>,
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
        let chart_left = Rc::new(Cell::new(None));
        let remeasure: Rc<Cell<Option<Retained<NSTimer>>>> = Rc::default();
        let held = Rc::new(Cell::new(false));
        let block = {
            let (panel, stack) = (panel.clone(), stack.clone());
            let (menu_end, chart_left) = (menu_end.clone(), chart_left.clone());
            let (remeasure, held) = (remeasure.clone(), held.clone());
            RcBlock::new(move |notification: NonNull<NSNotification>| {
                // SAFETY: AppKit passes a live notification for the call.
                if !activates_this_app(unsafe { notification.as_ref() }) {
                    measure(
                        &panel,
                        &stack,
                        &menu_end,
                        &chart_left,
                        &held,
                        &remeasure,
                        true,
                    );
                } else if movable(&panel, &held) {
                    place(&panel, &stack, menu_end.get(), chart_left.get());
                }
            })
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: the block only touches main-thread objects and AppKit posts
        // both notifications on the main thread.
        let observers = unsafe {
            vec![
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                    None,
                    &block,
                ),
                workspace.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidActivateApplicationNotification),
                    None,
                    None,
                    &block,
                ),
            ]
        };
        Self {
            panel,
            stack,
            menu_end,
            chart_left,
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
                &self.chart_left,
                &self.held,
                &self.remeasure,
                false,
            );
        } else if movable(&self.panel, &self.held) {
            place(
                &self.panel,
                &self.stack,
                self.menu_end.get(),
                self.chart_left.get(),
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
            timer.invalidate();
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
/// it is off screen, as right after its providers are rebuilt.
fn movable(panel: &NSPanel, held: &Cell<bool>) -> bool {
    !held.get() || !panel.isVisible()
}

/// Size the panel to its buttons and center it in the free stretch of the
/// primary display's menu bar, the one the status items live in.
fn place(panel: &NSPanel, stack: &NSStackView, menu_end: Option<f64>, chart_left: Option<f64>) {
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
    let items = menu_space::status_items_start(chart_left, bar);
    let x = menu_space::centered_x(width, (frame.origin.x, frame.size.width), menu_end, items);
    panel.setFrame_display(
        NSRect::new(NSPoint::new(x, top - bar), NSSize::new(width, bar)),
        true,
    );
    panel.orderFrontRegardless();
}

/// Whether `notification` reports this app coming to the front. It shows no
/// menus, so the providers stay where they are while its popover opens.
fn activates_this_app(notification: &NSNotification) -> bool {
    // SAFETY: an immutable AppKit constant.
    let key: &AnyObject = unsafe { NSWorkspaceApplicationKey }.as_ref();
    notification
        .userInfo()
        .and_then(|info| info.objectForKey(key))
        .and_then(|app| app.downcast::<NSRunningApplication>().ok())
        .is_some_and(|app| app.processIdentifier() == std::process::id() as i32)
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
}

impl Reading {
    /// A round that starts from the end kept until now.
    fn from_kept(x: Option<f64>) -> Self {
        Self { x, read: false }
    }

    /// Take in a measurement and tell whether to measure again. Menus
    /// Accessibility cannot read may be another app's by now, unless this
    /// round read them; menus still being laid out keep the end until theirs
    /// is read.
    fn take(&mut self, measured: Option<MenuEnd>) -> bool {
        match measured {
            None if !self.read => self.x = None,
            Some(MenuEnd { x: Some(x), .. }) => {
                *self = Self {
                    x: Some(x),
                    read: true,
                }
            }
            _ => {}
        }
        measured.is_none_or(|end| !end.laid_out)
    }

    /// Take in a later try, the `last` one or not, and tell whether to
    /// measure again. Once none is left, an end not read in this round is
    /// another app's.
    fn retake(&mut self, measured: Option<MenuEnd>, last: bool) -> bool {
        let again = self.take(measured) && !last;
        if !again && !self.read {
            self.x = None;
        }
        again
    }
}

/// Measure where the menus end and place the panel, measuring again while
/// the menu bar lays them out or, when `unreadable` and with the permission,
/// while Accessibility cannot read them. Right after an app comes to the
/// front, it may not answer yet; later on, a hung app would cost each try
/// the timeout.
fn measure(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_left: &Rc<Cell<Option<f64>>>,
    held: &Rc<Cell<bool>>,
    remeasure: &Cell<Option<Retained<NSTimer>>>,
    unreadable: bool,
) {
    if let Some(timer) = remeasure.take() {
        timer.invalidate();
    }
    let measured = menu_space::app_menu_end();
    let mut reading = Reading::from_kept(menu_end.get());
    let again = reading.take(measured);
    menu_end.set(reading.x);
    if movable(panel, held) {
        place(panel, stack, menu_end.get(), chart_left.get());
    }
    if again && (measured.is_some() || (unreadable && menu_space::trusted())) {
        let timer = remeasure_menus(panel, stack, menu_end, chart_left, held, reading);
        remeasure.set(Some(timer));
    }
}

/// Measure the menus again until the last one is laid out or the tries run
/// out, and place the panel whenever their end moves. A last menu that
/// stays hidden leaves the end of those drawn; an app that does not answer
/// costs each try one Accessibility timeout.
fn remeasure_menus(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_left: &Rc<Cell<Option<f64>>>,
    held: &Rc<Cell<bool>>,
    reading: Reading,
) -> Retained<NSTimer> {
    let (panel, stack) = (panel.clone(), stack.clone());
    let (menu_end, chart_left, held) = (menu_end.clone(), chart_left.clone(), held.clone());
    let reading = Cell::new(reading);
    let tries = Cell::new(0);
    let block = RcBlock::new(move |timer: NonNull<NSTimer>| {
        tries.set(tries.get() + 1);
        let mut now = reading.get();
        let again = now.retake(menu_space::app_menu_end(), tries.get() == REMEASURE_TRIES);
        reading.set(now);
        if menu_end.get() != now.x {
            menu_end.set(now.x);
            if movable(&panel, &held) {
                place(&panel, &stack, now.x, chart_left.get());
            }
        }
        if !again {
            // SAFETY: the run loop passes the timer that fired, alive for the call.
            unsafe { timer.as_ref() }.invalidate();
        }
    });
    // SAFETY: the timer is added to the main run loop only, so the block never
    // leaves this thread. In the default mode it waits while a menu is open,
    // so the panel does not move under it. NSDefaultRunLoopMode is an
    // immutable constant.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(REMEASURE_INTERVAL, true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSDefaultRunLoopMode);
        timer
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
