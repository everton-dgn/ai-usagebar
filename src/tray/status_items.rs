//! One macOS status item per provider, left of the tray's chart glyph.
//!
//! tray-icon owns a single `NSStatusItem`, so the providers get their own,
//! created and removed here as the menu bar's chips change. Each one draws its
//! provider's mark and value and reports clicks through one target object.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAttributedStringAttachmentConveniences, NSColor, NSCompositingOperation,
    NSControlStateValueOn, NSEventMask, NSEventModifierFlags, NSEventType, NSFont,
    NSFontAttributeName, NSImage, NSKernAttributeName, NSMenu, NSMenuItem,
    NSRectFillUsingOperation, NSStatusBar, NSStatusItem, NSTextAttachment,
    NSVariableStatusItemLength,
};
use objc2_foundation::{
    MainThreadMarker, NSAttributedString, NSData, NSDictionary, NSMutableAttributedString,
    NSNumber, NSObject, NSPoint, NSRect, NSSize, NSString,
};

use super::menu_bar::Chip;

/// Side of a provider mark in the menu bar, in points.
const MARK_SIDE: f64 = 15.0;

const ITEM_PADDING: &str = "  ";
const PADDING_KERN: f64 = -2.0;
const CHART_GAP_KERN: f64 = 10.0;

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
    items: Vec<(String, Retained<NSStatusItem>)>,
    target: Retained<ItemTarget>,
}

impl ProviderItems {
    pub fn new(mtm: MainThreadMarker, callback: impl Fn(ItemAction) + 'static) -> Self {
        Self {
            items: Vec::new(),
            target: ItemTarget::new(mtm, Box::new(callback)),
        }
    }

    /// Show `chips`, one item each, with `tooltips` alongside. The items are
    /// rebuilt only when the providers or their order change; otherwise each
    /// one's title is redrawn in place.
    pub fn sync(&mut self, chips: &[Chip], tooltips: &[String]) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let same = self.items.len() == chips.len()
            && self
                .items
                .iter()
                .zip(chips)
                .all(|((id, _), chip)| *id == chip.id);
        if !same {
            self.clear();
            let bar = NSStatusBar::systemStatusBar();
            // A new status item lands left of the existing ones, so the last
            // chip goes first and the first ends up leftmost.
            let mut created = Vec::with_capacity(chips.len());
            for (index, chip) in chips.iter().enumerate().rev() {
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
                created.push((chip.id.clone(), item));
            }
            created.reverse();
            self.items = created;
        }
        let last = self.items.len().saturating_sub(1);
        for (index, (((_, item), chip), tip)) in
            self.items.iter().zip(chips).zip(tooltips).enumerate()
        {
            if let Some(button) = item.button(mtm) {
                button.setAttributedTitle(&chip_title(chip, index == last));
                button.setToolTip(Some(&NSString::from_str(tip)));
            }
        }
    }

    /// Remove every provider item from the menu bar.
    pub fn clear(&mut self) {
        let bar = NSStatusBar::systemStatusBar();
        for (_, item) in self.items.drain(..) {
            bar.removeStatusItem(&item);
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

    /// Each item's window frame in AppKit screen coordinates.
    pub fn frames(&self) -> Vec<(String, NSRect)> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        self.items
            .iter()
            .filter_map(|(id, item)| {
                let window = item.button(mtm)?.window()?;
                Some((id.clone(), window.frame()))
            })
            .collect()
    }

    /// Pop `lines` up as a menu under the item at `index`.
    pub fn show_menu(&self, index: usize, lines: &[MenuLine]) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(button) = self.items.get(index).and_then(|(_, item)| item.button(mtm)) else {
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

/// A chip as the item's title: its mark and value, or its name and value
/// where the mark cannot be drawn (SVG needs macOS 14).
fn chip_title(chip: &Chip, rightmost: bool) -> Retained<NSMutableAttributedString> {
    let font = NSFont::menuBarFontOfSize(0.0);
    let font_object: &AnyObject = font.as_ref();
    // SAFETY: NSFontAttributeName is an immutable AppKit constant.
    let font_key = unsafe { NSFontAttributeName };
    let attributes = NSDictionary::from_slices(&[font_key], &[font_object]);
    let run = |text: &str| {
        // SAFETY: the attributes map NSFontAttributeName to an NSFont.
        unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(text),
                Some(&attributes),
            )
        }
    };
    let padding = |kern: f64| {
        // SAFETY: NSKernAttributeName is an immutable AppKit constant.
        let kern_key = unsafe { NSKernAttributeName };
        let kern = NSNumber::numberWithDouble(kern / ITEM_PADDING.len() as f64);
        let kern_object: &AnyObject = kern.as_ref();
        let attributes =
            NSDictionary::from_slices(&[font_key, kern_key], &[font_object, kern_object]);
        // SAFETY: the attributes map NSFontAttributeName to an NSFont and
        // NSKernAttributeName to an NSNumber.
        unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(ITEM_PADDING),
                Some(&attributes),
            )
        }
    };
    let title = NSMutableAttributedString::new();
    title.appendAttributedString(&padding(PADDING_KERN));
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
                title.appendAttributedString(&run(&format!(" {value}")));
            }
        }
        None => title.appendAttributedString(&run(&chip.text())),
    }
    let trailing = PADDING_KERN + if rightmost { CHART_GAP_KERN } else { 0.0 };
    title.appendAttributedString(&padding(trailing));
    title
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
    match &chip.value {
        Some(value) => format!("{} · {value}", chip.name),
        None => chip.name.clone(),
    }
}
