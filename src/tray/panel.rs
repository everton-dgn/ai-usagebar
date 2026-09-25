//! Shared popover geometry. Pure numbers so Linux CI can test the clamp
//! without compiling a WebView host.
#![allow(dead_code)]

/// Width of the macOS provider switcher in logical points.
pub const WINDOW_WIDTH: f64 = 390.0;
/// Initial height only: the web content drives it afterwards via `resize`.
pub const WINDOW_HEIGHT: f64 = 420.0;
/// Smallest height a `resize` request can shrink the popover to.
/// Sized so the footer Options menu (nine rows, opens upward) fits without
/// Radix scrolling the list on short screens like Customize / provider detail.
pub const MIN_POPOVER_HEIGHT: f64 = 360.0;
/// Narrowest the macOS popover can be dragged to.
pub const MIN_POPOVER_WIDTH: f64 = 320.0;
/// Shortest the macOS popover can be dragged to. Content taller than the
/// dragged height scrolls instead of growing the panel.
pub const MIN_USER_HEIGHT: f64 = 200.0;
/// Breathing room kept between the popover and the monitor's edges.
pub const WORK_AREA_MARGIN: f64 = 16.0;
/// Gap between the bottom of the menu bar and the top of the popover.
/// Zero so the panel hangs flush under the status item, like a menu.
pub const POPOVER_TOP_GAP: f64 = 0.0;
/// Gap between the popover and the bottom of the visible screen (dock / desktop).
pub const POPOVER_BOTTOM_GAP: f64 = 24.0;
/// Horizontal inset from the visible screen edges.
pub const POPOVER_SIDE_MARGIN: f64 = 8.0;
/// Tall dashboards stop short of filling the column, matching OpenUsage.
pub const POPOVER_MAX_HEIGHT_FRACTION: f64 = 0.85;
/// Used when `visibleFrame` does not subtract the menu bar (accessory apps).
pub const MENU_BAR_MIN_HEIGHT: f64 = 24.0;
/// Used when no monitor can be resolved at all.
pub const FALLBACK_WORK_AREA_HEIGHT: f64 = 800.0;
/// Absorb the mouse-up that opened the popover so it cannot hit the footer.
pub const CLICK_LOCK_MS: u64 = 400;
/// Opaque WebView background (WebView2 ignores translucency; WKWebView matches).
pub const LIGHT_BACKGROUND: (u8, u8, u8, u8) = (255, 255, 255, 255);
pub const DARK_BACKGROUND: (u8, u8, u8, u8) = (30, 30, 30, 255);
/// Corner radius of the panel surface; tuned to read like a system popover.
pub const CORNER_RADIUS: f64 = 13.0;

/// Height the popover may grow to for `requested` logical px: rounded, never
/// below [`MIN_POPOVER_HEIGHT`], never past the work area minus its margin.
pub fn clamp_popover_height(requested: f64, work_area_height: f64) -> f64 {
    let max = (work_area_height - WORK_AREA_MARGIN).max(MIN_POPOVER_HEIGHT);
    requested.round().clamp(MIN_POPOVER_HEIGHT, max)
}

/// Whether losing focus should close an open popover.
///
/// Pressing the status item takes focus away from the popover before that
/// press's click arrives. Closing on the blur would leave the click to find
/// the popover closed and open it again, so a press on the status item is
/// left to its click, which closes it.
pub fn close_on_blur(guarded: bool, press_on_status_item: bool) -> bool {
    !guarded && !press_on_status_item
}

/// The popover height for the content's `requested` height: automatic as
/// before, but never taller than the height the user dragged the panel to.
/// Below the content's height the list scrolls.
pub fn fit_popover_height(requested: f64, work_area_height: f64, cap: Option<f64>) -> f64 {
    let auto = clamp_popover_height(requested, work_area_height);
    match cap {
        Some(cap) => auto.min(cap.round().max(MIN_USER_HEIGHT)),
        None => auto,
    }
}

