use gtk::prelude::*;
use std::cell::Cell;
use std::time::Duration;

const WINDOWED_WIDTH: i32 = 1280;
const WINDOWED_HEIGHT: i32 = 720;
const WINDOWED_VIEWPORT_HEIGHT_REDUCTION: i32 = 30;
const WINDOWED_MONITOR_MARGIN_X: i32 = 48;
const WINDOWED_MONITOR_MARGIN_Y: i32 = 96;
const WINDOWED_MIN_WIDTH: i32 = 640;
const WINDOWED_MIN_HEIGHT: i32 = 360;

thread_local! {
    static FULLSCREEN_ENABLED: Cell<bool> = const { Cell::new(true) };
}

pub fn apply_fullscreen_policy(window: &gtk::ApplicationWindow, enabled: bool) {
    set_fullscreen_enabled(window, enabled);

    let weak_window = window.downgrade();
    gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
        let Some(window) = weak_window.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        if fullscreen_enabled() && window.is_visible() && !window.is_fullscreen() {
            window.fullscreen();
        }
        gtk::glib::ControlFlow::Continue
    });
}

pub fn set_fullscreen_enabled(window: &gtk::ApplicationWindow, enabled: bool) {
    FULLSCREEN_ENABLED.with(|state| state.set(enabled));
    apply_current_policy(window);
}

pub fn reassert_fullscreen(window: &gtk::ApplicationWindow) {
    apply_current_policy(window);
}

fn fullscreen_enabled() -> bool {
    FULLSCREEN_ENABLED.with(Cell::get)
}

fn apply_current_policy(window: &gtk::ApplicationWindow) {
    if fullscreen_enabled() {
        window.set_decorated(false);
        window.set_resizable(false);
        window.fullscreen();
    } else {
        window.unfullscreen();
        window.set_decorated(true);
        window.set_resizable(true);
        let (width, height) = windowed_default_size();
        window.set_default_size(width, height);
    }
}

fn windowed_default_size() -> (i32, i32) {
    monitor_geometry()
        .map(|(width, height)| capped_windowed_size(width, height))
        .unwrap_or((WINDOWED_WIDTH, WINDOWED_HEIGHT))
}

fn monitor_geometry() -> Option<(i32, i32)> {
    let display = gtk::gdk::Display::default()?;
    let monitor = display
        .monitors()
        .item(0)?
        .downcast::<gtk::gdk::Monitor>()
        .ok()?;
    let geometry = monitor.geometry();
    Some((geometry.width(), geometry.height()))
}

fn capped_windowed_size(monitor_width: i32, monitor_height: i32) -> (i32, i32) {
    let width_floor = WINDOWED_MIN_WIDTH.min(monitor_width.max(1));
    let height_floor = WINDOWED_MIN_HEIGHT.min(monitor_height.max(1));
    let max_width = (monitor_width - WINDOWED_MONITOR_MARGIN_X).max(width_floor);
    let max_height =
        (monitor_height - WINDOWED_MONITOR_MARGIN_Y - WINDOWED_VIEWPORT_HEIGHT_REDUCTION)
            .max(height_floor);
    (
        WINDOWED_WIDTH.min(max_width),
        (WINDOWED_HEIGHT - WINDOWED_VIEWPORT_HEIGHT_REDUCTION).min(max_height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_size_keeps_standard_size_on_large_monitors() {
        assert_eq!(capped_windowed_size(1920, 1080), (1280, 690));
    }

    #[test]
    fn windowed_size_caps_height_for_short_monitors() {
        assert_eq!(capped_windowed_size(900, 540), (852, 414));
    }

    #[test]
    fn windowed_size_does_not_exceed_tiny_monitors() {
        assert_eq!(capped_windowed_size(500, 320), (500, 320));
    }
}
