//! One macOS status item per provider, left of the tray's chart glyph.
//!
//! tray-icon owns a single `NSStatusItem`, so the providers get their own,
//! created and removed here as the menu bar's chips change. Each one draws its
//! provider's mark and value and reports clicks through one target object.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use std::ptr::NonNull;

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSApplicationDidChangeScreenParametersNotification,
    NSAttributedStringAttachmentConveniences, NSAttributedStringNSStringDrawing,
    NSBackingStoreType, NSBaselineOffsetAttributeName, NSButton, NSColor, NSCompositingOperation,
    NSControl, NSControlStateValueOn, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType,
    NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSLayoutAttribute,
    NSMenu, NSMenuItem, NSPanel, NSRectFillUsingOperation, NSResponder, NSScreen, NSStackView,
    NSStatusBar, NSStatusItem, NSStatusWindowLevel, NSTextAttachment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSVariableStatusItemLength, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSData, NSDictionary, NSMutableAttributedString,
    NSNotification, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};

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

/// What a provider item or its menu reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAction {
    /// A click on the item at `index`; `right` for a right or control click.
    Click { index: usize, right: bool },
    /// A pick in a provider's menu: the item's `tag`.
    Menu { tag: isize },
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
        let menu_end = Rc::new(Cell::new(menu_space::app_menu_end()));
        let chart_left = Rc::new(Cell::new(None));
        let block = {
            let (panel, stack) = (panel.clone(), stack.clone());
            let (menu_end, chart_left) = (menu_end.clone(), chart_left.clone());
            RcBlock::new(move |_: NonNull<NSNotification>| {
                if let Some(end) = menu_space::app_menu_end() {
                    menu_end.set(Some(end));
                }
                place(&panel, &stack, menu_end.get(), chart_left.get());
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
            observers,
        }
    }

    fn place(&self) {
        place(
            &self.panel,
            &self.stack,
            self.menu_end.get(),
            self.chart_left.get(),
        );
    }
}

impl Drop for CenterBar {
    fn drop(&mut self) {
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

/// Where a provider is drawn: its own status item, or a button in the center.
enum Slot {
    Status(Retained<NSStatusItem>),
    Center(Retained<NSButton>),
}

impl Slot {
    fn button(&self, mtm: MainThreadMarker) -> Option<Retained<NSButton>> {
        match self {
            Slot::Status(item) => item.button(mtm).map(Retained::into_super),
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
                created.push((chip.id.clone(), Slot::Status(item)));
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
            if let Slot::Status(item) = item {
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
                Slot::Status(item) => bar.removeStatusItem(&item),
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
                Slot::Status(item) => {
                    if let Some(button) = item.button(mtm) {
                        mark_open(&button, on);
                    }
                }
                Slot::Center(button) => mark_open(button, on),
            }
        }
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
        let Some(button) = self.items.get(index).and_then(|(_, slot)| slot.button(mtm)) else {
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
        let below = NSPoint::new(0.0, button.bounds().size.height + 4.0);
        menu.popUpMenuPositioningItem_atLocation_inView(None, below, Some(&button));
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

const PILL_RADIUS: f64 = 6.0;

/// Draw or clear the open-item highlight behind `button`. A status item's
/// button does not keep AppKit's highlight once its click ends, so the pill is
/// drawn on the button's own layer, the same size as the hover highlight.
pub fn mark_open(button: &NSButton, on: bool) {
    let view: &NSView = button;
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        let color = on.then(|| pill_color(view)).map(|c| c.CGColor());
        layer.setBackgroundColor(color.as_deref());
        layer.setCornerRadius(PILL_RADIUS);
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
        NSColor::colorWithWhite_alpha(1.0, 0.2)
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

/// Dracula's yellow and red on a dark menu bar; darker tones on a light one,
/// where Dracula's pastels would not read. Green keeps the menu bar's text
/// color, so only a quota running out draws the eye.
fn level_color(level: Level) -> Option<Retained<NSColor>> {
    let (dark, light) = match level {
        Level::Green => return None,
        Level::Yellow => ((0xf1, 0xfa, 0x8c), (0x9a, 0x74, 0x00)),
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
