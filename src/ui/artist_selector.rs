use crate::app_paths;
use crate::models::Artist;
use crate::persistence::artist_store::ArtistStore;
use crate::services::validation::{validate_artwork_path, validate_name};
use crate::ui::{
    confirm, main_window, notifications,
    row_icons::{self, RowActionIcon},
    slide_panel::SlidePanel,
    window_policy,
};
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

const LIST_IMAGE_COLUMN_WIDTH: i32 = 160;
const ROW_ACTION_BUTTON_SIZE: i32 = LIST_IMAGE_COLUMN_WIDTH / 2;

pub fn show_in_window(app: &gtk::Application, window: &gtk::ApplicationWindow) {
    window.set_title(Some("ROPER - Artists"));
    window.add_css_class("surface");
    window_policy::reassert_fullscreen(window);

    let root_overlay = gtk::Overlay::new();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
    root.add_css_class("artist-selector");
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(Some("Select Artist"));
    title.set_xalign(0.0);
    title.add_css_class("pane-title");
    title.set_hexpand(true);
    let create = icon_text_button("add.svg", "CREATE ARTIST");
    create.add_css_class("transparent-action-button");
    header.append(&title);
    header.append(&create);

    let list = gtk::ListBox::new();
    list.add_css_class("artist-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let scrolled = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&list)
        .build();

    let notice = gtk::Label::new(None);
    notice.add_css_class("notification");
    notice.set_wrap(true);
    notice.set_visible(false);

    root.append(&header);
    root.append(&scrolled);
    root.append(&notice);
    root_overlay.set_child(Some(&root));

    let panel = Rc::new(SlidePanel::new());
    root_overlay.add_overlay(&panel.layer);
    window.set_child(Some(&root_overlay));
    window.present();
    window_policy::reassert_fullscreen(window);

    let store = match ArtistStore::new_default() {
        Ok(store) => Rc::new(store),
        Err(err) => {
            notifications::show_error(&notice, err.to_string());
            return;
        }
    };

    reload_artists(&list, &notice, app, window, &store, &panel);

    {
        let store = store.clone();
        let list = list.clone();
        let notice = notice.clone();
        let app = app.clone();
        let window = window.clone();
        let panel = panel.clone();
        create.connect_clicked(move |_| {
            show_artist_form(&app, &window, &panel, &store, &list, &notice, None);
        });
    }

    {
        let panel = panel.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape && panel.is_visible() {
                panel.hide();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        root_overlay.add_controller(key);
    }
}

fn reload_artists(
    list: &gtk::ListBox,
    notice: &gtk::Label,
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    store: &Rc<ArtistStore>,
    panel: &Rc<SlidePanel>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    match store.load() {
        Ok(file) => {
            notifications::clear(notice);
            for artist in file.artists {
                list.append(&artist_row(app, window, artist, store, list, notice, panel));
            }
        }
        Err(err) => notifications::show_error(
            notice,
            format!(
                "Could not load artists from {}: {}",
                store.path().display(),
                err
            ),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn artist_row(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    artist: Artist,
    store: &Rc<ArtistStore>,
    list: &gtk::ListBox,
    notice: &gtk::Label,
    panel: &Rc<SlidePanel>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_size_request(-1, 160);
    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-row");
    shell.set_size_request(-1, 160);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
    labels.set_hexpand(true);
    labels.set_valign(gtk::Align::Start);
    let name = gtk::Label::new(Some(&artist.name));
    name.set_xalign(0.0);
    name.set_valign(gtk::Align::Start);
    name.add_css_class("artist-name");
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let description = gtk::Label::new(Some(if artist.description.trim().is_empty() {
        "No description"
    } else {
        artist.description.trim()
    }));
    description.add_css_class("muted");
    description.add_css_class("artist-info");
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_lines(3);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    labels.append(&name);
    labels.append(&description);

    let open_button = gtk::Button::new();
    open_button.add_css_class("row-open-button");
    open_button.set_child(Some(&labels));
    open_button.set_hexpand(true);
    open_button.set_vexpand(true);
    open_button.set_valign(gtk::Align::Fill);
    {
        let window = window.clone();
        let artist = artist.clone();
        open_button.connect_clicked(move |_| {
            main_window::show_in_window(&window, artist.clone());
        });
    }

    let edit = gtk::Button::new();
    edit.add_css_class("row-action-button");
    edit.add_css_class("row-edit-button");
    edit.set_size_request(ROW_ACTION_BUTTON_SIZE, ROW_ACTION_BUTTON_SIZE);
    edit.set_tooltip_text(Some("Edit artist"));
    let edit_icon = row_icons::icon(RowActionIcon::Edit);
    edit.set_child(Some(&edit_icon));
    {
        let app = app.clone();
        let window = window.clone();
        let panel = panel.clone();
        let store = store.clone();
        let list = list.clone();
        let notice = notice.clone();
        let artist = artist.clone();
        edit.connect_clicked(move |_| {
            show_artist_form(
                &app,
                &window,
                &panel,
                &store,
                &list,
                &notice,
                Some(artist.clone()),
            );
        });
    }
    let remove = gtk::Button::new();
    remove.add_css_class("row-action-button");
    remove.add_css_class("row-remove-button");
    remove.set_size_request(ROW_ACTION_BUTTON_SIZE, ROW_ACTION_BUTTON_SIZE);
    remove.set_tooltip_text(Some("Remove artist"));
    let remove_icon = row_icons::icon(RowActionIcon::Remove);
    remove.set_child(Some(&remove_icon));
    {
        let window = window.clone();
        let app = app.clone();
        let store = store.clone();
        let list = list.clone();
        let notice = notice.clone();
        let panel = panel.clone();
        let artist = artist.clone();
        remove.connect_clicked(move |_| {
            request_remove_artist(
                &app,
                &window,
                &store,
                &list,
                &notice,
                &panel,
                artist.clone(),
            );
        });
    }
    let actions = row_action_stack(edit, remove);
    let edit_revealer = gtk::Revealer::new();
    edit_revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
    edit_revealer.set_transition_duration(140);
    edit_revealer.set_reveal_child(false);
    edit_revealer.set_vexpand(true);
    edit_revealer.set_valign(gtk::Align::Fill);
    edit_revealer.set_child(Some(&actions));

    {
        let enter_revealer = edit_revealer.clone();
        let leave_revealer = edit_revealer.clone();
        let hover = gtk::EventControllerMotion::new();
        hover.connect_enter(move |_, _, _| {
            enter_revealer.set_reveal_child(true);
        });
        hover.connect_leave(move |_| {
            leave_revealer.set_reveal_child(false);
        });
        shell.add_controller(hover);
    }

    shell.append(&open_button);
    shell.append(&edit_revealer);
    shell.append(&artist_image_widget(&artist));
    row.set_child(Some(&shell));
    row
}

fn request_remove_artist(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    store: &Rc<ArtistStore>,
    list: &gtk::ListBox,
    notice: &gtk::Label,
    panel: &Rc<SlidePanel>,
    artist: Artist,
) {
    let message = format!("Remove artist \"{}\" from the catalog?", artist.name);
    let app = app.clone();
    let window_for_confirm = window.clone();
    let window = window.clone();
    let store = store.clone();
    let list = list.clone();
    let notice = notice.clone();
    let panel = panel.clone();
    confirm::confirm_remove(
        &window_for_confirm,
        "Remove Artist",
        &message,
        move || match store.remove_artist(&artist.id) {
            Ok(_) => {
                notifications::show_info(&notice, "Artist removed.");
                reload_artists(&list, &notice, &app, &window, &store, &panel);
            }
            Err(err) => notifications::show_error(&notice, err.to_string()),
        },
    );
}

fn row_action_stack(edit: gtk::Button, remove: gtk::Button) -> gtk::Box {
    let stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_css_class("row-action-stack");
    stack.set_size_request(ROW_ACTION_BUTTON_SIZE, LIST_IMAGE_COLUMN_WIDTH);
    stack.set_hexpand(false);
    stack.set_vexpand(true);
    stack.set_valign(gtk::Align::Fill);
    stack.append(&edit);
    stack.append(&remove);
    stack
}

#[allow(clippy::too_many_arguments)]
fn show_artist_form(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    panel: &Rc<SlidePanel>,
    store: &Rc<ArtistStore>,
    list: &gtk::ListBox,
    notice: &gtk::Label,
    artist: Option<Artist>,
) {
    let is_edit = artist.is_some();
    let form = gtk::Box::new(gtk::Orientation::Vertical, 10);
    form.add_css_class("editor-form");

    let name = gtk::Entry::builder()
        .placeholder_text("Artist name")
        .text(
            artist
                .as_ref()
                .map(|artist| artist.name.as_str())
                .unwrap_or(""),
        )
        .build();
    name.add_css_class("form-field");
    name.add_css_class("name-field");
    name.set_hexpand(true);
    name.set_halign(gtk::Align::Fill);

    let (description_frame, description) = description_field(
        artist
            .as_ref()
            .map(|artist| artist.description.as_str())
            .unwrap_or(""),
    );
    description_frame.set_hexpand(true);
    description_frame.set_halign(gtk::Align::Fill);

    let image_source: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let (image_picker, image_preview, image_placeholder) =
        artist_editor_image_picker(artist.as_ref().and_then(|artist| artist.image.as_ref()));

    let fields = gtk::Box::new(gtk::Orientation::Vertical, 10);
    fields.set_hexpand(true);
    fields.set_halign(gtk::Align::Fill);
    fields.append(&name);
    fields.append(&description_frame);

    let content_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content_row.set_hexpand(true);
    content_row.set_halign(gtk::Align::Fill);
    content_row.append(&fields);
    content_row.append(&image_picker);

    let error = gtk::Label::new(None);
    error.add_css_class("notification");
    error.set_wrap(true);
    error.set_visible(false);

    let bottom_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bottom_bar.set_hexpand(true);
    bottom_bar.set_vexpand(true);
    bottom_bar.set_halign(gtk::Align::Fill);
    bottom_bar.set_valign(gtk::Align::End);
    let bottom_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bottom_spacer.set_hexpand(true);
    let save = icon_button(
        "done.svg",
        if is_edit {
            "Save artist"
        } else {
            "Create artist"
        },
    );
    save.add_css_class("overlay-close-button");
    save.add_css_class("track-editor-done-button");
    save.set_size_request(48, 48);
    save.set_halign(gtk::Align::End);
    save.set_valign(gtk::Align::End);
    bottom_bar.append(&bottom_spacer);
    bottom_bar.append(&save);

    form.append(&content_row);
    form.append(&error);
    form.append(&bottom_bar);

    panel.set_content(
        if is_edit {
            "Edit Artist"
        } else {
            "Create Artist"
        },
        &form,
    );
    panel.show(window.width());

    let update_validity: Rc<dyn Fn()> = {
        let name = name.clone();
        let save = save.clone();
        Rc::new(move || {
            save.set_sensitive(validate_name(&name.text(), "artist.name").is_ok());
        })
    };
    update_validity();
    {
        let update = update_validity.clone();
        name.connect_changed(move |_| update());
    }

    {
        let window = window.clone();
        let image_preview = image_preview.clone();
        let image_placeholder = image_placeholder.clone();
        let image_source = image_source.clone();
        let error = error.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let chooser = gtk::FileChooserNative::new(
                Some("Choose artist image"),
                Some(&window),
                gtk::FileChooserAction::Open,
                Some("Choose"),
                Some("Cancel"),
            );
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            chooser.add_filter(&filter);
            let image_preview = image_preview.clone();
            let image_placeholder = image_placeholder.clone();
            let image_source = image_source.clone();
            let error = error.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        match validate_artwork_path(&path) {
                            Ok(()) => {
                                image_preview.set_from_file(Some(&path));
                                image_placeholder.set_visible(false);
                                *image_source.borrow_mut() = Some(path);
                                notifications::clear(&error);
                            }
                            Err(err) => notifications::show_error(&error, err.to_string()),
                        }
                    }
                }
                chooser.destroy();
            });
            chooser.show();
        });
        image_picker.add_controller(click);
    }

    {
        let app = app.clone();
        let window = window.clone();
        let panel = panel.clone();
        let store = store.clone();
        let list = list.clone();
        let notice = notice.clone();
        let artist = artist.clone();
        let name = name.clone();
        let description = description.clone();
        let image_source = image_source.clone();
        let error = error.clone();
        save.connect_clicked(move |_| {
            let buffer = description.buffer();
            let desc = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
            let result = if let Some(artist) = &artist {
                store.update_artist_with_image(
                    &artist.id,
                    &name.text(),
                    desc.as_str(),
                    image_source.borrow().clone(),
                )
            } else {
                store.create_artist_with_image(
                    &name.text(),
                    desc.as_str(),
                    image_source.borrow().clone(),
                )
            };

            match result {
                Ok(saved_artist) => {
                    panel.hide();
                    reload_artists(&list, &notice, &app, &window, &store, &panel);
                    if artist.is_none() {
                        main_window::show_in_window(&window, saved_artist);
                    }
                }
                Err(err) => notifications::show_error(&error, err.to_string()),
            }
        });
    }
}

