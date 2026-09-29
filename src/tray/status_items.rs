//! One macOS status item per provider, left of the tray's chart glyph.
//!
//! tray-icon owns a single `NSStatusItem`, so the providers get their own,
//! created and removed here as the menu bar's chips change. Each one draws its
//! provider's mark and value and reports clicks through one target object.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Sel};
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadOnly, Message, define_class, msg_send, sel,
};
use std::ptr::NonNull;

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSApplicationDidChangeScreenParametersNotification,
    NSAttributedStringAttachmentConveniences, NSAttributedStringNSStringDrawing,
    NSBackingStoreType, NSBaselineOffsetAttributeName, NSBezierPath, NSButton, NSColor,
    NSCompositingOperation, NSControl, NSControlStateValueOn, NSEvent, NSEventMask,
    NSEventModifierFlags, NSEventType, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSImage, NSLayoutAttribute, NSLineCapStyle, NSMenu, NSMenuItem, NSPanel,
    NSRectFillUsingOperation, NSResponder, NSRunningApplication, NSScreen, NSStackView,
    NSStatusBar, NSStatusItem, NSStatusWindowLevel, NSTextAttachment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSVariableStatusItemLength, NSView,
    NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceApplicationKey, NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSData, NSDefaultRunLoopMode, NSDictionary,
    NSMutableAttributedString, NSNotification, NSNotificationCenter, NSNumber, NSObject,
    NSObjectProtocol, NSPoint, NSProcessInfo, NSRect, NSRunLoop, NSSize, NSString, NSTimer,
};
use objc2_quartz_core::{CAAutoresizingMask, CALayer};

use super::menu_bar::{Chip, Level};
use super::menu_space;
use std::cell::Cell;
use std::rc::Rc;

/// Side of a provider mark in the menu bar, in points.
const MARK_SIDE: f64 = 15.0;

/// Room on each side of a provider's content. Items get a fixed width, so
/// the gap between two accounts is the status bar's spacing plus twice this,
/// and the highlight stays even on both sides.
const ITEM_ROOM: f64 = 1.25;
/// Room on each side of a rule, which draws no highlight: providers sit
/// further from a rule than from another account of their own.
const RULE_ROOM: f64 = 12.75;
/// The centered row's gap between two accounts, and around a rule.
const CENTER_ACCOUNT_GAP: f64 = 20.0;
const CENTER_RULE_GAP: f64 = 28.0;
/// Room a centered button keeps on each side of its content, where the
/// highlight of an open item shows; taken back out of the row's spacing.
const CENTER_ROOM: f64 = 6.0;
/// Right after an app comes to the front, the menu bar can still be laying
/// out its menus. They are measured again this often until the last one is,
/// at most this many times.
const REMEASURE_INTERVAL: f64 = 0.2;
const REMEASURE_TRIES: u32 = 5;

/// What a provider item or its menu reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAction {
    /// A click on the item at `index`; `right` for a right or control click.
    Click { index: usize, right: bool },
    /// A pick in a provider's menu: the item's `tag`.
    Menu { tag: isize },
    /// The menu bar began the expanded session of the item at `index`: its
    /// left click, which the system tracks itself on macOS 27.
    Expanded { index: usize },
    /// The menu bar ended that session: a long press released, a drag out
    /// or a cancel.
    Collapsed { index: usize },
}

type Callback = Box<dyn Fn(ItemAction)>;

/// Instance state of [`ItemTarget`]: where its actions go.
pub struct TargetIvars {
    callback: Callback,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class does not
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubStatusItemTarget"]
    #[ivars = TargetIvars]
    pub struct ItemTarget;

    impl ItemTarget {
        #[unsafe(method(itemClicked:))]
        fn item_clicked(&self, sender: &AnyObject) {
            // SAFETY: the sender is the NSStatusBarButton this target was set on.
            let tag: isize = unsafe { msg_send![sender, tag] };
            let right = MainThreadMarker::new()
                .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent())
                .is_some_and(|event| {
                    matches!(event.r#type(), NSEventType::RightMouseUp | NSEventType::RightMouseDown)
                        || event.modifierFlags().contains(NSEventModifierFlags::Control)
                });
            if let Ok(index) = usize::try_from(tag) {
                (self.ivars().callback)(ItemAction::Click { index, right });
            }
        }

        #[unsafe(method(menuPicked:))]
        fn menu_picked(&self, sender: &AnyObject) {
            // SAFETY: the sender is an NSMenuItem this target was set on.
            let tag: isize = unsafe { msg_send![sender, tag] };
            (self.ivars().callback)(ItemAction::Menu { tag });
        }
    }
);

impl ItemTarget {
    fn new(mtm: MainThreadMarker, callback: Callback) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { callback });
        // SAFETY: NSObject's init takes no arguments and returns the object.
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    // SAFETY: NSButton has no subclassing requirements and this class does not
    // implement Drop.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubCenterButton"]
    struct CenterButton;

    impl CenterButton {
        /// A right click reports like a status item's does.
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            // SAFETY: the action and target are the ones `sync` set.
            unsafe { self.sendAction_to(self.action(), self.target().as_deref()) };
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            // SAFETY: NSButton implements intrinsicContentSize.
            let size: NSSize = unsafe { msg_send![super(self), intrinsicContentSize] };
            NSSize::new(size.width + 2.0 * CENTER_ROOM, size.height)
        }
    }
);

