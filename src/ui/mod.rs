pub mod artist_selector;
pub mod blur_box;
pub mod confirm;
pub mod editor_panes;
pub mod ideas_workspace;
pub mod live_highlights;
pub mod main_window;
pub mod notifications;
pub mod raw_gutter;
pub mod row_icons;
pub mod slide_panel;
pub mod splash;
pub mod track_overlay;
pub mod window_policy;

use crate::app_logging;
use gtk::prelude::*;

pub(crate) const APP_ID: &str = "org.rmml.roper";
pub(crate) const APP_ICON_NAME: &str = APP_ID;

pub fn run() -> gtk::glib::ExitCode {
    if let Ok(log_path) = app_logging::init_logging() {
        app_logging::log_info(format!("log file: {}", log_path.display()));
    }
    install_log_filter();
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| {
        gtk::Window::set_default_icon_name(APP_ICON_NAME);
        load_css();
    });

    app.connect_activate(|app| {
        splash::show(app);
    });

    app.run()
}

fn install_log_filter() {
    gtk::glib::log_set_writer_func(|level, fields| {
        if is_ignored_log_message(fields) {
            gtk::glib::LogWriterOutput::Handled
        } else {
            if let Some(message) = extract_log_message(fields) {
                app_logging::log_info(format!("glib {level:?}: {message}"));
            }
            gtk::glib::log_writer_default(level, fields)
        }
    });
}

fn extract_log_message(fields: &[gtk::glib::LogField<'_>]) -> Option<String> {
    fields
        .iter()
        .find(|field| field.key() == "MESSAGE")
        .and_then(|field| field.value_str())
        .map(ToOwned::to_owned)
}

fn is_ignored_log_message(fields: &[gtk::glib::LogField<'_>]) -> bool {
    fields.iter().any(|field| {
        field.key() == "MESSAGE" && field.value_str().is_some_and(is_ignored_log_message_text)
    })
}

fn is_ignored_log_message_text(message: &str) -> bool {
    is_ignored_gtk_settings_warning(message) || is_ignored_gtk_active_state_warning(message)
}

fn is_ignored_gtk_settings_warning(message: &str) -> bool {
    message.contains("Unknown key gtk-modules") && message.contains("gtk-4.0/settings.ini")
}

fn is_ignored_gtk_active_state_warning(message: &str) -> bool {
    message.starts_with("Broken accounting of active state for widget ")
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("../resources/style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_filter_suppresses_known_gtk_active_state_warning() {
        assert!(is_ignored_log_message_text(
            "Broken accounting of active state for widget 0x123(GtkTextView)"
        ));
    }

    #[test]
    fn log_filter_keeps_unrelated_gtk_warnings_visible() {
        assert!(!is_ignored_log_message_text(
            "Failed to measure widget allocation"
        ));
    }

    #[test]
    fn log_filter_keeps_existing_settings_suppression() {
        assert!(is_ignored_log_message_text(
            "Unknown key gtk-modules in /home/user/.config/gtk-4.0/settings.ini"
        ));
    }
}
