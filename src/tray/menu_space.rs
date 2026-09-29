//! The free stretch of the menu bar between the active app's menus and the
//! status items, where centered providers sit.
//!
//! Both edges come from the Accessibility API: macOS 26 draws every app's
//! status items in one system window, so the window list cannot find them.
//! Without the permission only this app's own chart item is known.

use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGRect};
use std::ptr::NonNull;

#[path = "accessibility_prompt.rs"]
pub(crate) mod accessibility_prompt;

/// Whether this process holds the Accessibility permission.
pub fn trusted() -> bool {
    // SAFETY: a plain query with no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Guide the user through authorizing this exact running app. Called only
/// when the user turns centering on, never at launch or if already trusted.
pub fn request_access(changed: impl Fn() + 'static) {
    if trusted() {
        return;
    }
    accessibility_prompt::show(Box::new(changed));
}

/// Where the menus shown in the menu bar end, as Accessibility reports them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuEnd {
    /// The right edge of the last menu that has a size, in screen points
    /// from the left.
    pub x: Option<f64>,
    /// Whether the last menu has one. Right after an app comes to the front,
    /// the menu bar can still be laying out its menus.
    pub laid_out: bool,
}

/// Where the menus shown in the menu bar end, or `None` without the
/// permission or when Accessibility cannot read them, as for a hung app.
pub fn app_menu_end() -> Option<MenuEnd> {
    if !trusted() {
        return None;
    }
    // An accessory app in front, such as this one or a launcher, leaves the
    // menu bar to the last regular app; its own menus are never on screen.
    let app = NSWorkspace::sharedWorkspace().menuBarOwningApplication()?;
    let pid = app.processIdentifier();
    if pid == std::process::id() as i32 {
        return None;
    }
    // SAFETY: any pid is accepted; a gone process makes later calls fail.
    let element = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: a plain setter on a live element.
    unsafe { element.set_messaging_timeout(MENUS_TIMEOUT) };
    let bar = attribute(&element, "AXMenuBar")?
        .downcast::<AXUIElement>()
        .ok()?;
    Some(menu_end(
        children(&bar, MENUS_TIMEOUT)?
            .iter()
            .map(|item| frame(item)),
    ))
}

/// How long Accessibility waits on the app whose menus are shown, and on
/// each other app for its status items: a hung app must not stall the menu
/// bar.
const MENUS_TIMEOUT: f32 = 0.25;
const EXTRAS_TIMEOUT: f32 = 0.1;

/// Where menus with these frames, left to right, end. `None` stands for a
/// menu without a size. Only the frames up to the last menu that has one,
/// from the right, are read.
fn menu_end(frames: impl DoubleEndedIterator<Item = Option<CGRect>>) -> MenuEnd {
    let mut frames = frames.rev();
    let last = frames.next().flatten();
    let drawn = last.or_else(|| frames.flatten().next());
    MenuEnd {
        x: drawn.map(|rect| rect.origin.x + rect.size.width),
        laid_out: last.is_some(),
    }
}

/// `element`'s children, asked for and then read within `timeout`, which
/// Accessibility keeps only on the element it was set on.
fn children(element: &AXUIElement, timeout: f32) -> Option<Vec<CFRetained<AXUIElement>>> {
    // SAFETY: a plain setter on a live element.
    unsafe { element.set_messaging_timeout(timeout) };
    let array = attribute(element, "AXChildren")?
        .downcast::<CFArray>()
        .ok()?;
    // SAFETY: an AXChildren value is an array of AXUIElements.
    let array: CFRetained<CFArray<AXUIElement>> = unsafe { CFRetained::cast_unchecked(array) };
    Some(
        array
            .iter()
            // SAFETY: a plain setter on a live element.
            .inspect(|child| unsafe {
                child.set_messaging_timeout(timeout);
            })
            .collect(),
    )
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_static_str(name);
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: `value` is a valid out-pointer for a +1 reference.
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if error != AXError::Success {
        return None;
    }
    // SAFETY: on success the value is a +1 reference we now own.
    NonNull::new(value.cast_mut()).map(|value| unsafe { CFRetained::from_raw(value) })
}

fn frame(element: &AXUIElement) -> Option<CGRect> {
    let value = attribute(element, "AXFrame")?.downcast::<AXValue>().ok()?;
    let mut rect = CGRect::default();
    // SAFETY: an AXFrame value holds a CGRect, written into `rect`.
    let ok = unsafe { value.value(AXValueType::CGRect, NonNull::from(&mut rect).cast()) };
    ok.then_some(rect).filter(on_screen)
}