/// A borderless panel over the middle of the menu bar holding the centered
/// providers. AppKit only places status items at the right, so centering
/// needs a window of its own.
struct CenterBar {
    panel: Retained<NSPanel>,
    stack: Retained<NSStackView>,
    /// Where the last other app's menus ended; kept while this app is in
    /// front, so opening the popover does not move the providers.
    menu_end: Rc<Cell<Option<f64>>>,
    /// The left edge of this app's chart item.
    chart_left: Rc<Cell<Option<f64>>>,
    /// Measures the menus again while the last one is not laid out.
    remeasure: Rc<Cell<Option<Retained<NSTimer>>>>,
    /// One of the providers is open: its popover stays where it opened, so
    /// a new measurement moves the panel only once it closes.
    held: Rc<Cell<bool>>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl CenterBar {
    fn new(mtm: MainThreadMarker) -> Self {
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
                    measure(&panel, &stack, &menu_end, &chart_left, &held, &remeasure);
                } else if !held.get() {
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

    fn place(&self) {
        // Not measured yet, as when centering or the permission has just
        // been turned on.
        if self.menu_end.get().is_none() {
            measure(
                &self.panel,
                &self.stack,
                &self.menu_end,
                &self.chart_left,
                &self.held,
                &self.remeasure,
            );
        } else {
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
    fn hold(&self, open: bool) {
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

/// Measure where the menus end and place the panel, measuring again while
/// the last menu is not laid out.
fn measure(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_left: &Rc<Cell<Option<f64>>>,
    held: &Rc<Cell<bool>>,
    remeasure: &Cell<Option<Retained<NSTimer>>>,
) {
    if let Some(timer) = remeasure.take() {
        timer.invalidate();
    }
    let measured = menu_space::app_menu_end();
    if let Some(x) = measured.and_then(|end| end.x) {
        menu_end.set(Some(x));
    }
    if !held.get() {
        place(panel, stack, menu_end.get(), chart_left.get());
    }
    if measured.is_some_and(|end| !end.laid_out) {
        let timer = remeasure_menus(panel, stack, menu_end, chart_left, held);
        remeasure.set(Some(timer));
    }
}

/// Measure the menus again until the last one is laid out or Accessibility
/// stops answering, and place the panel whenever their end moves. A last
/// menu that stays hidden leaves the end of those drawn.
fn remeasure_menus(
    panel: &Retained<NSPanel>,
    stack: &Retained<NSStackView>,
    menu_end: &Rc<Cell<Option<f64>>>,
    chart_left: &Rc<Cell<Option<f64>>>,
    held: &Rc<Cell<bool>>,
) -> Retained<NSTimer> {
    let (panel, stack) = (panel.clone(), stack.clone());
    let (menu_end, chart_left, held) = (menu_end.clone(), chart_left.clone(), held.clone());
    let tries = Cell::new(0);
    let block = RcBlock::new(move |timer: NonNull<NSTimer>| {
        tries.set(tries.get() + 1);
        let measured = menu_space::app_menu_end();
        if let Some(x) = measured.and_then(|end| end.x)
            && menu_end.get() != Some(x)
        {
            menu_end.set(Some(x));
            if !held.get() {
                place(&panel, &stack, Some(x), chart_left.get());
            }
        }
        let pending = measured.is_some_and(|end| !end.laid_out);
        if !pending || tries.get() == REMEASURE_TRIES {
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

/// Where a provider is drawn: its own status item, or a button in the center.
enum Slot {
    Status(Retained<NSStatusItem>, Option<ExpandedSession>),
    Center(Retained<NSButton>),
}

impl Slot {
    fn button(&self, mtm: MainThreadMarker) -> Option<Retained<NSButton>> {
        match self {
            Slot::Status(item, _) => item.button(mtm).map(Retained::into_super),
            Slot::Center(button) => Some(button.clone()),
        }
    }
}

/// One line of a provider's menu.
pub enum MenuLine {
    /// A disabled heading, such as the provider's name.
    Heading(String),
    /// A pickable line reported back with `tag`, ticked when `checked`.
    Pick {
        title: String,
        tag: isize,
        checked: bool,
    },
    Separator,
}

/// The provider items currently in the menu bar, left to right.
pub struct ProviderItems {
    items: Vec<(String, Slot)>,
    /// The rules between providers and before the chart glyph: their own
    /// items, so a provider's click area and highlight end at its content.
    rules: Vec<Rule>,
    center: Option<CenterBar>,
    target: Retained<ItemTarget>,
}

impl ProviderItems {
    pub fn new(mtm: MainThreadMarker, callback: impl Fn(ItemAction) + 'static) -> Self {
        Self {
            items: Vec::new(),
            rules: Vec::new(),
            center: None,
            target: ItemTarget::new(mtm, Box::new(callback)),
        }
    }

    /// Show `chips`, one item each, with `tooltips` alongside, at the right
    /// of the menu bar or in its middle. The items are rebuilt only when the
    /// providers, their order or the placement change; otherwise each one's
    /// title is redrawn in place.
    ///
    /// `chart_left` is where the chart item starts, the right edge centered
    /// providers keep clear of.
    pub fn sync(
        &mut self,
        chips: &[Chip],
        tooltips: &[String],
        centered: bool,
        chart_left: Option<f64>,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let same = self.items.len() == chips.len()
            && self.center.is_some() == centered
            && self
                .items
                .iter()
                .zip(chips)
                .all(|((id, _), chip)| *id == chip.id);
        if !same && centered {
            self.clear();
            let center = self.center.get_or_insert_with(|| CenterBar::new(mtm));
            for (index, chip) in chips.iter().enumerate() {
                // SAFETY: NSButton's init takes no arguments and returns the object.
                let button: Retained<CenterButton> =
                    unsafe { msg_send![CenterButton::alloc(mtm), init] };
                let button: Retained<NSButton> = Retained::into_super(button);
                button.setBordered(false);
                button.setTag(index as isize);
                // SAFETY: the target outlives the buttons (both live in
                // `self`) and implements `itemClicked:`.
                unsafe {
                    button.setTarget(Some(&self.target));
                    button.setAction(Some(sel!(itemClicked:)));
                }
                center.stack.addArrangedSubview(&button);
                // Keep the content centered but make the button's hit region
                // cover the full menu bar, including above and below its title.
                button
                    .topAnchor()
                    .constraintEqualToAnchor(&center.stack.topAnchor())
                    .setActive(true);
                button
                    .bottomAnchor()
                    .constraintEqualToAnchor(&center.stack.bottomAnchor())
                    .setActive(true);
                self.items.push((chip.id.clone(), Slot::Center(button)));
                if rule_after(chips, index, centered) {
                    let rule = NSTextField::labelWithAttributedString(
                        &separator(&NSColor::secondaryLabelColor()),
                        mtm,
                    );
                    rule.setAccessibilityElement(false);
                    center.stack.addArrangedSubview(&rule);
                    self.rules
                        .push(Rule::Center(Retained::into_super(Retained::into_super(
                            rule,
                        ))));
                }
            }
        } else if !same {
            self.clear();
            self.center = None;
            let bar = NSStatusBar::systemStatusBar();
            // A new status item lands left of the existing ones, so the last
            // chip goes first and the first ends up leftmost.
            let mut created = Vec::with_capacity(chips.len());
            for (index, chip) in chips.iter().enumerate().rev() {
                if rule_after(chips, index, centered) {
                    let rule = bar.statusItemWithLength(NSVariableStatusItemLength);
                    if let Some(button) = rule.button(mtm) {
                        // A disabled button dims its title on its own.
                        let title = separator(&NSColor::labelColor());
                        rule.setLength(title.size().width + 2.0 * RULE_ROOM);
                        button.setAttributedTitle(&title);
                        button.setEnabled(false);
                        button.setAccessibilityElement(false);
                    }
                    self.rules.push(Rule::Status(rule));
                }
                let item = bar.statusItemWithLength(NSVariableStatusItemLength);
                if let Some(button) = item.button(mtm) {
                    button.setTag(index as isize);
                    // SAFETY: the target outlives the items (both live in
                    // `self`) and implements `itemClicked:`.
                    unsafe {
                        button.setTarget(Some(&self.target));
                        button.setAction(Some(sel!(itemClicked:)));
                    }
                    button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
                }
                let target = self.target.clone();
                let session = ExpandedSession::attach(&item, move |began| {
                    (target.ivars().callback)(if began {
                        ItemAction::Expanded { index }
                    } else {
                        ItemAction::Collapsed { index }
                    })
                });
                created.push((chip.id.clone(), Slot::Status(item, session)));
            }
            created.reverse();
            self.items = created;
        }
        for (index, (((_, item), chip), tip)) in
            self.items.iter().zip(chips).zip(tooltips).enumerate()
        {
            let before_account = chips
                .get(index + 1)
                .is_some_and(|next| vendor(&next.id) == vendor(&chip.id));
            let title = chip_title(chip);
            if let Slot::Status(item, _) = item {
                item.setLength(title.size().width + 2.0 * ITEM_ROOM);
            }
            if let Some(button) = item.button(mtm) {
                button.setAttributedTitle(&title);
                let tip = NSString::from_str(tip);
                button.setToolTip(Some(&tip));
                button.setAccessibilityLabel(Some(&tip));
                if let (Some(center), true) = (&self.center, before_account) {
                    center.stack.setCustomSpacing_afterView(
                        CENTER_ACCOUNT_GAP - 2.0 * CENTER_ROOM,
                        &button,
                    );
                }
            }
        }
        if let Some(center) = &self.center {
            center.chart_left.set(chart_left);
            center.place();
        }
    }

    /// Remove every provider item from the menu bar.
    pub fn clear(&mut self) {
        let bar = NSStatusBar::systemStatusBar();
        for (_, slot) in self.items.drain(..) {
            match slot {
                Slot::Status(item, _) => bar.removeStatusItem(&item),
                Slot::Center(button) => button.removeFromSuperview(),
            }
        }
        for rule in self.rules.drain(..) {
            match rule {
                Rule::Status(item) => bar.removeStatusItem(&item),
                Rule::Center(view) => view.removeFromSuperview(),
            }
        }
        if let Some(center) = &self.center {
            center.panel.orderOut(None);
        }
    }

    /// Where `id`'s item sits, left to right.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|(item, _)| item == id)
    }

    /// The provider shown by the item at `index`.
    pub fn id_at(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(|(id, _)| id.as_str())
    }

    /// Highlight the item at `open`, the one whose popover is showing, and
    /// clear the rest, as a native status item's menu does.
    pub fn highlight(&self, open: Option<usize>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        for (index, (_, slot)) in self.items.iter().enumerate() {
            let on = open == Some(index);
            match slot {
                Slot::Status(item, session) => {
                    if let Some(button) = item.button(mtm) {
                        // The menu bar draws its own capsule for a session.
                        mark_open(&button, on && session.is_none());
                    }
                    if let Some(session) = session {
                        session.sync(on);
                    }
                }
                Slot::Center(button) => mark_open(button, on),
            }
        }
        if let Some(center) = &self.center {
            center.hold(open.is_some());
        }
    }

    /// The host has handled the item's `Expanded` action.
    pub fn acknowledge(&self, index: usize) {
        if let Some((_, Slot::Status(_, Some(session)))) = self.items.get(index) {
            session.acknowledge();
        }
    }

    /// Whether a press at `timestamp` landed on the item at `index` while
    /// its session was open. See [`ExpandedSession::open_before`].
    pub fn session_open_before(&self, index: usize, timestamp: f64) -> bool {
        matches!(
            self.items.get(index),
            Some((_, Slot::Status(_, Some(session)))) if session.open_before(timestamp)
        )
    }

    /// Each item's frame in AppKit screen coordinates.
    pub fn frames(&self) -> Vec<(String, NSRect)> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        self.items
            .iter()
            .filter_map(|(id, slot)| {
                let button = slot.button(mtm)?;
                let window = button.window()?;
                let rect = button.convertRect_toView(button.bounds(), None);
                Some((id.clone(), window.convertRectToScreen(rect)))
            })
            .collect()
    }

    /// Pop `lines` up as a menu under the item at `index`.
    pub fn show_menu(&self, index: usize, lines: &[MenuLine]) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some((_, slot)) = self.items.get(index) else {
            return;
        };
        let Some(button) = slot.button(mtm) else {
            return;
        };
        let session = match slot {
            Slot::Status(_, session) => session.as_ref(),
            Slot::Center(_) => None,
        };
        self.show_menu_on_button(&button, lines, session);
    }

    /// Use the same native menu actions for the chart's own status button.
    /// With a `session`, the menu bar presents the menu and draws the same
    /// capsule as for the item's left click.
    pub fn show_menu_on_button(
        &self,
        button: &NSButton,
        lines: &[MenuLine],
        session: Option<&ExpandedSession>,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for line in lines {
            let entry = match line {
                MenuLine::Separator => NSMenuItem::separatorItem(mtm),
                MenuLine::Heading(title) => {
                    let entry = menu_item(mtm, title, None);
                    entry.setEnabled(false);
                    entry
                }
                MenuLine::Pick {
                    title,
                    tag,
                    checked,
                } => {
                    let entry = menu_item(mtm, title, Some(sel!(menuPicked:)));
                    entry.setTag(*tag);
                    // SAFETY: the target outlives the menu and implements
                    // `menuPicked:`.
                    unsafe { entry.setTarget(Some(&self.target)) };
                    if *checked {
                        entry.setState(NSControlStateValueOn);
                    }
                    entry
                }
            };
            menu.addItem(&entry);
        }
        if let Some(session) = session {
            session.present_menu(button, &menu);
            return;
        }
        let below = NSPoint::new(0.0, button.bounds().size.height + 4.0);
        // tray-icon highlights on right-down but does not clear it on
        // right-up. The popover/menu fill must be the only persistent one.
        button.highlight(false);
        menu.popUpMenuPositioningItem_atLocation_inView(None, below, Some(button));
        button.highlight(false);
    }
}

impl Drop for ProviderItems {
    fn drop(&mut self) {
        self.clear();
    }
}

fn menu_item(mtm: MainThreadMarker, title: &str, action: Option<Sel>) -> Retained<NSMenuItem> {
    // SAFETY: a plain title, an action the target implements (or none) and no
    // key equivalent.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    }
}

/// A rule drawn between providers.
enum Rule {
    Status(Retained<NSStatusItem>),
    Center(Retained<NSView>),
}

/// Stable app identity: a usage gauge with AI sparkles.
pub fn template_main_image(point: f64) -> Retained<NSImage> {
    let block = RcBlock::new(move |dst: NSRect| {
        let scale = dst.size.width.min(dst.size.height) / 18.0;
        let point =
            |x: f64, y: f64| NSPoint::new(dst.origin.x + x * scale, dst.origin.y + y * scale);
        NSColor::blackColor().setStroke();
        NSColor::blackColor().setFill();

        let rim = NSBezierPath::bezierPath();
        rim.setLineWidth(1.7 * scale);
        rim.setLineCapStyle(NSLineCapStyle::Round);
        // Leave room in the upper-right arc for the AI sparkle.
        for (start, end) in [(225.0, 95.0), (10.0, -45.0)] {
            rim.moveToPoint(point(
                8.1 + 6.4 * f64::to_radians(start).cos(),
                7.4 + 6.4 * f64::to_radians(start).sin(),
            ));
            rim.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
                point(8.1, 7.4),
                6.4 * scale,
                start,
                end,
                true,
            );
        }
        rim.stroke();

        let ticks = NSBezierPath::bezierPath();
        ticks.setLineWidth(1.3 * scale);
        ticks.setLineCapStyle(NSLineCapStyle::Round);
        for (from, to) in [
            ((3.8, 7.4), (4.6, 7.4)),
            ((7.7, 11.1), (7.7, 11.9)),
            ((11.8, 7.4), (12.6, 7.4)),
        ] {
            ticks.moveToPoint(point(from.0, from.1));
            ticks.lineToPoint(point(to.0, to.1));
        }
        ticks.stroke();

        let needle = NSBezierPath::bezierPath();
        needle.setLineWidth(1.8 * scale);
        needle.setLineCapStyle(NSLineCapStyle::Round);
        needle.moveToPoint(point(8.1, 7.4));
        needle.lineToPoint(point(10.7, 10.0));
        needle.stroke();
        NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
            point(6.8, 6.1),
            NSSize::new(2.6 * scale, 2.6 * scale),
        ))
        .fill();

        for (x, y, radius) in [(13.8, 13.4, 3.2), (2.6, 15.4, 1.25)] {
            let sparkle = NSBezierPath::bezierPath();
            let inset = radius * 0.18;
            sparkle.moveToPoint(point(x, y + radius));
            for (tip, before, after) in [
                (
                    (x + radius, y),
                    (x + inset, y + inset),
                    (x + inset, y + inset),
                ),
                (
                    (x, y - radius),
                    (x + inset, y - inset),
                    (x + inset, y - inset),
                ),
                (
                    (x - radius, y),
                    (x - inset, y - inset),
                    (x - inset, y - inset),
                ),
                (
                    (x, y + radius),
                    (x - inset, y + inset),
                    (x - inset, y + inset),
                ),
            ] {
                sparkle.curveToPoint_controlPoint1_controlPoint2(
                    point(tip.0, tip.1),
                    point(before.0, before.1),
                    point(after.0, after.1),
                );
            }
            sparkle.closePath();
            sparkle.fill();
        }
        Bool::from(true)
    });
    let image =
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(point, point), false, &block);
    image.setTemplate(true);
    image
}

/// Height of the menu bar's own capsule for a pressed or open status item
/// on macOS 27, measured on a 30-point bar. Every highlight this app draws
/// uses the same capsule so all items match the system's.
const CAPSULE_HEIGHT: f64 = 24.0;

/// The system capsule's shape inside `bounds`: full width, vertically centered.
fn capsule_rect(bounds: NSRect) -> NSRect {
    let height = CAPSULE_HEIGHT.min(bounds.size.height);
    NSRect::new(
        NSPoint::new(
            bounds.origin.x,
            bounds.origin.y + (bounds.size.height - height) / 2.0,
        ),
        NSSize::new(bounds.size.width, height),
    )
}

#[derive(Default)]
struct ChartFillIvars {
    active: Cell<bool>,
}

define_class!(
    // SAFETY: a passive background, without mouse handling or a custom cell.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubChartFill"]
    #[ivars = ChartFillIvars]
    struct ChartFill;

    impl ChartFill {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<&NSView> { None }

        #[unsafe(method(drawRect:))]
        fn draw(&self, _rect: NSRect) {
            if self.ivars().active.get() {
                pill_color(self).setFill();
                let rect = capsule_rect(self.bounds());
                let radius = rect.size.height / 2.0;
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius)
                    .fill();
            }
        }
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartAction {
    Pressed,
    Released(tray_icon::MouseButton),
    Cancelled,
}

struct ChartTargetIvars {
    fill: Retained<ChartFill>,
    callback: Box<dyn Fn(ChartAction)>,
    open: Cell<bool>,
    pressed: Cell<Option<tray_icon::MouseButton>>,
    inside: Cell<bool>,
    pending: Cell<bool>,
    /// The menu bar draws the press and open capsule itself (macOS 27).
    system: Cell<bool>,
}

define_class!(
    // SAFETY: an event-only view. The original NSStatusBarButton keeps its
    // native cell and image rendering; no superclass mouse tracking is used.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubChartTarget"]
    #[ivars = ChartTargetIvars]
    struct ChartTarget;

    impl ChartTarget {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) { self.begin(event, tray_icon::MouseButton::Left); }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) { self.finish(event, tray_icon::MouseButton::Left); }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) { self.begin(event, tray_icon::MouseButton::Right); }
        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) { self.finish(event, tray_icon::MouseButton::Right); }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 { self.begin(event, tray_icon::MouseButton::Middle); }
        }
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            if event.buttonNumber() == 2 { self.finish(event, tray_icon::MouseButton::Middle); }
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) { self.drag(event); }
        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) { self.drag(event); }
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) { self.drag(event); }
    }
);