/// The size the user dragged the macOS popover to, remembered across runs.
/// `max_height` caps the content-driven height; `None` keeps it automatic.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanelSize {
    pub width: f64,
    pub max_height: Option<f64>,
}

impl Default for PanelSize {
    fn default() -> Self {
        Self {
            width: WINDOW_WIDTH,
            max_height: None,
        }
    }
}

impl PanelSize {
    /// What Reset Panel Size returns to: the narrowest width and the
    /// automatic height.
    pub fn reset() -> Self {
        Self {
            width: MIN_POPOVER_WIDTH,
            max_height: None,
        }
    }

    /// A size from a drag, rounded and held to the minimums.
    pub fn dragged(width: f64, height: f64) -> Self {
        Self {
            width: width.round().max(MIN_POPOVER_WIDTH),
            max_height: Some(height.round().max(MIN_USER_HEIGHT)),
        }
    }

    /// The saved size, or `None` for anything malformed: a bad file falls
    /// back to the default size rather than to a broken panel.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let raw: Self = serde_json::from_slice(bytes).ok()?;
        let valid = |v: f64| v.is_finite() && v > 0.0;
        if !valid(raw.width) || raw.max_height.is_some_and(|h| !valid(h)) {
            return None;
        }
        Some(Self {
            width: raw.width.round().max(MIN_POPOVER_WIDTH),
            max_height: raw.max_height.map(|h| h.round().max(MIN_USER_HEIGHT)),
        })
    }
}

/// Axis-aligned rectangle in Cocoa screen space: origin at the bottom-left of
/// the primary display, y increasing up, units in points (not pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CocoaRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl CocoaRect {
    pub fn contains(self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn max_x(self) -> f64 {
        self.x + self.w
    }

    pub fn max_y(self) -> f64 {
        self.y + self.h
    }
}

/// Inputs for [`cocoa_popover_frame`]. `below_y` is the Cocoa min-Y of the
/// menu bar (bottom edge of `NSStatusBarWindow`); the popover hangs
/// [`POPOVER_TOP_GAP`] under that and keeps [`POPOVER_BOTTOM_GAP`] above the
/// visible bottom (dock / desktop).
#[derive(Debug, Clone, Copy)]
pub struct PopoverPlacement {
    pub visible: CocoaRect,
    pub below_y: f64,
    pub icon_x: f64,
    pub popover_w: f64,
    pub popover_h: f64,
}

/// Infer the Cocoa Y of the menu-bar's bottom edge when we cannot see the
/// status-item window. `visibleFrame` *should* already exclude the bar; some
/// accessory apps get `visibleFrame == frame`, so we never trust a zero inset.
pub fn menu_bar_bottom_y(screen: CocoaRect, visible: CocoaRect) -> f64 {
    let inset = (screen.max_y() - visible.max_y()).max(0.0);
    screen.max_y() - inset.max(MENU_BAR_MIN_HEIGHT)
}

