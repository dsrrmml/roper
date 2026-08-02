use gtk::prelude::*;

pub fn show_error(label: &gtk::Label, message: impl AsRef<str>) {
    label.remove_css_class("notification-info");
    label.add_css_class("notification-error");
    label.set_label(message.as_ref());
    label.set_visible(true);
    let label = label.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(4200), move || {
        label.set_label("");
        label.set_visible(false);
    });
}

pub fn show_info(label: &gtk::Label, message: impl AsRef<str>) {
    label.remove_css_class("notification-error");
    label.add_css_class("notification-info");
    label.set_label(message.as_ref());
    label.set_visible(true);
    let label = label.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(2600), move || {
        label.set_label("");
        label.set_visible(false);
    });
}

pub fn clear(label: &gtk::Label) {
    label.remove_css_class("notification-error");
    label.remove_css_class("notification-info");
    label.set_label("");
    label.set_visible(false);
}