impl ChartTarget {
    fn contains_event(&self, event: &NSEvent) -> bool {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let bounds = self.bounds();
        point.x >= bounds.min().x
            && point.x < bounds.max().x
            && point.y >= bounds.min().y
            && point.y < bounds.max().y
    }

    fn update_fill(&self) {
        let ivars = self.ivars();
        let press = ivars.pending.get() || (ivars.pressed.get().is_some() && ivars.inside.get());
        ivars
            .fill
            .ivars()
            .active
            .set(ivars.open.get() || (press && !ivars.system.get()));
        ivars.fill.setNeedsDisplay(true);
    }

    fn begin(&self, event: &NSEvent, button: tray_icon::MouseButton) {
        if !self.contains_event(event) {
            return;
        }
        // A new down is authoritative, including after a lost mouse-up during
        // menu-bar reordering or a change of Space. Never strand the receiver.
        self.ivars().pressed.set(Some(button));
        self.ivars().inside.set(true);
        self.ivars().pending.set(false);
        self.update_fill();
        (self.ivars().callback)(ChartAction::Pressed);
    }

    fn drag(&self, event: &NSEvent) {
        self.ivars().inside.set(self.contains_event(event));
        self.update_fill();
    }

    fn finish(&self, event: &NSEvent, button: tray_icon::MouseButton) {
        if self.ivars().pressed.get() != Some(button) {
            return;
        }
        self.ivars().pressed.set(None);
        let inside = self.contains_event(event);
        // Keep the same fill until the host acknowledges the action. This
        // avoids a blank frame between mouse-up and the queued popover open.
        self.ivars().pending.set(inside);
        self.update_fill();
        (self.ivars().callback)(if inside {
            ChartAction::Released(button)
        } else {
            ChartAction::Cancelled
        });
    }

