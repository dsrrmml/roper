use crate::app_paths;
use gtk::prelude::*;

const ROW_ACTION_ICON_SIZE: i32 = 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowActionIcon {
    Edit,
    Remove,
}

pub fn icon(kind: RowActionIcon) -> gtk::Widget {
    let file_name = match kind {
        RowActionIcon::Edit => "edit.svg",
        RowActionIcon::Remove => "remove.svg",
    };
    let path = app_paths::icon_path(file_name);

    if path.exists() {
        let image = gtk::Image::from_file(path);
        image.add_css_class("row-action-icon");
        image.set_pixel_size(ROW_ACTION_ICON_SIZE);
        image.set_size_request(ROW_ACTION_ICON_SIZE, ROW_ACTION_ICON_SIZE);
        image.set_halign(gtk::Align::Center);
        image.set_valign(gtk::Align::Center);
        image.set_can_target(false);
        image.upcast()
    } else {
        let fallback = gtk::Label::new(Some("•"));
        fallback.add_css_class("row-action-icon");
        fallback.set_size_request(ROW_ACTION_ICON_SIZE, ROW_ACTION_ICON_SIZE);
        fallback.set_halign(gtk::Align::Center);
        fallback.set_valign(gtk::Align::Center);
        fallback.set_can_target(false);
        fallback.upcast()
    }
}
