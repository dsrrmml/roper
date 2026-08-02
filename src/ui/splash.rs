use crate::app_paths;
use crate::persistence::settings_store::{AppSettings, SettingsStore};
use crate::ui::{APP_ICON_NAME, main_window, window_policy};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static MAIN_WINDOW: RefCell<Option<gtk::ApplicationWindow>> = const { RefCell::new(None) };
}

const SPLASH_HOLD: Duration = Duration::from_secs(3);
const SPLASH_FADE: Duration = Duration::from_millis(700);

pub fn show(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("ROPER")
        .default_width(420)
        .default_height(220)
        .decorated(false)
        .build();
    window.set_icon_name(Some(APP_ICON_NAME));
    window.add_css_class("splash");
    window_policy::apply_fullscreen_policy(&window, preferred_fullscreen());
    MAIN_WINDOW.with(|slot| {
        *slot.borrow_mut() = Some(window.clone());
    });
    window.connect_close_request(|_| {
        MAIN_WINDOW.with(|slot| {
            *slot.borrow_mut() = None;
        });
        gtk::glib::Propagation::Proceed
    });

    let root = view();
    window.set_child(Some(&root));
    window.present();

    let app = app.clone();
    gtk::glib::timeout_add_local_once(SPLASH_HOLD, move || {
        fade_to_artist_selector(app, window, root);
    });
}

fn preferred_fullscreen() -> bool {
    SettingsStore::new_default()
        .and_then(|store| store.load())
        .map(|settings| settings.fullscreen)
        .unwrap_or_else(|_| AppSettings::default().fullscreen)
}

pub fn view() -> gtk::Overlay {
    let root = gtk::Overlay::new();
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("splash");

    let picture = gtk::Picture::for_filename(splash_path());
    picture.set_keep_aspect_ratio(false);
    picture.set_can_shrink(false);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    root.set_child(Some(&picture));

    let wave = wave_layer();
    root.add_overlay(&wave);
    root
}

pub fn splash_path() -> std::path::PathBuf {
    app_paths::splash_path()
}

fn wave_layer() -> gtk::DrawingArea {
    let wave = gtk::DrawingArea::new();
    wave.set_hexpand(true);
    wave.set_vexpand(true);
    wave.set_halign(gtk::Align::Fill);
    wave.set_valign(gtk::Align::Fill);

    let start = Rc::new(Instant::now());
    let draw_start = Rc::clone(&start);
    wave.set_draw_func(move |_, cr, width, height| {
        draw_wave(
            cr,
            width as f64,
            height as f64,
            draw_start.elapsed().as_secs_f64(),
        );
    });

    wave.add_tick_callback(move |area, _| {
        area.queue_draw();
        gtk::glib::ControlFlow::Continue
    });
    wave
}

fn draw_wave(cr: &gtk::cairo::Context, width: f64, height: f64, elapsed: f64) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
    cr.rectangle(0.0, 0.0, width, height);
    cr.fill().ok();

    let base_y = height * 0.54;
    let amplitude = (height * 0.055).clamp(26.0, 82.0);
    let wavelength = (width * 0.64).max(420.0);
    let speed = elapsed * 0.72;

    for band in 0..5 {
        let offset = band as f64 * 34.0;
        let alpha = 0.18 - band as f64 * 0.026;
        cr.set_source_rgba(1.0, 1.0, 1.0, alpha.max(0.055));
        cr.set_line_width(18.0 + band as f64 * 4.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);

        let mut x = -24.0;
        let first_y = wave_y(x, base_y + offset, amplitude, wavelength, speed, band);
        cr.move_to(x, first_y);
        while x <= width + 24.0 {
            let y = wave_y(x, base_y + offset, amplitude, wavelength, speed, band);
            cr.line_to(x, y);
            x += 18.0;
        }
        cr.stroke().ok();
    }
}

fn wave_y(x: f64, base_y: f64, amplitude: f64, wavelength: f64, speed: f64, band: i32) -> f64 {
    let phase = (x / wavelength * TAU) + speed + band as f64 * 0.7;
    base_y + phase.sin() * amplitude + (phase * 0.43).cos() * amplitude * 0.42
}

fn fade_to_artist_selector(
    app: gtk::Application,
    window: gtk::ApplicationWindow,
    root: gtk::Overlay,
) {
    let started = Instant::now();
    let finished = Rc::new(Cell::new(false));
    gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
        let progress = (started.elapsed().as_secs_f64() / SPLASH_FADE.as_secs_f64()).min(1.0);
        root.set_opacity(1.0 - progress);

        if progress >= 1.0 && !finished.replace(true) {
            let _ = app;
            main_window::show_in_window(&window, main_window::startup_artist());
            gtk::glib::ControlFlow::Break
        } else if progress >= 1.0 {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}