    fn cancel(&self) {
        self.ivars().pressed.set(None);
        self.ivars().pending.set(false);
        self.update_fill();
    }
}

fn chart_target(button: &NSButton) -> Option<Retained<ChartTarget>> {
    button
        .subviews()
        .into_iter()
        .find_map(|view| view.downcast::<ChartTarget>().ok())
}

/// Install one receiver without tray-icon's independent pressed highlight.
pub fn install_chart_button(button: &NSButton, callback: impl Fn(ChartAction) + 'static) {
    if chart_target(button).is_some() {
        return;
    }
    let Some(content) = button.window().and_then(|window| window.contentView()) else {
        return;
    };
    let fill = ChartFill::alloc(button.mtm()).set_ivars(ChartFillIvars::default());
    // SAFETY: both views are initialized on the main thread in their parent's bounds.
    let fill: Retained<ChartFill> =
        unsafe { msg_send![super(fill), initWithFrame: content.bounds()] };
    fill.setAccessibilityElement(false);
    // A sibling behind the button cannot tint or cover its native template glyph.
    content.addSubview_positioned_relativeTo(&fill, NSWindowOrderingMode::Below, None);
    let target = ChartTarget::alloc(button.mtm()).set_ivars(ChartTargetIvars {
        fill,
        callback: Box::new(callback),
        open: Cell::new(false),
        pressed: Cell::new(None),
        inside: Cell::new(false),
        pending: Cell::new(false),
        system: Cell::new(false),
    });
    let target: Retained<ChartTarget> =
        unsafe { msg_send![super(target), initWithFrame: button.bounds()] };
    target.setAccessibilityElement(false);
    button.addSubview(&target);
    button.setTransparent(false);
    fit_chart_button(button);
}

