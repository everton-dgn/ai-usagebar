//! The free stretch of the menu bar between the active app's menus and the
//! status items, where centered providers sit.
//!
//! Both edges come from the Accessibility API: macOS 26 draws every app's
//! status items in one system window, so the window list cannot find them.
//! Without the permission only this app's own chart item is known.

use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
use objc2_application_services::{
    AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElement, AXValue, AXValueType,
    kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, CGRect,
};
use std::ptr::NonNull;

/// Whether this process holds the Accessibility permission.
pub fn trusted() -> bool {
    // SAFETY: a plain query with no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Ask for the Accessibility permission that reading the app's menus needs;
/// macOS shows its own prompt when it is missing. Called only when the user
/// turns centering on, never at launch, so a restart does not prompt again.
pub fn request_access() {
    // SAFETY: an immutable HIServices constant.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: the options dictionary maps the prompt key to a CFBoolean.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
}

/// Where the frontmost app's menus end, in screen points from the left, or
/// `None` without the permission, for this app itself, or on any failure.
pub fn app_menu_end() -> Option<f64> {
    if !trusted() {
        return None;
    }
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    if pid == std::process::id() as i32 {
        return None;
    }
    // SAFETY: any pid is accepted; a gone process makes later calls fail.
    let element = unsafe { AXUIElement::new_application(pid) };
    // A hung app must not stall the menu bar.
    // SAFETY: a plain setter on a live element.
    unsafe { element.set_messaging_timeout(0.25) };
    let bar = attribute(&element, "AXMenuBar")?
        .downcast::<AXUIElement>()
        .ok()?;
    let last = children(&bar)?.iter().rev().find_map(|item| frame(item))?;
    Some(last.origin.x + last.size.width)
}

fn children(element: &AXUIElement) -> Option<Vec<CFRetained<AXUIElement>>> {
    let array = attribute(element, "AXChildren")?
        .downcast::<CFArray>()
        .ok()?;
    // SAFETY: an AXChildren value is an array of AXUIElements.
    let array: CFRetained<CFArray<AXUIElement>> = unsafe { CFRetained::cast_unchecked(array) };
    Some(array.iter().collect())
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
    ok.then_some(rect)
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
            unsafe { element.set_messaging_timeout(0.1) };
            let extras = attribute(&element, "AXExtrasMenuBar")?
                .downcast::<AXUIElement>()
                .ok()?;
            children(&extras)?
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
