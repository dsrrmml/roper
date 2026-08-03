use crate::app_paths;
use gtk::prelude::*;
use std::path::PathBuf;

const TAB_ACTION_WIDTH: i32 = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayTab {
    Artists,
    Tracks,
    Ideas,
    Settings,
    Info,
    Exit,
}

impl OverlayTab {
    fn page_name(self) -> &'static str {
        match self {
            Self::Artists => "artists",
            Self::Tracks => "tracks",
            Self::Ideas => "ideas",
            Self::Settings => "settings",
            Self::Info => "info",
            Self::Exit => "exit",
        }
    }
}

pub struct TrackOverlay {
    pub layer: gtk::Box,
    pub panel: gtk::Box,
    track_list: gtk::FlowBox,
    artist_list: gtk::ListBox,
    pub scrolled: gtk::ScrolledWindow,
    pub create_button: gtk::Button,
    pub create_artist_button: gtk::Button,
    pub artists_tab_button: gtk::Button,
    pub tracks_tab_button: gtk::Button,
    pub artists_tab_label: gtk::Label,
    pub tracks_tab_label: gtk::Label,
    pub ideas_tab_button: gtk::Button,
    pub settings_tab_button: gtk::Button,
    pub info_tab_button: gtk::Button,
    pub exit_tab_button: gtk::Button,
    tab_bar: gtk::Box,
    tab_action_box: gtk::Box,
    content_stack: gtk::Stack,
    pub settings_box: gtk::Box,
    pub ideas_box: gtk::Box,
    pub info_box: gtk::Box,
    pub exit_box: gtk::Box,
    pub edit_box: gtk::Box,
    on_visibility_change: std::rc::Rc<dyn Fn(bool)>,
}

impl TrackOverlay {
    pub fn new(
        on_close: impl Fn() + 'static,
        on_visibility_change: impl Fn(bool) + 'static,
    ) -> Self {
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
        let close = std::rc::Rc::new(on_close);
        let on_visibility_change = std::rc::Rc::new(on_visibility_change);
        let click = gtk::GestureClick::new();
        {
            let close = close.clone();
            click.connect_pressed(move |_, _, _, _| close());
        }
        backdrop.add_controller(click);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("track-panel");
        panel.set_vexpand(true);
        panel.set_halign(gtk::Align::End);

        let create_artist_button = icon_text_button("add.svg", "CREATE ARTIST");
        create_artist_button.add_css_class("primary-button");
        create_artist_button.add_css_class("tab-action-button");
        create_artist_button.set_size_request(TAB_ACTION_WIDTH, -1);
        create_artist_button.set_halign(gtk::Align::End);

        let create_button = icon_text_button("add.svg", "CREATE TRACK");
        create_button.add_css_class("primary-button");
        create_button.add_css_class("tab-action-button");
        create_button.set_size_request(TAB_ACTION_WIDTH, -1);
        create_button.set_halign(gtk::Align::End);

        let tab_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_bar.add_css_class("menu-tabs");
        let (artists_tab_button, artists_tab_label) = tab_button("artist.svg", "ARTISTS");
        let (tracks_tab_button, tracks_tab_label) = tab_button("lyrics.svg", "TRACKS");
        let (ideas_tab_button, _) = tab_button("lightbulb.svg", "IDEAS");
        let (settings_tab_button, _) = tab_button("tab-settings.svg", "SETTINGS");
        let (info_tab_button, _) = tab_button("info.svg", "INFO");
        let (exit_tab_button, _) = tab_button("exit.svg", "EXIT");
        let tab_action_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_action_spacer.set_hexpand(true);
        let tab_action_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_action_box.add_css_class("menu-tab-actions");
        tab_bar.append(&artists_tab_button);
        tab_bar.append(&tracks_tab_button);
        tab_bar.append(&ideas_tab_button);
        tab_bar.append(&settings_tab_button);
        tab_bar.append(&info_tab_button);
        tab_bar.append(&exit_tab_button);
        tab_bar.append(&tab_action_spacer);
        tab_action_box.append(&create_artist_button);
        tab_action_box.append(&create_button);
        tab_bar.append(&tab_action_box);
        panel.append(&tab_bar);

        let artists_page = menu_page();
        let artist_list = gtk::ListBox::new();
        artist_list.add_css_class("artist-list");
        artist_list.set_selection_mode(gtk::SelectionMode::None);
        artist_list.set_vexpand(true);
        artist_list.set_valign(gtk::Align::Fill);
        artist_list.set_hexpand(true);
        let artist_scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&artist_list)
            .build();
        artists_page.append(&artist_scrolled);