/// Leave the press feedback to the menu bar, which draws it out of process.
/// A press drawn here as well would stack a second highlight on it.
pub fn use_system_highlight(button: &NSButton) {
    if let Some(target) = chart_target(button) {
        target.ivars().system.set(true);
        target.update_fill();
    }
}

pub fn cancel_chart_press(button: &NSButton) {
    if let Some(target) = chart_target(button) {
        target.cancel();
    }
}

/// Only the host handling the release may finish the queued click. A data
/// refresh can update `open` before that event reaches the host.
pub fn acknowledge_chart_click(button: &NSButton) {
    if let Some(target) = chart_target(button) {
        target.ivars().pending.set(false);
        target.update_fill();
    }
}

/// Expand the chart's native button and tray-icon's full-button event surface
/// to the actual menu bar height. AppKit initially gives the button 22 points
/// even when the status window is taller (30 points on current macOS).
pub fn fit_chart_button(button: &NSButton) {
    let Some(content) = button.window().and_then(|window| window.contentView()) else {
        return;
    };
    // SAFETY: the status item owns this live view and its parent on the main thread.
    let Some(parent) = (unsafe { button.superview() }) else {
        return;
    };
    let height = content.bounds().size.height;
    if height <= 0.0 {
        return;
    }
    let pin_to_edges = |view: &NSView, container: &NSView| {
        let identifier = NSString::from_str("ai-usagebar-main-button-edges");
        if container.constraints().iter().any(|constraint| {
            constraint.identifier().as_deref() == Some(&identifier)
                // SAFETY: these are live constraints installed on this view.
                && unsafe { constraint.firstItem() }
                    .is_some_and(|item| std::ptr::eq(&*item, view.as_ref() as &AnyObject))
        }) {
            return;
        }
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        for constraint in [
            view.topAnchor()
                .constraintEqualToAnchor(&container.topAnchor()),
            view.bottomAnchor()
                .constraintEqualToAnchor(&container.bottomAnchor()),
            view.leadingAnchor()
                .constraintEqualToAnchor(&container.leadingAnchor()),
            view.trailingAnchor()
                .constraintEqualToAnchor(&container.trailingAnchor()),
        ] {
            constraint.setIdentifier(Some(&identifier));
            constraint.setActive(true);
        }
    };
    // Current AppKit wraps the button in an NSView inset by four points at
    // both vertical edges. Expand that wrapper as well, or it clips the fill
    // and intercepts hit-testing before the button can receive an edge click.
    if parent != content {
        // SAFETY: same live view hierarchy as above.
        if unsafe { parent.superview() }.as_deref() != Some(content.as_ref()) {
            return;
        }
        pin_to_edges(&parent, &content);
    }
    if let Some(target) = chart_target(button) {
        // AppKit may move the status button to a different content view.
        // Reattach before adding constraints, which require a common ancestor.
        let fill = &target.ivars().fill;
        // SAFETY: the target retains this live view on the main thread.
        if unsafe { fill.superview() }.as_deref() != Some(content.as_ref()) {
            fill.removeFromSuperview();
            content.addSubview_positioned_relativeTo(fill, NSWindowOrderingMode::Below, None);
        }
        pin_to_edges(&target.ivars().fill, &content);
        pin_to_edges(&target, button);
    }
    // tray-icon 0.25 positions this child using button.frame(), which is in
    // the parent's coordinates. Identify it independently of that frame and
    // pin it to bounds so refreshes cannot leave an offset or a dead edge.
    for child in button.subviews() {
        if child.class().name().to_bytes() == b"TaoTrayTarget" {
            child.setHidden(chart_target(button).is_some());
            pin_to_edges(&child, button);
        }
    }
    // A frame assignment only lasts until AppKit's next layout, which puts
    // the original 22-point inset back. Constraints keep both surfaces full
    // height after image changes, tracking and window layout.
    pin_to_edges(button, &parent);
    content.layoutSubtreeIfNeeded();
}