fn artist_editor_image_picker(
    artist_image: Option<&PathBuf>,
) -> (gtk::Overlay, gtk::Image, gtk::Label) {
    let picker = gtk::Overlay::new();
    picker.add_css_class("artwork-picker");
    picker.set_size_request(100, 100);
    picker.set_vexpand(false);
    picker.set_hexpand(false);
    picker.set_halign(gtk::Align::End);
    picker.set_valign(gtk::Align::Start);
    picker.set_tooltip_text(Some("Choose artist image"));

    let picture = gtk::Image::new();
    picture.set_pixel_size(100);
    picture.set_size_request(100, 100);
    picture.set_halign(gtk::Align::Fill);
    picture.set_valign(gtk::Align::Fill);
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.add_css_class("artwork-thumb");
    picker.set_child(Some(&picture));

    let placeholder = gtk::Label::new(Some("Choose image"));
    placeholder.add_css_class("artwork-placeholder");
    placeholder.set_wrap(true);
    placeholder.set_justify(gtk::Justification::Center);
    placeholder.set_halign(gtk::Align::Center);
    placeholder.set_valign(gtk::Align::Center);
    placeholder.set_can_target(false);
    picker.add_overlay(&placeholder);

    if let Some(path) = artist_image {
        picture.set_from_file(Some(path));
        placeholder.set_visible(false);
    }

    (picker, picture, placeholder)
}