        let tracks_page = menu_page();
        let track_list = gtk::FlowBox::new();
        track_list.add_css_class("track-list");
        track_list.set_selection_mode(gtk::SelectionMode::None);
        track_list.set_homogeneous(false);
        track_list.set_max_children_per_line(1);
        track_list.set_min_children_per_line(1);
        track_list.set_row_spacing(2);
        track_list.set_column_spacing(0);
        track_list.set_vexpand(false);
        track_list.set_valign(gtk::Align::Start);
        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&track_list)
            .build();
        tracks_page.append(&scrolled);

        let settings_box = menu_page();
        let ideas_box = menu_page();
        let info_box = menu_page();
        let exit_box = menu_page();

        let content_stack = gtk::Stack::new();
        content_stack.set_hexpand(true);
        content_stack.set_vexpand(true);
        content_stack.add_named(&artists_page, Some(OverlayTab::Artists.page_name()));
        content_stack.add_named(&tracks_page, Some(OverlayTab::Tracks.page_name()));
        content_stack.add_named(&ideas_box, Some(OverlayTab::Ideas.page_name()));
        content_stack.add_named(&settings_box, Some(OverlayTab::Settings.page_name()));
        content_stack.add_named(&info_box, Some(OverlayTab::Info.page_name()));
        content_stack.add_named(&exit_box, Some(OverlayTab::Exit.page_name()));
        panel.append(&content_stack);

        let edit_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        edit_box.set_hexpand(true);
        edit_box.set_vexpand(true);
        edit_box.set_visible(false);
        panel.append(&edit_box);

        layer.append(&backdrop);
        layer.append(&panel);

        let overlay = Self {
            layer,
            panel,
            track_list,
            artist_list,
            scrolled,
            create_button,
            create_artist_button,
            artists_tab_button,
            tracks_tab_button,
            artists_tab_label,
            tracks_tab_label,
            ideas_tab_button,
            settings_tab_button,
            info_tab_button,
            exit_tab_button,
            tab_bar,
            tab_action_box,
            content_stack,
            settings_box,
            ideas_box,
            info_box,
            exit_box,
            edit_box,
            on_visibility_change,
        };
        overlay.select_tab(OverlayTab::Tracks);
        overlay
    }

    pub fn show(&self, viewport_width: i32) {
        self.panel
            .set_width_request(((viewport_width.max(640) as f64) * 0.60).round() as i32);
        (self.on_visibility_change)(true);
        self.layer.set_visible(true);
    }

    pub fn hide(&self) {
        self.edit_box.set_visible(false);
        self.tab_bar.set_visible(true);
        self.content_stack.set_visible(true);
        self.layer.set_visible(false);
        (self.on_visibility_change)(false);
    }

    pub fn is_visible(&self) -> bool {
        self.layer.is_visible()
    }

    pub fn clear_list(&self) {
        while let Some(child) = self.track_list.first_child() {
            self.track_list.remove(&child);
        }
    }

    pub fn append_track_row(&self, row: &impl IsA<gtk::Widget>) {
        self.track_list.insert(row, -1);
    }

    pub fn clear_artists(&self) {
        while let Some(child) = self.artist_list.first_child() {
            self.artist_list.remove(&child);
        }
    }

    pub fn append_artist_row(&self, row: &impl IsA<gtk::Widget>) {
        self.artist_list.append(row);
    }

    pub fn clear_settings(&self) {
        clear_box(&self.settings_box);
    }

    pub fn clear_ideas(&self) {
        clear_box(&self.ideas_box);
    }

    pub fn clear_info(&self) {
        clear_box(&self.info_box);
    }

    pub fn clear_exit(&self) {
        clear_box(&self.exit_box);
    }

    pub fn clear_edit(&self) {
        while let Some(child) = self.edit_box.first_child() {
            self.edit_box.remove(&child);
        }
        self.edit_box.set_visible(false);
        self.tab_bar.set_visible(true);
        self.content_stack.set_visible(true);
    }

    pub fn show_edit(&self, blur: bool) {
        self.tab_bar.set_visible(false);
        self.content_stack.set_visible(false);
        self.edit_box.set_visible(true);
        (self.on_visibility_change)(blur);
    }

    pub fn clear_tab_actions(&self) {
        while let Some(child) = self.tab_action_box.first_child() {
            self.tab_action_box.remove(&child);
        }
    }

    pub fn append_tab_action(&self, widget: &impl IsA<gtk::Widget>) {
        self.tab_action_box.append(widget);
    }

    pub fn show_default_tab_actions(&self, tab: OverlayTab) {
        self.clear_tab_actions();
        match tab {
            OverlayTab::Artists => self.append_tab_action(&self.create_artist_button),
            OverlayTab::Tracks => self.append_tab_action(&self.create_button),
            _ => {}
        }
    }

    pub fn select_tab(&self, tab: OverlayTab) {
        self.clear_edit();
        self.content_stack.set_visible_child_name(tab.page_name());
        self.show_default_tab_actions(tab);
        for (button, candidate) in [
            (&self.artists_tab_button, OverlayTab::Artists),
            (&self.tracks_tab_button, OverlayTab::Tracks),
            (&self.ideas_tab_button, OverlayTab::Ideas),
            (&self.settings_tab_button, OverlayTab::Settings),
            (&self.info_tab_button, OverlayTab::Info),
            (&self.exit_tab_button, OverlayTab::Exit),
        ] {
            if candidate == tab {
                button.add_css_class("menu-tab-active");
            } else {
                button.remove_css_class("menu-tab-active");
            }
        }
    }
}

fn menu_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page
}

fn tab_button(icon_name: &str, label: &str) -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("menu-tab-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.add_css_class("menu-tab-content");
    content.append(&icon_widget(icon_name, 18));
    let label_widget = gtk::Label::new(Some(label));
    label_widget.set_xalign(0.0);
    content.append(&label_widget);
    button.set_child(Some(&content));
    (button, label_widget)
}

fn clear_box(box_: &gtk::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}

fn icon_text_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("icon-text-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_size_request(TAB_ACTION_WIDTH - 20, -1);
    content.set_hexpand(true);
    content.set_halign(gtk::Align::Fill);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let label = gtk::Label::new(Some(label));
    label.set_xalign(1.0);
    label.set_hexpand(false);
    content.append(&spacer);
    content.append(&icon_widget(icon_name, 18));
    content.append(&label);
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