/// Draw or clear the open-item highlight without replacing the native cell.
pub fn mark_open(button: &NSButton, on: bool) {
    // Clear tray-icon's independent pressed state before drawing our fill.
    // In particular, a right click or interrupted press can leave it behind
    // when the popover is dismissed outside the button.
    button.highlight(false);
    if let Some(target) = chart_target(button) {
        target.ivars().open.set(on);
        target.update_fill();
        if let Some(layer) = button.layer() {
            layer.setBackgroundColor(None);
        }
        return;
    }
    let view: &NSView = button;
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        // WindowServer passes physical clicks through fully transparent pixels
        // in the centered panel, even when NSView::hitTest finds the button.
        // Keep a nearly invisible fill over the whole centered button.
        let clickable = button
            .downcast_ref::<CenterButton>()
            .map(|_| NSColor::colorWithWhite_alpha(0.0, 0.01).CGColor());
        layer.setBackgroundColor(clickable.as_deref());
        layer.setCornerRadius(0.0);
        let capsule = capsule_layer(&layer);
        let rect = capsule_rect(view.bounds());
        capsule.setFrame(rect);
        capsule.setCornerRadius(rect.size.height / 2.0);
        let color = on.then(|| pill_color(view).CGColor());
        capsule.setBackgroundColor(color.as_deref());
    }
}

const CAPSULE_LAYER: &str = "ai-usagebar-capsule";

/// The open-item capsule behind a button's content, created once.
fn capsule_layer(layer: &CALayer) -> Retained<CALayer> {
    let name = NSString::from_str(CAPSULE_LAYER);
    // SAFETY: reading the sublayers of a live layer on the main thread.
    let existing = unsafe { layer.sublayers() }.and_then(|sublayers| {
        sublayers
            .iter()
            .find(|sublayer| sublayer.name().as_deref() == Some(&*name))
    });
    existing.unwrap_or_else(|| {
        let capsule = CALayer::new();
        capsule.setName(Some(&name));
        // Width follows the button when its title changes; the height stays
        // the capsule's, centered in the bar.
        capsule.setAutoresizingMask(
            CAAutoresizingMask::LayerWidthSizable
                | CAAutoresizingMask::LayerMinYMargin
                | CAAutoresizingMask::LayerMaxYMargin,
        );
        layer.insertSublayer_atIndex(&capsule, 0);
        capsule
    })
}

/// Instance state of [`SessionDelegate`].
pub struct SessionIvars {
    callback: Box<dyn Fn(bool)>,
    /// A session began and the host has not handled it yet. A redraw in
    /// between must not read the still-closed popover as a reason to cancel.
    opening: Cell<bool>,
    /// System uptime when the session began, the clock of `NSEvent::timestamp`.
    began_at: Cell<f64>,
    /// A context menu is showing; its session, until it ends, is not the
    /// item's interface.
    menu: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class does not
    // implement Drop. Both methods match NSStatusItemExpandedInterfaceDelegate.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubExpandedSessionDelegate"]
    #[ivars = SessionIvars]
    struct SessionDelegate;

    impl SessionDelegate {
        #[unsafe(method(statusItem:didBeginExpandedInterfaceSession:))]
        fn began(&self, _item: &NSStatusItem, _session: &AnyObject) {
            if self.ivars().menu.get() {
                return;
            }
            self.ivars().opening.set(true);
            self.ivars().began_at.set(NSProcessInfo::processInfo().systemUptime());
            (self.ivars().callback)(true);
        }

        #[unsafe(method(statusItemDidEndExpandedInterfaceSession:animated:))]
        fn ended(&self, _item: &NSStatusItem, _animated: Bool) {
            // The menu's own session ends as the menu closes, before its
            // fade-out lets `performClick` return. A press in between begins
            // the item's next session and must reach the host.
            if self.ivars().menu.replace(false) {
                return;
            }
            self.ivars().opening.set(false);
            (self.ivars().callback)(false);
        }
    }
);