/// Popover frame in Cocoa coordinates (origin bottom-left, y up, points).
pub fn cocoa_popover_frame(p: PopoverPlacement) -> CocoaRect {
    let top = p.below_y - POPOVER_TOP_GAP;
    let bottom = p.visible.y + POPOVER_BOTTOM_GAP;
    let available = (top - bottom).max(1.0);
    let max_h = (available * POPOVER_MAX_HEIGHT_FRACTION).min(available);
    let h = p.popover_h.min(max_h).max(1.0);
    let max_w = (p.visible.w - 2.0 * POPOVER_SIDE_MARGIN).max(1.0);
    let w = p.popover_w.min(max_w);
    let min_x = p.visible.x + POPOVER_SIDE_MARGIN;
    let max_x = p.visible.max_x() - POPOVER_SIDE_MARGIN - w;
    let x = (p.icon_x - w / 2.0).clamp(min_x, max_x.max(min_x));
    let y = (top - h).max(bottom);
    CocoaRect { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::{
        CocoaRect, MENU_BAR_MIN_HEIGHT, MIN_POPOVER_HEIGHT, MIN_POPOVER_WIDTH, MIN_USER_HEIGHT,
        POPOVER_BOTTOM_GAP, POPOVER_SIDE_MARGIN, POPOVER_TOP_GAP, PanelSize, PopoverPlacement,
        WINDOW_WIDTH, WORK_AREA_MARGIN, clamp_popover_height, close_on_blur, cocoa_popover_frame,
        fit_popover_height, menu_bar_bottom_y,
    };

    #[test]
    fn a_press_on_the_status_item_leaves_closing_to_its_click() {
        assert!(close_on_blur(false, false));
        assert!(!close_on_blur(false, true));
        // Just opened: the click that opened it must not close it again.
        assert!(!close_on_blur(true, false));
    }

    #[test]
    fn fit_without_a_dragged_height_is_the_automatic_height() {
        assert_eq!(fit_popover_height(512.4, 1080.0, None), 512.0);
        assert_eq!(fit_popover_height(40.0, 1080.0, None), MIN_POPOVER_HEIGHT);
    }

    #[test]
    fn a_dragged_height_caps_taller_content_so_the_list_scrolls() {
        assert_eq!(fit_popover_height(700.0, 1080.0, Some(300.0)), 300.0);
        // Below the automatic floor too: the user asked for a shorter panel.
        assert_eq!(fit_popover_height(400.0, 1080.0, Some(250.0)), 250.0);
    }

    #[test]
    fn a_dragged_height_never_grows_the_panel_past_its_content() {
        assert_eq!(fit_popover_height(420.0, 1080.0, Some(900.0)), 420.0);
    }

    #[test]
    fn a_dragged_height_keeps_the_minimum() {
        assert_eq!(
            fit_popover_height(700.0, 1080.0, Some(50.0)),
            MIN_USER_HEIGHT
        );
    }

    #[test]
    fn a_drag_is_rounded_and_held_to_the_minimums() {
        assert_eq!(
            PanelSize::dragged(512.6, 333.3),
            PanelSize {
                width: 513.0,
                max_height: Some(333.0)
            }
        );
        let tiny = PanelSize::dragged(10.0, 10.0);
        assert_eq!(tiny.width, MIN_POPOVER_WIDTH);
        assert_eq!(tiny.max_height, Some(MIN_USER_HEIGHT));
    }

    #[test]
    fn a_reset_goes_to_the_minimum_width_and_the_automatic_height() {
        let reset = PanelSize::reset();
        assert_eq!(reset.width, MIN_POPOVER_WIDTH);
        assert_eq!(reset.max_height, None);
    }

    #[test]
    fn a_saved_size_round_trips() {
        let size = PanelSize::dragged(560.0, 480.0);
        let bytes = serde_json::to_vec(&size).unwrap();
        assert_eq!(PanelSize::parse(&bytes), Some(size));
        let auto = serde_json::to_vec(&PanelSize::default()).unwrap();
        assert_eq!(
            PanelSize::parse(&auto),
            Some(PanelSize {
                width: WINDOW_WIDTH,
                max_height: None
            })
        );
    }

    #[test]
    fn a_malformed_saved_size_is_ignored() {
        assert_eq!(PanelSize::parse(b"not json"), None);
        assert_eq!(PanelSize::parse(br#"{"width":-5,"max_height":null}"#), None);
        assert_eq!(PanelSize::parse(br#"{"width":500,"max_height":0}"#), None);
        assert_eq!(
            PanelSize::parse(br#"{"width":100,"max_height":null}"#).map(|s| s.width),
            Some(MIN_POPOVER_WIDTH)
        );
    }

    fn place(visible: CocoaRect, icon_x: f64, h: f64) -> CocoaRect {
        cocoa_popover_frame(PopoverPlacement {
            visible,
            below_y: menu_bar_bottom_y(visible, visible),
            icon_x,
            popover_w: 320.0,
            popover_h: h,
        })
    }

    #[test]
    fn clamp_raises_requests_below_the_minimum() {
        assert_eq!(clamp_popover_height(40.0, 1080.0), MIN_POPOVER_HEIGHT);
    }

    #[test]
    fn clamp_keeps_requests_within_range_rounded() {
        assert_eq!(clamp_popover_height(512.4, 1080.0), 512.0);
        assert_eq!(clamp_popover_height(512.6, 1080.0), 513.0);
    }

    #[test]
    fn clamp_caps_requests_at_work_area_minus_margin() {
        assert_eq!(
            clamp_popover_height(5000.0, 1080.0),
            1080.0 - WORK_AREA_MARGIN
        );
    }

    #[test]
    fn clamp_never_shrinks_below_minimum_on_tiny_work_area() {
        assert_eq!(clamp_popover_height(300.0, 100.0), MIN_POPOVER_HEIGHT);
        assert_eq!(clamp_popover_height(50.0, 10.0), MIN_POPOVER_HEIGHT);
    }

    fn primary() -> CocoaRect {
        CocoaRect {
            x: 0.0,
            y: 0.0,
            w: 1440.0,
            h: 900.0,
        }
    }

    fn right_of_primary() -> CocoaRect {
        CocoaRect {
            x: 1440.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        }
    }

    #[test]
    fn popover_on_a_right_hand_screen_does_not_jump_to_the_primary() {
        let visible = right_of_primary();
        let frame = place(visible, 2400.0, 420.0);
        assert!(
            frame.x >= visible.x,
            "left edge {} must stay on the secondary (origin {})",
            frame.x,
            visible.x
        );
        assert!(frame.max_x() <= visible.max_x() + 0.01);
        let expected_top = menu_bar_bottom_y(visible, visible) - POPOVER_TOP_GAP;
        assert!((frame.max_y() - expected_top).abs() < 0.01);
    }

    #[test]
    fn popover_near_the_right_edge_is_clamped_onto_that_screen() {
        let visible = right_of_primary();
        let frame = place(visible, 1440.0 + 1910.0, 420.0);
        assert!((frame.max_x() - (visible.max_x() - POPOVER_SIDE_MARGIN)).abs() < 0.01);
        assert!(frame.x >= visible.x + POPOVER_SIDE_MARGIN);
    }

    #[test]
    fn popover_on_a_left_hand_screen_keeps_a_negative_origin() {
        let left = CocoaRect {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let frame = place(left, -200.0, 420.0);
        assert!(frame.x >= left.x);
        assert!(frame.max_x() <= left.max_x() + 0.01);
        assert!(frame.x < 0.0, "must not be shifted onto the primary");
    }

    #[test]
    fn a_tall_popover_keeps_a_gap_above_the_bottom_of_the_screen() {
        let visible = primary();
        let frame = place(visible, 200.0, 5000.0);
        assert!(frame.y >= visible.y + POPOVER_BOTTOM_GAP - 0.01);
        assert!(frame.max_y() <= visible.max_y() - MENU_BAR_MIN_HEIGHT + 0.01);
        assert!(frame.h < visible.h);
    }

    #[test]
    fn accessory_visible_frame_still_clears_the_menu_bar() {
        let screen = primary();
        // Accessory apps sometimes see visibleFrame == frame (no menu-bar inset).
        let below = menu_bar_bottom_y(screen, screen);
        assert_eq!(below, screen.max_y() - MENU_BAR_MIN_HEIGHT);
        let frame = cocoa_popover_frame(PopoverPlacement {
            visible: screen,
            below_y: below,
            icon_x: 200.0,
            popover_w: 320.0,
            popover_h: 420.0,
        });
        assert!(
            (frame.max_y() - (below - POPOVER_TOP_GAP)).abs() < 0.01,
            "popover top {} must sit flush under the menu bar at {}",
            frame.max_y(),
            below
        );
    }

    #[test]
    fn popover_top_is_flush_with_the_menu_bar() {
        let visible = primary();
        let below = menu_bar_bottom_y(visible, visible);
        let frame = place(visible, 200.0, 420.0);
        assert_eq!(frame.max_y(), below);
        assert!(frame.max_y() < visible.max_y());
    }
}
