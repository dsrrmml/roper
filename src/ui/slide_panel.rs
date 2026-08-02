use crate::app_paths;
use gtk::prelude::*;

pub struct SlidePanel {
    pub layer: gtk::Box,
    panel: gtk::Box,
    title: gtk::Label,
    content: gtk::Box,
    close_button: gtk::Button,
}

impl Default for SlidePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SlidePanel {
    pub fn new() -> Self {
        let layer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        layer.add_css_class("overlay-backdrop");
        layer.set_hexpand(true);
        layer.set_vexpand(true);
        layer.set_halign(gtk::Align::Fill);
        layer.set_valign(gtk::Align::Fill);
        layer.set_visible(false);

        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
        panel.add_css_class("slide-panel");
        panel.set_vexpand(true);
        panel.set_halign(gtk::Align::End);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(None);
        title.add_css_class("pane-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let close_button = icon_button("close.svg", "Close");
        close_button.add_css_class("overlay-close-button");
        close_button.set_size_request(48, 48);
        header.append(&title);
        header.append(&close_button);
        panel.append(&header);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_vexpand(true);
        panel.append(&content);

        layer.append(&backdrop);
        layer.append(&panel);

        {
            let layer = layer.clone();
            close_button.connect_clicked(move |_| layer.set_visible(false));
        }
        {
            let layer = layer.clone();
            let click = gtk::GestureClick::new();
            click.connect_pressed(move |_, _, _, _| layer.set_visible(false));
            backdrop.add_controller(click);
        }

        Self {
            layer,
            panel,
            title,
            content,
            close_button,
        }
    }

    pub fn set_content(&self, title: &str, child: &impl IsA<gtk::Widget>) {
        self.title.set_label(title);
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        self.content.append(child);
    }

    pub fn show(&self, viewport_width: i32) {
        self.panel
            .set_width_request(((viewport_width.max(640) as f64) * 0.60).round() as i32);
        self.layer.set_visible(true);
    }

    pub fn hide(&self) {
        self.layer.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.layer.is_visible()
    }

    pub fn close_button(&self) -> gtk::Button {
        self.close_button.clone()
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&icon_widget(icon_name, 18)));
    button
}

fn icon_widget(icon_name: &str, size: i32) -> gtk::Widget {
    let size = size.max(1);
    let image = gtk::Image::from_file(icon_path(icon_name));
    image.set_pixel_size(size);
    image.set_size_request(size, size);
    image.set_halign(gtk::Align::Center);
    image.set_valign(gtk::Align::Center);
    image.set_can_target(false);
    image.upcast()
}

fn icon_path(icon_name: &str) -> std::path::PathBuf {
    app_paths::icon_path(icon_name)
}