/// A status item whose left click the menu bar tracks as an expanded
/// interface session (`NSStatusItemExpandedInterfaceDelegate`, macOS 27).
/// The menu bar then draws one capsule for both the press and the open item,
/// the same one every native status item gets. objc2-app-kit 0.3 predates
/// this API, so it is reached by selector and only where the OS has it.
pub struct ExpandedSession {
    item: Retained<NSStatusItem>,
    delegate: Retained<SessionDelegate>,
}

impl ExpandedSession {
    /// `callback` gets `true` when a session begins and `false` when it ends.
    /// `None` on macOS before 27, where the item keeps its own highlight.
    pub fn attach(item: &NSStatusItem, callback: impl Fn(bool) + 'static) -> Option<Self> {
        if !item.respondsToSelector(sel!(setExpandedInterfaceDelegate:)) {
            return None;
        }
        let class: &AnyClass = SessionDelegate::class();
        if let Some(protocol) = AnyProtocol::get(c"NSStatusItemExpandedInterfaceDelegate") {
            // SAFETY: the class implements both of the protocol's methods;
            // adding a protocol it already has is a no-op.
            unsafe {
                objc2::ffi::class_addProtocol(class as *const AnyClass as *mut AnyClass, protocol)
            };
        }
        let mtm = MainThreadMarker::new()?;
        let delegate = SessionDelegate::alloc(mtm).set_ivars(SessionIvars {
            callback: Box::new(callback),
            opening: Cell::new(false),
            began_at: Cell::new(f64::INFINITY),
            menu: Cell::new(false),
        });
        // SAFETY: NSObject's init takes no arguments and returns the object.
        let delegate: Retained<SessionDelegate> = unsafe { msg_send![super(delegate), init] };
        // SAFETY: the item holds the delegate weakly; `Self` keeps it alive
        // and clears the reference on drop.
        let _: () = unsafe { msg_send![item, setExpandedInterfaceDelegate: &*delegate] };
        Some(Self {
            item: item.retain(),
            delegate,
        })
    }

    fn session(&self) -> Option<Retained<AnyObject>> {
        // SAFETY: a readonly, nullable object property of a live item.
        unsafe { msg_send![&*self.item, expandedInterfaceSession] }
    }

    /// Whether the menu bar tracks an open session for this item now.
    pub fn active(&self) -> bool {
        self.session().is_some()
    }

    /// Whether a press at `timestamp` (`NSEvent::timestamp`) landed on a
    /// session that was already open. While one is, a click on the item
    /// reaches this app only through a global event monitor, and the host
    /// closes the interface itself. The monitor can deliver the very press
    /// that began the session after the session began, so the order in which
    /// the two arrive proves nothing; the press's own time does.
    pub fn open_before(&self, timestamp: f64) -> bool {
        let ivars = self.delegate.ivars();
        !ivars.opening.get() && ivars.began_at.get() < timestamp && self.active()
    }

    /// The host has acted on the session's beginning.
    pub fn acknowledge(&self) {
        self.delegate.ivars().opening.set(false);
    }

    /// Show `menu` from the item the way the menu bar shows a status item's
    /// own menu, with its capsule, and return once it closes. The menu is
    /// the item's only while it shows: an item with a menu sends its left
    /// click to the menu instead of the session.
    pub fn present_menu(&self, button: &NSButton, menu: &NSMenu) {
        let ivars = self.delegate.ivars();
        ivars.menu.set(true);
        self.item.setMenu(Some(menu));
        // SAFETY: the item's own live button; with a menu assigned, the
        // click tracks that menu and returns when it closes.
        unsafe { button.performClick(None) };
        self.item.setMenu(None);
        ivars.menu.set(false);
    }

    /// End the menu bar's session once this app's interface is no longer
    /// open for the item, e.g. closed by a click elsewhere or the keyboard.
    pub fn sync(&self, open: bool) {
        if open || self.delegate.ivars().opening.get() {
            return;
        }
        if let Some(session) = self.session() {
            // SAFETY: `cancel` takes no arguments; the delegate then receives
            // the end of the session.
            let _: () = unsafe { msg_send![&*session, cancel] };
        }
    }
}

impl Drop for ExpandedSession {
    fn drop(&mut self) {
        // SAFETY: clearing a weak delegate reference on a live item.
        let _: () = unsafe {
            msg_send![&*self.item, setExpandedInterfaceDelegate: std::ptr::null::<AnyObject>()]
        };
    }
}

/// The menu bar's highlight for an open item: a light wash on a dark bar, a
/// dark one on a light bar.
fn pill_color(view: &NSView) -> Retained<NSColor> {
    // SAFETY: these appearance names are immutable AppKit constants.
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let dark = view
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
        .is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua });
    if dark {
        // The menu bar's own capsule on a dark bar, measured on macOS 27.
        NSColor::colorWithWhite_alpha(1.0, 0.14)
    } else {
        NSColor::colorWithWhite_alpha(0.0, 0.12)
    }
}

fn vendor(id: &str) -> &str {
    id.split_once('@').map_or(id, |(vendor, _)| vendor)
}

/// Whether a rule follows the chip at `index`: before a different provider,
/// and before the chart glyph unless the providers are centered.
fn rule_after(chips: &[Chip], index: usize, centered: bool) -> bool {
    match chips.get(index + 1) {
        Some(next) => vendor(&next.id) != vendor(&chips[index].id),
        None => !centered,
    }
}