fn description_field(initial: &str) -> (gtk::Overlay, gtk::TextView) {
    let description = gtk::TextView::new();
    description.add_css_class("form-field");
    description.add_css_class("description-field");
    description.set_wrap_mode(gtk::WrapMode::WordChar);
    description.set_vexpand(true);
    description.buffer().set_text(initial);

    let scrolled = gtk::ScrolledWindow::builder()
        .min_content_height(120)
        .child(&description)
        .build();
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scrolled));

    let placeholder = gtk::Label::new(Some("Description"));
    placeholder.add_css_class("placeholder");
    placeholder.set_halign(gtk::Align::Start);
    placeholder.set_valign(gtk::Align::Start);
    placeholder.set_margin_start(12);
    placeholder.set_margin_top(10);
    placeholder.set_can_target(false);
    placeholder.set_visible(initial.trim().is_empty());
    overlay.add_overlay(&placeholder);

    {
        let placeholder = placeholder.clone();
        description.buffer().connect_changed(move |buffer| {
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
            placeholder.set_visible(text.trim().is_empty());
        });
    }

    (overlay, description)
}

fn artist_image_widget(artist: &Artist) -> gtk::Widget {
    if let Some(path) = &artist.image {
        let picture = gtk::Picture::for_file(&gtk::gio::File::for_path(path));
        picture.set_size_request(LIST_IMAGE_COLUMN_WIDTH, LIST_IMAGE_COLUMN_WIDTH);
        picture.set_halign(gtk::Align::End);
        picture.set_valign(gtk::Align::Center);
        picture.set_margin_end(0);
        picture.add_css_class("artist-preview");
        picture.upcast()
    } else {
        let placeholder = gtk::Label::new(Some("Artist"));
        placeholder.set_size_request(LIST_IMAGE_COLUMN_WIDTH, LIST_IMAGE_COLUMN_WIDTH);
        placeholder.set_halign(gtk::Align::End);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_margin_end(0);
        placeholder.add_css_class("image-placeholder");
        placeholder.upcast()
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&icon_widget(icon_name, 18)));
    button
}

fn icon_text_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("icon-text-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&icon_widget(icon_name, 18));
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
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

fn icon_path(icon_name: &str) -> PathBuf {
    app_paths::icon_path(icon_name)
}