/// Whether Accessibility reports `rect` as drawn. A menu that is not, such as
/// an accessory app's or one the menu bar has yet to lay out, sits at the
/// bar's bottom-left corner, less than a point wide or tall.
fn on_screen(rect: &CGRect) -> bool {
    rect.size.width >= 1.0 && rect.size.height >= 1.0
}

/// The left edge of the leftmost status item, from every other app's menu
/// extras and `own` (this app's chart item, which Accessibility cannot ask
/// this process about without blocking it).
pub fn status_items_start(own: Option<f64>, bar_height: f64) -> Option<f64> {
    if !trusted() {
        return own;
    }
    let me = std::process::id() as i32;
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|app| app.activationPolicy() != NSApplicationActivationPolicy::Prohibited)
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid != me)
        .filter_map(|pid| {
            // SAFETY: any pid is accepted; a gone process makes later calls fail.
            let element = unsafe { AXUIElement::new_application(pid) };
            // SAFETY: a plain setter on a live element.
            unsafe { element.set_messaging_timeout(EXTRAS_TIMEOUT) };
            let extras = attribute(&element, "AXExtrasMenuBar")?
                .downcast::<AXUIElement>()
                .ok()?;
            children(&extras, EXTRAS_TIMEOUT)?
                .iter()
                .filter_map(|item| frame(item))
                .filter(|rect| rect.origin.y < bar_height)
                .map(|rect| rect.origin.x)
                .reduce(f64::min)
        })
        .chain(own)
        .reduce(f64::min)
}

/// Where a `width`-wide strip starts so it sits in the middle of the free
/// stretch `left..right`. With the left edge unknown or no room, it takes the
/// middle of the screen `(x, width)`, pulled left of `right` when that would
/// cover the status items.
pub fn centered_x(width: f64, screen: (f64, f64), left: Option<f64>, right: Option<f64>) -> f64 {
    let middle = screen.0 + (screen.1 - width) / 2.0;
    match (left, right) {
        (Some(left), Some(right)) if right - left >= width => left + (right - left - width) / 2.0,
        (_, Some(right)) => middle.min(right - width).max(screen.0),
        _ => middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CGPoint, CGSize};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
    }

    #[test]
    fn a_menu_without_a_size_is_not_on_screen() {
        // A menu the menu bar has not drawn and ChatGPT's Help menu, as
        // macOS 27 reports them.
        assert!(!on_screen(&rect(0.0, 30.0, 0.0, 0.0)));
        assert!(on_screen(&rect(323.0, 0.0, 49.0, 30.0)));
        assert!(!on_screen(&rect(-1.0, 30.0, 1.0, 0.0)));
    }

    #[test]
    fn the_menus_are_laid_out_once_the_last_one_is() {
        let apple = Some(rect(10.0, 0.0, 34.0, 30.0));
        let help = Some(rect(323.0, 0.0, 49.0, 30.0));
        let end = |x, laid_out| MenuEnd { x, laid_out };
        // ChatGPT's menus while the menu bar redrew the first ones.
        assert_eq!(
            menu_end([apple, None, None, help].into_iter()),
            end(Some(372.0), true)
        );
        // Those of an app that has just come to the front, or has none yet.
        assert_eq!(menu_end([None, None].into_iter()), end(None, false));
        assert_eq!(menu_end(std::iter::empty()), end(None, false));
        // Only the first ones laid out: where they end is not the end yet.
        assert_eq!(menu_end([apple, None].into_iter()), end(Some(44.0), false));
    }

    #[test]
    fn the_strip_centers_in_the_free_stretch() {
        assert_eq!(
            centered_x(100.0, (0.0, 2000.0), Some(200.0), Some(1600.0)),
            850.0
        );
    }

    #[test]
    fn an_unknown_edge_centers_on_the_screen() {
        assert_eq!(centered_x(100.0, (0.0, 2000.0), None, Some(1600.0)), 950.0);
        assert_eq!(centered_x(200.0, (0.0, 2000.0), Some(400.0), None), 900.0);
    }

    #[test]
    fn the_strip_stays_left_of_the_status_items() {
        assert_eq!(centered_x(100.0, (0.0, 2000.0), None, Some(1000.0)), 900.0);
        assert_eq!(
            centered_x(500.0, (0.0, 2000.0), Some(900.0), Some(1200.0)),
            700.0
        );
    }
}