/// A chip as the item's title: its mark and value, or its name and value
/// where the mark cannot be drawn (SVG needs macOS 14).
fn chip_title(chip: &Chip) -> Retained<NSMutableAttributedString> {
    let font = NSFont::menuBarFontOfSize(0.0);
    let font_object: &AnyObject = font.as_ref();
    // SAFETY: NSFontAttributeName is an immutable AppKit constant.
    let font_key = unsafe { NSFontAttributeName };
    let label = NSColor::labelColor();
    let label_object: &AnyObject = label.as_ref();
    // SAFETY: NSForegroundColorAttributeName is an immutable AppKit constant.
    let color_key = unsafe { NSForegroundColorAttributeName };
    let attributes =
        NSDictionary::from_slices(&[font_key, color_key], &[font_object, label_object]);
    let run = |text: &str| {
        // SAFETY: the attributes map font and color keys to an NSFont and NSColor.
        unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(text),
                Some(&attributes),
            )
        }
    };
    let title = NSMutableAttributedString::new();
    match chip.mark.and_then(mark_image) {
        Some(image) => {
            let attachment = NSTextAttachment::new();
            attachment.setImage(Some(&image));
            // Centred on the menu bar font's cap height rather than its baseline.
            let offset = (font.capHeight() - MARK_SIDE) / 2.0;
            attachment.setBounds(NSRect::new(
                NSPoint::new(0.0, offset),
                NSSize::new(MARK_SIDE, MARK_SIDE),
            ));
            title.appendAttributedString(&NSAttributedString::attributedStringWithAttachment(
                &attachment,
            ));
            if let Some(value) = &chip.value {
                title.appendAttributedString(&run(" "));
                title.appendAttributedString(&value_run(value, chip.level, &font));
            }
        }
        None => {
            title.appendAttributedString(&run(&chip.name));
            if let Some(value) = &chip.value {
                title.appendAttributedString(&run(" "));
                title.appendAttributedString(&value_run(value, chip.level, &font));
            }
        }
    }
    if chip.active_account {
        title.appendAttributedString(&active_mark());
    }
    title
}

/// A faint vertical rule between providers, and before the chart glyph.
fn separator(color: &NSColor) -> Retained<NSAttributedString> {
    let font = NSFont::menuBarFontOfSize(0.0);
    let font_object: &AnyObject = font.as_ref();
    let color_object: &AnyObject = color.as_ref();
    // SAFETY: immutable AppKit attribute-name constants.
    let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let attributes = NSDictionary::from_slices(&keys, &[font_object, color_object]);
    // SAFETY: the attributes map each key to a value of its documented type.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str("|"),
            Some(&attributes),
        )
    }
}

/// Warning values share the active-account star's native yellow. Red uses
/// Dracula's tone on a dark menu bar and a darker tone on a light one.
/// Green keeps the menu bar's text color.
fn level_color(level: Level) -> Option<Retained<NSColor>> {
    let (dark, light) = match level {
        Level::Green => return None,
        Level::Yellow => return Some(NSColor::systemYellowColor()),
        Level::Red => ((0xff, 0x55, 0x55), (0xc4, 0x1e, 0x1e)),
    };
    let srgb = |(r, g, b): (u8, u8, u8)| {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        )
    };
    let (dark, light) = (srgb(dark), srgb(light));
    let provider = RcBlock::new(
        move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
            // SAFETY: AppKit passes a live appearance for the duration of the call.
            let appearance = unsafe { appearance.as_ref() };
            // SAFETY: these appearance names are immutable AppKit constants.
            let names =
                unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
            let is_dark = appearance
                .bestMatchFromAppearancesWithNames(&names)
                .is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua });
            NonNull::from(if is_dark { &*dark } else { &*light })
        },
    );
    // SAFETY: the provider returns colors it owns for as long as it lives.
    Some(unsafe { NSColor::colorWithName_dynamicProvider(None, &provider) })
}

/// The popover's star for the account in use, small and raised after the value.
fn active_mark() -> Retained<NSAttributedString> {
    let font = NSFont::menuBarFontOfSize(8.0);
    let font_object: &AnyObject = font.as_ref();
    let color = NSColor::systemYellowColor();
    let color_object: &AnyObject = color.as_ref();
    let raise = NSNumber::numberWithDouble(3.0);
    let raise_object: &AnyObject = raise.as_ref();
    // SAFETY: immutable AppKit attribute-name constants.
    let keys = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSBaselineOffsetAttributeName,
        ]
    };
    let attributes = NSDictionary::from_slices(&keys, &[font_object, color_object, raise_object]);
    // SAFETY: the attributes map each key to a value of its documented type.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(" ★"),
            Some(&attributes),
        )
    }
}

fn value_run(value: &str, level: Option<Level>, font: &NSFont) -> Retained<NSAttributedString> {
    let font_object: &AnyObject = font.as_ref();
    // SAFETY: NSFontAttributeName is an immutable AppKit constant.
    let font_key = unsafe { NSFontAttributeName };
    // An explicit color: a centered button dims a title that has none.
    let color = level
        .and_then(level_color)
        .unwrap_or_else(NSColor::labelColor);
    // SAFETY: NSForegroundColorAttributeName is an immutable AppKit constant.
    let color_key = unsafe { NSForegroundColorAttributeName };
    let color_object: &AnyObject = color.as_ref();
    let attributes =
        NSDictionary::from_slices(&[font_key, color_key], &[font_object, color_object]);
    // SAFETY: the attributes map font and color keys to an NSFont and NSColor.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(value),
            Some(&attributes),
        )
    }
}

/// A provider mark in the menu bar's text color, or `None` where AppKit
/// cannot read SVG. An image inside an attributed title is never tinted as a
/// template, so the mark is filled with `labelColor` each time it is drawn and
/// follows the menu bar between light and dark.
fn mark_image(svg: &str) -> Option<Retained<NSImage>> {
    // Only the shape's alpha is kept; a concrete fill keeps AppKit from
    // guessing what `currentColor` means outside a document.
    let svg = svg.replace("currentColor", "#000");
    let data = NSData::with_bytes(svg.as_bytes());
    let shape = NSImage::initWithData(NSImage::alloc(), &data)?;
    let size = NSSize::new(MARK_SIDE, MARK_SIDE);
    shape.setSize(size);
    let handler = RcBlock::new(move |rect: NSRect| -> Bool {
        shape.drawInRect(rect);
        NSColor::labelColor().set();
        NSRectFillUsingOperation(rect, NSCompositingOperation::SourceAtop);
        Bool::YES
    });
    Some(NSImage::imageWithSize_flipped_drawingHandler(
        size, false, &handler,
    ))
}

/// A chip's tooltip line: its name, its value and whether it is cached.
pub fn tooltip_line(chip: &Chip) -> String {
    let line = match &chip.value {
        Some(value) => format!("{} · {value}", chip.name),
        None => chip.name.clone(),
    };
    if chip.stale {
        format!("{line} · cached")
    } else {
        line
    }
}
