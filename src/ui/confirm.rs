use gtk::prelude::*;

pub fn confirm_remove(
    parent: &gtk::ApplicationWindow,
    title: &str,
    message: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = gtk::Dialog::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .build();
    dialog.add_css_class("confirm-dialog");
    dialog.set_default_size(360, -1);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let label = gtk::Label::new(Some(message));
    label.add_css_class("confirm-dialog-message");
    label.set_wrap(true);
    label.set_xalign(0.0);
    content.append(&label);

    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    let remove = dialog.add_button("Remove", gtk::ResponseType::Accept);
    remove.add_css_class("danger-button");
    dialog.set_default_response(gtk::ResponseType::Cancel);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            on_confirm();
        }
        dialog.close();
    });

    dialog.present();
}
