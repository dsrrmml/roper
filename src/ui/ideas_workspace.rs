use crate::app_paths;
use crate::error_handling::AppResult;
use crate::models::{Artist, CasingMode};
use crate::persistence::artist_store::ArtistStore;
use crate::persistence::idea_store::{IdeaPaths, IdeaSettings, IdeaSnapshot, IdeaStore};
use crate::persistence::track_store::{TrackPager, TrackStore};
use crate::services::casing::apply_casing;
use crate::services::search::{SearchMatch, SearchOptions, find_matches};
use crate::ui::notifications;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

const SIDE_PANEL_WIDTH: i32 = 360;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdeaPane {
    InOut,
    Verses,
    Hooks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdeaStructureKind {
    Intro,
    Verse,
    Hook,
    Bridge,
    Outro,
}

#[derive(Clone)]
struct IdeaRuntime {
    settings: IdeaSettings,
    paths: IdeaPaths,
}

type TransferCompleteHandler = Rc<RefCell<Option<Box<dyn Fn(&str)>>>>;

pub struct IdeasWorkspace {
    pub root: gtk::Overlay,
    pub leave_button: gtk::Button,
    pub manager_button: gtk::Button,
    manager_create_button: gtk::Button,
    pub notice: gtk::Label,
    in_out_buffer: gtk::TextBuffer,
    verses_buffer: gtk::TextBuffer,
    hooks_buffer: gtk::TextBuffer,
    in_out_view: gtk::TextView,
    verses_view: gtk::TextView,
    hooks_view: gtk::TextView,
    structure_tool_root: gtk::Box,
    structure_tool_intro: gtk::Button,
    structure_tool_verse: gtk::Button,
    structure_tool_hook: gtk::Button,
    structure_tool_bridge: gtk::Button,
    structure_tool_outro: gtk::Button,
    casing_button: gtk::Button,
    font_combo: gtk::ComboBoxText,
    line_bubble: gtk::Label,
    word_bubble: gtk::Label,
    char_bubble: gtk::Label,
    transfer_button: gtk::Button,
    idea_store: IdeaStore,
    track_store: TrackStore,
    state: Rc<RefCell<IdeasState>>,
    search_revealer: gtk::Revealer,
    search_entry: gtk::Entry,
    search_fuzzy: gtk::Button,
    search_fuzzy_active: Rc<Cell<bool>>,
    manager_revealer: gtk::Revealer,
    transfer_revealer: gtk::Revealer,
    manager_panel: gtk::Box,
    transfer_panel: gtk::Box,
    transfer_header_title: gtk::Label,
    transfer_header_back_button: gtk::Button,
    manager_list: gtk::ListBox,
    transfer_content: gtk::Box,
    transfer_complete_handler: TransferCompleteHandler,
}

struct IdeasState {
    current: Option<IdeaRuntime>,
    casing_mode: CasingMode,
    programmatic_change: bool,
    last_focus: IdeaPane,
    search_cursor: usize,
    search_flat: Vec<(IdeaPane, SearchMatch)>,
    is_stored_as_idea: bool,
    is_dirty: bool,
}

impl IdeasWorkspace {
    pub fn new(default_font_size: u16, default_casing: CasingMode) -> AppResult<Self> {
        let idea_store = IdeaStore::new_default()?;
        let track_store = TrackStore::new_default()?;

        let root = gtk::Overlay::new();
        root.set_hexpand(true);
        root.set_vexpand(true);

        let in_out_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        let verses_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        let hooks_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        in_out_buffer.set_enable_undo(true);
        verses_buffer.set_enable_undo(true);
        hooks_buffer.set_enable_undo(true);

        let in_out_view = idea_text_view(&in_out_buffer);
        let verses_view = idea_text_view(&verses_buffer);
        let hooks_view = idea_text_view(&hooks_buffer);

        let (
            structure_tool_root,
            structure_tool_intro,
            structure_tool_verse,
            structure_tool_hook,
            structure_tool_bridge,
            structure_tool_outro,
        ) = ideas_structure_tool_widgets();

        let casing_button = gtk::Button::with_label(default_casing.label());
        casing_button.add_css_class("floating-button");
        casing_button.add_css_class("toolbar-control");
        casing_button.set_size_request(36, 36);

        let font_combo = gtk::ComboBoxText::new();
        for size in [10_u16, 12, 14, 16, 18] {
            font_combo.append_text(&size.to_string());
        }
        let active_index = [10_u16, 12, 14, 16, 18]
            .iter()
            .position(|size| *size == default_font_size)
            .unwrap_or(3);
        font_combo.set_active(Some(active_index as u32));
        font_combo.add_css_class("font-size-combo");
        font_combo.set_size_request(-1, 36);
        font_combo.set_valign(gtk::Align::Fill);

        let line_bubble = stat_bubble("L 0");
        let word_bubble = stat_bubble("W 0");
        let char_bubble = stat_bubble("C 0");
        line_bubble.set_size_request(-1, 36);
        line_bubble.set_valign(gtk::Align::Fill);
        word_bubble.set_size_request(-1, 36);
        word_bubble.set_valign(gtk::Align::Fill);
        char_bubble.set_size_request(-1, 36);
        char_bubble.set_valign(gtk::Align::Fill);

        let transfer_button = icon_text_button("transfer.svg", "TRANSFER");
        transfer_button.add_css_class("secondary-button");
        transfer_button.add_css_class("ideas-toolbar-button");
        transfer_button.add_css_class("ideas-toolbar-right-button");
        transfer_button.add_css_class("toolbar-control");
        transfer_button.set_size_request(-1, 36);

        let leave_button = icon_button("menu.svg", "OPEN MENU");
        leave_button.add_css_class("secondary-button");
        leave_button.add_css_class("ideas-toolbar-button");
        leave_button.add_css_class("ideas-toolbar-right-button");
        leave_button.add_css_class("toolbar-control");
        leave_button.add_css_class("back-action-button");
        leave_button.set_size_request(36, 36);

        let manager_button = icon_text_button("batch-prediction.svg", "MANAGE");
        manager_button.add_css_class("secondary-button");
        manager_button.add_css_class("ideas-toolbar-button");
        manager_button.add_css_class("ideas-toolbar-right-button");
        manager_button.add_css_class("toolbar-control");
        manager_button.set_size_request(-1, 36);

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        toolbar.add_css_class("ideas-toolbar");
        toolbar.set_hexpand(true);
        toolbar.set_height_request(36);
        toolbar.set_valign(gtk::Align::End);
        toolbar.append(&casing_button);
        toolbar.append(&font_combo);
        toolbar.append(&line_bubble);
        toolbar.append(&word_bubble);
        toolbar.append(&char_bubble);
        toolbar.append(&structure_tool_root);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        toolbar.append(&spacer);
        toolbar.append(&transfer_button);
        toolbar.append(&manager_button);
        toolbar.append(&leave_button);

        let search_revealer = gtk::Revealer::new();
        search_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        search_revealer.set_transition_duration(160);
        search_revealer.set_reveal_child(false);
        let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        search_row.add_css_class("search-panel");
        let search_push = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        search_push.set_hexpand(true);
        let search_entry = gtk::Entry::builder()
            .placeholder_text("Search ideas")
            .build();
        search_entry.add_css_class("search-field");
        search_entry.set_size_request(220, -1);
        let search_fuzzy = gtk::Button::with_label("F");
        search_fuzzy.add_css_class("search-fuzzy-button");
        search_row.append(&search_push);
        search_row.append(&search_entry);
        search_row.append(&search_fuzzy);
        search_revealer.set_child(Some(&search_row));

        let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        panes.set_hexpand(true);
        panes.set_vexpand(true);
        panes.append(&idea_pane_shell("IN/OUT", &in_out_view));
        panes.append(&idea_pane_shell("VERSES", &verses_view));
        panes.append(&idea_pane_shell("HOOKS/BRIDGES", &hooks_view));

        let notice = gtk::Label::new(None);
        notice.add_css_class("notification");
        notice.set_wrap(true);
        notice.set_visible(false);

        let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layout.set_hexpand(true);
        layout.set_vexpand(true);
        layout.append(&search_revealer);
        layout.append(&panes);
        layout.append(&notice);
        layout.append(&toolbar);
        root.set_child(Some(&layout));

        let manager_revealer = gtk::Revealer::new();
        manager_revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        manager_revealer.set_transition_duration(180);
        manager_revealer.set_halign(gtk::Align::End);
        manager_revealer.set_valign(gtk::Align::Fill);
        manager_revealer.set_reveal_child(false);
        let manager_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        manager_panel.add_css_class("slide-panel");
        manager_panel.add_css_class("ideas-manager-panel");
        manager_panel.set_size_request(SIDE_PANEL_WIDTH, -1);
        let manager_header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        manager_header.add_css_class("ideas-manager-header");
        manager_header.set_height_request(36);
        let manager_title = gtk::Label::new(Some("Ideas manager"));
        manager_title.add_css_class("ideas-manager-title");
        manager_title.set_xalign(0.0);
        manager_title.set_hexpand(true);
        let manager_create_button = icon_text_button("add.svg", "CREATE NEW IDEA");
        manager_create_button.set_size_request(-1, 36);
        manager_header.append(&manager_title);
        manager_header.append(&manager_create_button);
        manager_panel.append(&manager_header);
        let manager_list = gtk::ListBox::new();
        manager_list.add_css_class("artist-list");
        manager_list.set_selection_mode(gtk::SelectionMode::None);
        let manager_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&manager_list)
            .build();
        manager_panel.append(&manager_scroll);
        let manager_close_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        manager_close_row.set_height_request(36);
        manager_close_row.set_hexpand(true);
        let manager_close_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        manager_close_spacer.set_hexpand(true);
        let manager_close = icon_button("close.svg", "Close ideas manager");

        manager_close.add_css_class("secondary-button");
        manager_close.add_css_class("ideas-toolbar-button");
        manager_close.add_css_class("ideas-toolbar-right-button");
        manager_close.add_css_class("toolbar-control");
        manager_close.add_css_class("back-action-button");

        manager_close.set_size_request(36, 36);
        manager_close_row.append(&manager_close_spacer);
        manager_close_row.append(&manager_close);
        manager_panel.append(&manager_close_row);
        manager_revealer.set_child(Some(&manager_panel));
        root.add_overlay(&manager_revealer);

        let transfer_revealer = gtk::Revealer::new();
        transfer_revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        transfer_revealer.set_transition_duration(180);
        transfer_revealer.set_halign(gtk::Align::End);
        transfer_revealer.set_valign(gtk::Align::Fill);
        transfer_revealer.set_reveal_child(false);
        let transfer_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        transfer_panel.add_css_class("slide-panel");
        transfer_panel.add_css_class("ideas-transfer-panel");
        transfer_panel.set_size_request(SIDE_PANEL_WIDTH, -1);
        let transfer_header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        transfer_header.add_css_class("ideas-transfer-header");
        transfer_header.set_height_request(36);
        let transfer_title = gtk::Label::new(Some("Transfer to track"));
        transfer_title.add_css_class("pane-title");
        transfer_title.add_css_class("ideas-transfer-header-title");
        transfer_title.set_xalign(0.0);
        transfer_title.set_hexpand(true);
        let transfer_header_back_button = icon_text_button("close.svg", "BACK TO ARTISTS");
        transfer_header_back_button.set_size_request(-1, 20);
        transfer_header_back_button.set_valign(gtk::Align::Center);
        transfer_header_back_button.set_focus_on_click(false);
        transfer_header_back_button.set_focusable(false);
        transfer_header_back_button.set_visible(false);
        transfer_header.append(&transfer_title);
        transfer_header.append(&transfer_header_back_button);
        transfer_panel.append(&transfer_header);
        let transfer_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        transfer_content.add_css_class("ideas-transfer-content");
        transfer_content.set_hexpand(true);
        transfer_content.set_vexpand(true);
        transfer_panel.append(&transfer_content);
        let transfer_complete_handler: TransferCompleteHandler = Rc::new(RefCell::new(None));
        let transfer_close_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        transfer_close_row.add_css_class("ideas-transfer-close-row");
        transfer_close_row.set_height_request(36);
        let transfer_close_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        transfer_close_spacer.set_hexpand(true);
        let transfer_close = icon_button("close.svg", "Close transfer");
        transfer_close.set_size_request(-1, 36);
        transfer_close_row.append(&transfer_close_spacer);
        transfer_close_row.append(&transfer_close);
        transfer_panel.append(&transfer_close_row);
        transfer_revealer.set_child(Some(&transfer_panel));
        root.add_overlay(&transfer_revealer);

        let state = Rc::new(RefCell::new(IdeasState {
            current: None,
            casing_mode: default_casing,
            programmatic_change: false,
            last_focus: IdeaPane::Verses,
            search_cursor: 0,
            search_flat: Vec::new(),
            is_stored_as_idea: false,
            is_dirty: false,
        }));

        let workspace = Self {
            root,
            leave_button,
            manager_button,
            manager_create_button,
            notice,
            in_out_buffer,
            verses_buffer,
            hooks_buffer,
            in_out_view,
            verses_view,
            hooks_view,
            structure_tool_root,
            structure_tool_intro,
            structure_tool_verse,
            structure_tool_hook,
            structure_tool_bridge,
            structure_tool_outro,
            casing_button,
            font_combo,
            line_bubble,
            word_bubble,
            char_bubble,
            transfer_button,
            idea_store,
            track_store,
            state,
            search_revealer,
            search_entry,
            search_fuzzy,
            search_fuzzy_active: Rc::new(Cell::new(false)),
            manager_revealer,
            transfer_revealer,
            manager_panel,
            transfer_panel,
            transfer_header_title: transfer_title,
            transfer_header_back_button,
            manager_list,
            transfer_content,
            transfer_complete_handler,
        };

        workspace.install_handlers();
        workspace.install_font_size(default_font_size);
        workspace.open_latest_or_create();
        workspace.refresh_stats();
        workspace.refresh_name_glow();
        workspace.refresh_manager();
        workspace.show_artist_picker_for_transfer();
        Ok(workspace)
    }

    pub fn focus_verses(&self) {
        let verses_view = self.verses_view.clone();
        gtk::glib::idle_add_local_once(move || {
            verses_view.grab_focus();
        });
    }

    pub fn set_default_casing(&self, mode: CasingMode) {
        self.state.borrow_mut().casing_mode = mode;
        self.update_casing_button();
    }

    pub fn clear_current_idea(&self) {
        self.save_if_dirty();
        {
            let mut state = self.state.borrow_mut();
            state.programmatic_change = true;
            state.current = None;
            state.is_dirty = false;
            state.is_stored_as_idea = false;
        }
        self.in_out_buffer.set_text("");
        self.verses_buffer.set_text("");
        self.hooks_buffer.set_text("");
        {
            let mut state = self.state.borrow_mut();
            state.programmatic_change = false;
        }
        self.refresh_stats();
        self.update_ideas_structure_tool();
        self.refresh_name_glow();
        self.refresh_manager();
    }

    pub fn restore_latest_idea(&self) {
        let snapshot = self.idea_store.latest_idea().ok().flatten();
        if let Some(snapshot) = snapshot {
            self.load_snapshot(snapshot);
        } else {
            self.clear_current_idea();
        }
    }

    pub fn set_font_size(&self, font_size: u16) {
        self.install_font_size(font_size);
        let index = [10_u16, 12, 14, 16, 18]
            .iter()
            .position(|value| *value == font_size)
            .unwrap_or(3);
        self.font_combo.set_active(Some(index as u32));
    }

    pub fn current_font_size(&self) -> Option<u16> {
        let text = self.font_combo.active_text()?;
        text.parse::<u16>().ok()
    }

    fn install_handlers(&self) {
        self.update_casing_button();

        {
            let this = self.clone_handles();
            self.manager_button.connect_clicked(move |_| {
                this.save_if_dirty();
                this.transfer_revealer.set_reveal_child(false);
                this.adjust_slide_panel_widths();
                this.refresh_manager();
                this.manager_revealer.set_reveal_child(true);
            });
        }

        {
            let this = self.clone_handles();
            self.manager_create_button.connect_clicked(move |_| {
                this.create_new_idea();
            });
        }

        {
            let manager_revealer = self.manager_revealer.clone();
            if let Some(panel) = self.manager_revealer.child() {
                if let Some(close_row) = panel.last_child() {
                    if let Some(close_button) = close_row
                        .last_child()
                        .and_then(|w| w.downcast::<gtk::Button>().ok())
                    {
                        close_button.connect_clicked(move |_| {
                            manager_revealer.set_reveal_child(false);
                        });
                    }
                }
            }
        }

        {
            let transfer_revealer = self.transfer_revealer.clone();
            if let Some(panel) = self.transfer_revealer.child() {
                if let Some(close_row) = panel.last_child() {
                    if let Some(close_button) = close_row
                        .last_child()
                        .and_then(|w| w.downcast::<gtk::Button>().ok())
                    {
                        close_button.connect_clicked(move |_| {
                            transfer_revealer.set_reveal_child(false);
                        });
                    }
                }
            }
        }

        {
            let this = self.clone_handles();
            self.transfer_header_back_button.connect_clicked(move |_| {
                this.show_artist_picker_for_transfer();
            });
        }

        {
            let this = self.clone_handles();
            self.transfer_button.connect_clicked(move |_| {
                this.save_if_dirty();
                this.manager_revealer.set_reveal_child(false);
                this.adjust_slide_panel_widths();
                this.show_artist_picker_for_transfer();
                this.transfer_revealer.set_reveal_child(true);
            });
        }

        {
            let this = self.clone_handles();
            self.structure_tool_intro.connect_clicked(move |_| {
                this.insert_structure_tag(IdeaStructureKind::Intro);
            });
        }

        {
            let this = self.clone_handles();
            self.structure_tool_verse.connect_clicked(move |_| {
                this.insert_structure_tag(IdeaStructureKind::Verse);
            });
        }

        {
            let this = self.clone_handles();
            self.structure_tool_hook.connect_clicked(move |_| {
                this.insert_structure_tag(IdeaStructureKind::Hook);
            });
        }

        {
            let this = self.clone_handles();
            self.structure_tool_bridge.connect_clicked(move |_| {
                this.insert_structure_tag(IdeaStructureKind::Bridge);
            });
        }

        {
            let this = self.clone_handles();
            self.structure_tool_outro.connect_clicked(move |_| {
                this.insert_structure_tag(IdeaStructureKind::Outro);
            });
        }

        {
            let this = self.clone_handles();
            self.casing_button.connect_clicked(move |_| {
                this.cycle_casing();
            });
        }

        {
            let this = self.clone_handles();
            self.font_combo.connect_changed(move |_| {
                if let Some(size) = this.current_font_size() {
                    this.install_font_size(size);
                }
            });
        }

        for view in [&self.in_out_view, &self.verses_view, &self.hooks_view] {
            view.connect_paste_clipboard(move |view| {
                view.stop_signal_emission_by_name("paste-clipboard");
                paste_plain_text_with_trailing_newline(view);
            });
        }

        for buffer in [&self.in_out_buffer, &self.verses_buffer, &self.hooks_buffer] {
            let this = self.clone_handles();
            buffer.connect_changed(move |_| {
                this.on_text_changed();
                this.update_ideas_structure_tool();
            });
        }

        {
            let state = self.state.clone();
            let this = self.clone_handles();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(move |_| {
                {
                    let mut state = state.borrow_mut();
                    state.last_focus = IdeaPane::InOut;
                }
                this.update_ideas_structure_tool();
            });
            self.in_out_view.add_controller(focus);
        }
        {
            let state = self.state.clone();
            let this = self.clone_handles();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(move |_| {
                {
                    let mut state = state.borrow_mut();
                    state.last_focus = IdeaPane::Verses;
                }
                this.update_ideas_structure_tool();
            });
            self.verses_view.add_controller(focus);
        }
        {
            let state = self.state.clone();
            let this = self.clone_handles();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(move |_| {
                {
                    let mut state = state.borrow_mut();
                    state.last_focus = IdeaPane::Hooks;
                }
                this.update_ideas_structure_tool();
            });
            self.hooks_view.add_controller(focus);
        }

        self.attach_focus_style(&self.in_out_view);
        self.attach_focus_style(&self.verses_view);
        self.attach_focus_style(&self.hooks_view);

        {
            let this = self.clone_handles();
            self.search_entry.connect_changed(move |_| {
                this.refresh_search_matches();
            });
        }
        {
            let this = self.clone_handles();
            self.search_entry.connect_activate(move |_| {
                this.advance_search(1);
            });
        }
        {
            let this = self.clone_handles();
            self.search_fuzzy.connect_clicked(move |_| {
                let active = !this.search_fuzzy_active.get();
                this.search_fuzzy_active.set(active);
                if active {
                    this.search_fuzzy.add_css_class("active");
                } else {
                    this.search_fuzzy.remove_css_class("active");
                }
                this.refresh_search_matches();
            });
        }

        {
            let this = self.clone_handles();
            let key = gtk::EventControllerKey::new();
            key.set_propagation_phase(gtk::PropagationPhase::Capture);
            key.connect_key_pressed(move |_, keyval, _, modifiers| {
                let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                if ctrl && keyval == gtk::gdk::Key::f {
                    this.search_revealer.set_reveal_child(true);
                    this.search_entry.grab_focus();
                    return gtk::glib::Propagation::Stop;
                }
                if keyval == gtk::gdk::Key::Escape {
                    if this.search_revealer.reveals_child() {
                        this.search_revealer.set_reveal_child(false);
                        this.clear_search_highlights();
                        return gtk::glib::Propagation::Stop;
                    }
                    if this.transfer_revealer.reveals_child() {
                        this.transfer_revealer.set_reveal_child(false);
                        return gtk::glib::Propagation::Stop;
                    }
                    if this.manager_revealer.reveals_child() {
                        this.manager_revealer.set_reveal_child(false);
                        return gtk::glib::Propagation::Stop;
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            self.root.add_controller(key);
        }
    }

    fn attach_focus_style(&self, view: &gtk::TextView) {
        let v_enter = view.clone();
        let v_leave = view.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            v_enter.add_css_class("idea-pane-focused");
        });
        focus.connect_leave(move |_| {
            v_leave.remove_css_class("idea-pane-focused");
        });
        view.add_controller(focus);
    }

    fn clone_handles(&self) -> Self {
        Self {
            root: self.root.clone(),
            leave_button: self.leave_button.clone(),
            manager_button: self.manager_button.clone(),
            manager_create_button: self.manager_create_button.clone(),
            notice: self.notice.clone(),
            in_out_buffer: self.in_out_buffer.clone(),
            verses_buffer: self.verses_buffer.clone(),
            hooks_buffer: self.hooks_buffer.clone(),
            in_out_view: self.in_out_view.clone(),
            verses_view: self.verses_view.clone(),
            hooks_view: self.hooks_view.clone(),
            structure_tool_root: self.structure_tool_root.clone(),
            structure_tool_intro: self.structure_tool_intro.clone(),
            structure_tool_verse: self.structure_tool_verse.clone(),
            structure_tool_hook: self.structure_tool_hook.clone(),
            structure_tool_bridge: self.structure_tool_bridge.clone(),
            structure_tool_outro: self.structure_tool_outro.clone(),
            casing_button: self.casing_button.clone(),
            font_combo: self.font_combo.clone(),
            line_bubble: self.line_bubble.clone(),
            word_bubble: self.word_bubble.clone(),
            char_bubble: self.char_bubble.clone(),
            transfer_button: self.transfer_button.clone(),
            idea_store: self.idea_store.clone(),
            track_store: self.track_store.clone(),
            state: self.state.clone(),
            search_revealer: self.search_revealer.clone(),
            search_entry: self.search_entry.clone(),
            search_fuzzy: self.search_fuzzy.clone(),
            search_fuzzy_active: self.search_fuzzy_active.clone(),
            manager_revealer: self.manager_revealer.clone(),
            transfer_revealer: self.transfer_revealer.clone(),
            manager_panel: self.manager_panel.clone(),
            transfer_panel: self.transfer_panel.clone(),
            transfer_header_title: self.transfer_header_title.clone(),
            transfer_header_back_button: self.transfer_header_back_button.clone(),
            manager_list: self.manager_list.clone(),
            transfer_content: self.transfer_content.clone(),
            transfer_complete_handler: self.transfer_complete_handler.clone(),
        }
    }

    pub fn set_transfer_complete_handler<F: Fn(&str) + 'static>(&self, handler: F) {
        *self.transfer_complete_handler.borrow_mut() = Some(Box::new(handler));
    }

    fn adjust_slide_panel_widths(&self) {
        let hooks_width = self
            .hooks_view
            .width()
            .max((self.root.width() / 3).max(280));
        self.manager_panel.set_width_request(hooks_width);
        self.transfer_panel.set_width_request(hooks_width);
    }

    fn open_latest_or_create(&self) {
        let snapshot = self
            .idea_store
            .latest_idea()
            .ok()
            .flatten()
            .or_else(|| self.idea_store.create_idea(None).ok());
        if let Some(snapshot) = snapshot {
            self.load_snapshot(snapshot);
        }
    }

    fn create_new_idea(&self) {
        self.save_if_dirty();
        match self.idea_store.create_idea(None) {
            Ok(snapshot) => {
                self.load_snapshot(snapshot);
                self.refresh_manager();
                self.verses_view.grab_focus();
            }
            Err(err) => notifications::show_error(&self.notice, err.to_string()),
        }
    }

    fn load_snapshot(&self, snapshot: IdeaSnapshot) {
        {
            let mut state = self.state.borrow_mut();
            state.programmatic_change = true;
            state.is_stored_as_idea = !snapshot.settings.name.trim().is_empty();
            state.current = Some(IdeaRuntime {
                settings: snapshot.settings.clone(),
                paths: snapshot.paths.clone(),
            });
        }
        self.in_out_buffer.set_text(&snapshot.in_out);
        self.verses_buffer.set_text(&snapshot.verses);
        self.hooks_buffer.set_text(&snapshot.hooks_bridges);
        {
            let mut state = self.state.borrow_mut();
            state.programmatic_change = false;
            state.is_dirty = false;
        }
        self.refresh_stats();
        self.update_ideas_structure_tool();
    }

    fn on_text_changed(&self) {
        if self.state.borrow().programmatic_change {
            return;
        }
        self.state.borrow_mut().is_dirty = true;
        self.refresh_stats();
        self.save_now();
        self.refresh_search_matches();
    }

    fn save_if_dirty(&self) {
        if self.state.borrow().is_dirty {
            self.save_now();
        }
    }

    fn save_now(&self) {
        let (paths, mut settings, was_stored_as_idea) = {
            let state = self.state.borrow();
            let Some(current) = state.current.as_ref() else {
                return;
            };
            (
                current.paths.clone(),
                current.settings.clone(),
                state.is_stored_as_idea,
            )
        };

        let in_out = buffer_text(&self.in_out_buffer);
        let verses = buffer_text(&self.verses_buffer);
        let hooks = buffer_text(&self.hooks_buffer);

        let result = self.idea_store.save_snapshot(
            &paths,
            &mut settings,
            &in_out,
            &verses,
            &hooks,
        );

        match result {
            Ok(()) => {
                let is_stored_as_idea = !settings.name.trim().is_empty();
                {
                    let mut state = self.state.borrow_mut();
                    if let Some(current) = state.current.as_mut() {
                        current.settings = settings;
                    }
                    state.is_dirty = false;
                    state.is_stored_as_idea = is_stored_as_idea;
                }
                notifications::clear(&self.notice);
                if self.manager_revealer.reveals_child()
                    || was_stored_as_idea != is_stored_as_idea
                {
                    self.refresh_manager();
                }
                self.refresh_name_glow();
            }
            Err(err) => {
                notifications::show_error(&self.notice, err.to_string());
                self.refresh_name_glow();
            }
        }
    }

    fn refresh_stats(&self) {
        let in_out = buffer_text(&self.in_out_buffer);
        let verses = buffer_text(&self.verses_buffer);
        let hooks = buffer_text(&self.hooks_buffer);
        let texts = [&in_out, &verses, &hooks];
        let total_lines = texts
            .iter()
            .map(|text| idea_line_count(text))
            .sum::<usize>();
        let total_words = texts
            .iter()
            .map(|text| idea_word_count(text))
            .sum::<usize>();
        let total_chars = texts.iter().map(|text| text.chars().count()).sum::<usize>();
        self.line_bubble.set_label(&format!("L {}", total_lines));
        self.word_bubble.set_label(&format!("W {}", total_words));
        self.char_bubble.set_label(&format!("C {}", total_chars));
    }

    fn refresh_name_glow(&self) {
        let should_glow = {
            let state = self.state.borrow();
            state.is_dirty || !state.is_stored_as_idea
        };

        if should_glow {
            self.notice.add_css_class("idea-name-glow");
        } else {
            self.notice.remove_css_class("idea-name-glow");
        }
    }

    fn current_ideas_pane(&self) -> IdeaPane {
        if self.in_out_view.has_focus() {
            IdeaPane::InOut
        } else if self.verses_view.has_focus() {
            IdeaPane::Verses
        } else if self.hooks_view.has_focus() {
            IdeaPane::Hooks
        } else {
            self.state.borrow().last_focus
        }
    }

    fn update_ideas_structure_tool(&self) {
        let focus = self.current_ideas_pane();
        let active_view_text = match focus {
            IdeaPane::InOut => buffer_text(&self.in_out_buffer),
            IdeaPane::Verses => buffer_text(&self.verses_buffer),
            IdeaPane::Hooks => buffer_text(&self.hooks_buffer),
        };

        let has_intro_tag =
            self.idea_contains_structure_tag(&active_view_text, IdeaStructureKind::Intro);
        let has_outro_tag =
            self.idea_contains_structure_tag(&active_view_text, IdeaStructureKind::Outro);

        self.structure_tool_intro
            .set_visible(matches!(focus, IdeaPane::InOut) && !has_intro_tag);
        self.structure_tool_outro
            .set_visible(matches!(focus, IdeaPane::InOut) && !has_outro_tag);
        self.structure_tool_verse
            .set_visible(matches!(focus, IdeaPane::Verses));
        self.structure_tool_hook
            .set_visible(matches!(focus, IdeaPane::Hooks));
        self.structure_tool_bridge
            .set_visible(matches!(focus, IdeaPane::Hooks));

        self.structure_tool_intro.set_label(
            &self.idea_structure_tool_button_label(&active_view_text, IdeaStructureKind::Intro),
        );
        self.structure_tool_outro.set_label(
            &self.idea_structure_tool_button_label(&active_view_text, IdeaStructureKind::Outro),
        );
        self.structure_tool_verse.set_label(
            &self.idea_structure_tool_button_label(&active_view_text, IdeaStructureKind::Verse),
        );
        self.structure_tool_hook.set_label(
            &self.idea_structure_tool_button_label(&active_view_text, IdeaStructureKind::Hook),
        );
        self.structure_tool_bridge.set_label(
            &self.idea_structure_tool_button_label(&active_view_text, IdeaStructureKind::Bridge),
        );
    }

    fn idea_contains_structure_tag(&self, text: &str, kind: IdeaStructureKind) -> bool {
        let normalized = normalized_structure_label(text);
        match kind {
            IdeaStructureKind::Intro => normalized.contains("[intro]"),
            IdeaStructureKind::Outro => normalized.contains("[outro]"),
            _ => false,
        }
    }

    fn insert_structure_tag(&self, kind: IdeaStructureKind) {
        let focus = self.state.borrow().last_focus;
        let active_text = match focus {
            IdeaPane::InOut => buffer_text(&self.in_out_buffer),
            IdeaPane::Verses => buffer_text(&self.verses_buffer),
            IdeaPane::Hooks => buffer_text(&self.hooks_buffer),
        };
        let label = self.idea_structure_tool_label(&active_text, kind);
        let buffer = match focus {
            IdeaPane::InOut => &self.in_out_buffer,
            IdeaPane::Verses => &self.verses_buffer,
            IdeaPane::Hooks => &self.hooks_buffer,
        };
        insert_structure_tag_at_cursor(buffer, &label);
        match focus {
            IdeaPane::InOut => {
                self.in_out_view.grab_focus();
            }
            IdeaPane::Verses => {
                self.verses_view.grab_focus();
            }
            IdeaPane::Hooks => {
                self.hooks_view.grab_focus();
            }
        }
    }

    fn idea_structure_tool_label(&self, text: &str, kind: IdeaStructureKind) -> String {
        match kind {
            IdeaStructureKind::Intro => "[INTRO]".to_owned(),
            IdeaStructureKind::Outro => "[OUTRO]".to_owned(),
            IdeaStructureKind::Verse => {
                format!("[VERSE {}]", next_idea_structure_number(text, kind))
            }
            IdeaStructureKind::Hook => format!("[HOOK {}]", next_idea_structure_number(text, kind)),
            IdeaStructureKind::Bridge => {
                format!("[BRIDGE {}]", next_idea_structure_number(text, kind))
            }
        }
    }

    fn idea_structure_tool_button_label(&self, text: &str, kind: IdeaStructureKind) -> String {
        match kind {
            IdeaStructureKind::Intro => "INTRO".to_owned(),
            IdeaStructureKind::Outro => "OUTRO".to_owned(),
            IdeaStructureKind::Verse => format!("VERSE {}", next_idea_structure_number(text, kind)),
            IdeaStructureKind::Hook => {
                let number = next_idea_structure_number(text, kind);
                if number == 1 {
                    "HOOK".to_owned()
                } else {
                    format!("HOOK {}", number)
                }
            }
            IdeaStructureKind::Bridge => {
                format!("BRIDGE {}", next_idea_structure_number(text, kind))
            }
        }
    }

    fn cycle_casing(&self) {
        let next = self.state.borrow().casing_mode.next();
        self.state.borrow_mut().casing_mode = next;
        self.update_casing_button();
        self.apply_casing_to_buffers();
    }

    fn update_casing_button(&self) {
        let mode = self.state.borrow().casing_mode;
        self.casing_button.set_label(mode.label());
        self.casing_button.remove_css_class("casing-active");
        if mode != CasingMode::Preserve {
            self.casing_button.add_css_class("casing-active");
        }
    }

    fn apply_casing_to_buffers(&self) {
        let mode = self.state.borrow().casing_mode;
        let mut changed_any = false;
        self.state.borrow_mut().programmatic_change = true;
        for buffer in [&self.in_out_buffer, &self.verses_buffer, &self.hooks_buffer] {
            let text = buffer_text(buffer);
            let cased = apply_casing(&text, mode);
            if cased != text {
                buffer.set_text(&cased);
                changed_any = true;
            }
        }
        self.state.borrow_mut().programmatic_change = false;
        self.refresh_stats();
        if changed_any {
            self.state.borrow_mut().is_dirty = true;
            self.save_now();
            self.refresh_search_matches();
        }
    }

    fn install_font_size(&self, size: u16) {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(&format!(".idea-text-view {{ font-size: {}pt; }}", size));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn refresh_manager(&self) {
        while let Some(child) = self.manager_list.first_child() {
            self.manager_list.remove(&child);
        }
        let ideas = match self.idea_store.list_named_ideas() {
            Ok(items) => items,
            Err(err) => {
                notifications::show_error(&self.notice, err.to_string());
                return;
            }
        };
        for snapshot in ideas {
            self.manager_list.append(&self.manager_row(snapshot));
        }
    }

    fn manager_row(&self, snapshot: IdeaSnapshot) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        shell.add_css_class("artist-row");
        shell.add_css_class("ideas-manager-row-shell");
        let is_current = self
            .state
            .borrow()
            .current
            .as_ref()
            .is_some_and(|current| current.settings.id == snapshot.settings.id);
        if is_current {
            shell.add_css_class("artist-row-selected");
        }
        shell.set_size_request(-1, 72);

        let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        labels.set_hexpand(true);
        labels.set_margin_start(12);
        let dt = gtk::Label::new(Some(&format_idea_datetime(snapshot.settings.updated_unix)));
        dt.add_css_class("muted");
        dt.add_css_class("ideas-manager-row-datetime");
        dt.set_xalign(0.0);
        labels.append(&dt);

        let open_button = gtk::Button::new();
        open_button.add_css_class("row-open-button");
        open_button.add_css_class("ideas-manager-open-button");
        if is_current {
            open_button.add_css_class("artist-row-selected");
        }
        open_button.set_hexpand(true);
        open_button.set_halign(gtk::Align::Fill);
        open_button.set_margin_start(0);
        open_button.set_margin_end(0);
        open_button.set_child(Some(&labels));

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions.add_css_class("row-action-stack");
        actions.set_valign(gtk::Align::Center);
        let transfer = icon_button("transfer.svg", "Transfer this idea");
        transfer.add_css_class("row-action-button");
        transfer.set_size_request(72, 72);
        let remove = icon_button("remove.svg", "Remove this idea");
        remove.add_css_class("row-action-button");
        remove.add_css_class("row-remove-button");
        remove.set_size_request(72, 72);
        actions.append(&transfer);
        actions.append(&remove);

        {
            let this = self.clone_handles();
            let id = snapshot.settings.id.clone();
            open_button.connect_clicked(move |_| {
                this.load_idea_into_editor(&id, true);
            });
        }

        {
            let this = self.clone_handles();
            let id = snapshot.settings.id.clone();
            remove.connect_clicked(move |_| match this.idea_store.remove_idea(&id) {
                Ok(()) => {
                    this.refresh_manager();
                }
                Err(err) => notifications::show_error(&this.notice, err.to_string()),
            });
        }

        {
            let this = self.clone_handles();
            let id = snapshot.settings.id.clone();
            transfer.connect_clicked(move |_| {
                if !this.load_idea_into_editor(&id, false) {
                    return;
                }
                this.manager_revealer.set_reveal_child(false);
                this.adjust_slide_panel_widths();
                this.show_artist_picker_for_transfer();
                this.transfer_revealer.set_reveal_child(true);
            });
        }

        shell.append(&open_button);
        shell.append(&actions);
        row.set_child(Some(&shell));
        row
    }

    fn show_artist_picker_for_transfer(&self) {
        self.transfer_header_title.set_label("Transfer to track");
        self.transfer_header_back_button.set_visible(false);
        clear_box(&self.transfer_content);
        let list = gtk::ListBox::new();
        list.add_css_class("artist-list");
        list.add_css_class("ideas-panel-list");
        list.set_selection_mode(gtk::SelectionMode::None);

        let artists = match ArtistStore::new_default().and_then(|store| store.load()) {
            Ok(file) => file.artists,
            Err(err) => {
                notifications::show_error(&self.notice, err.to_string());
                Vec::new()
            }
        };

        for artist in artists {
            list.append(&transfer_artist_row(self, artist));
        }

        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&list)
            .build();
        scroll.add_css_class("ideas-panel-scroll");
        self.transfer_content.append(&scroll);
    }

    fn show_track_picker_for_artist(&self, artist: &Artist) {
        self.transfer_header_title.set_label("Transfer to track");
        self.transfer_header_back_button.set_visible(true);
        clear_box(&self.transfer_content);

        let list = gtk::ListBox::new();
        list.add_css_class("artist-list");
        list.add_css_class("ideas-panel-list");
        list.set_selection_mode(gtk::SelectionMode::None);

        match TrackPager::new(self.track_store.clone(), &artist.id)
            .and_then(|mut pager| pager.load_next(10_000))
        {
            Ok(items) => {
                for item in items {
                    list.append(&transfer_track_row(
                        self,
                        &item.settings.name,
                        &item.settings.id,
                        item.settings.artwork.clone(),
                    ));
                }
            }
            Err(err) => notifications::show_error(&self.notice, err.to_string()),
        }

        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&list)
            .build();
        scroll.add_css_class("ideas-panel-scroll");
        self.transfer_content.append(&scroll);
    }

    fn load_idea_into_editor(&self, id: &str, close_manager_after: bool) -> bool {
        self.save_if_dirty();
        match self.idea_store.load_idea(id) {
            Ok(snapshot) => {
                self.load_snapshot(snapshot);
                self.refresh_manager();
                if close_manager_after {
                    self.manager_revealer.set_reveal_child(false);
                }
                self.verses_view.grab_focus();
                true
            }
            Err(err) => {
                notifications::show_error(&self.notice, err.to_string());
                false
            }
        }
    }

    fn transfer_current_idea_to_track(&self, track_id: &str) {
        let in_out = buffer_text(&self.in_out_buffer);
        let verses = buffer_text(&self.verses_buffer);
        let hooks = buffer_text(&self.hooks_buffer);
        let composed = {
            let result = build_transferred_lyrics(&in_out, &verses, &hooks);
            if result.trim().is_empty() {
                compose_plain_idea_text(&in_out, &verses, &hooks)
            } else {
                result
            }
        };

        if composed.trim().is_empty() {
            notifications::show_error(&self.notice, "Idea is empty; nothing to transfer.");
            return;
        }

        match self.track_store.load_track(track_id) {
            Ok((_settings, _final_text, raw_text, paths)) => {
                let mut merged = raw_text;
                if !merged.trim().is_empty() {
                    merged.push_str("\n\n");
                }
                merged.push_str(&composed);
                if let Err(err) = self.track_store.save_raw(&paths, &merged) {
                    notifications::show_error(&self.notice, err.to_string());
                    return;
                }
                notifications::show_info(
                    &self.notice,
                    "Idea transferred to selected track raw pane.",
                );
                self.manager_revealer.set_reveal_child(false);
                self.transfer_revealer.set_reveal_child(false);
                if self.transfer_complete_handler.borrow().is_some() {
                    let this = self.clone_handles();
                    let track_id = track_id.to_string();
                    gtk::glib::idle_add_local_once(move || {
                        if let Some(handler) = this.transfer_complete_handler.borrow().as_ref() {
                            handler(&track_id);
                        }
                    });
                }
            }
            Err(err) => notifications::show_error(&self.notice, err.to_string()),
        }
    }

    fn refresh_search_matches(&self) {
        self.clear_search_highlights();
        let query = self.search_entry.text().to_string();
        if query.trim().is_empty() {
            self.state.borrow_mut().search_flat.clear();
            self.state.borrow_mut().search_cursor = 0;
            return;
        }

        let options = SearchOptions {
            case_sensitive: false,
            fuzzy: self.search_fuzzy_active.get(),
        };

        let in_out_matches = find_matches(&buffer_text(&self.in_out_buffer), &query, &options);
        let verses_matches = find_matches(&buffer_text(&self.verses_buffer), &query, &options);
        let hooks_matches = find_matches(&buffer_text(&self.hooks_buffer), &query, &options);

        apply_search_highlights(&self.in_out_buffer, &in_out_matches);
        apply_search_highlights(&self.verses_buffer, &verses_matches);
        apply_search_highlights(&self.hooks_buffer, &hooks_matches);

        let mut flat = Vec::new();
        for item in in_out_matches {
            flat.push((IdeaPane::InOut, item));
        }
        for item in verses_matches {
            flat.push((IdeaPane::Verses, item));
        }
        for item in hooks_matches {
            flat.push((IdeaPane::Hooks, item));
        }

        let mut state = self.state.borrow_mut();
        state.search_flat = flat;
        state.search_cursor = 0;
        drop(state);
        self.focus_current_search();
    }

    fn advance_search(&self, direction: isize) {
        let len = self.state.borrow().search_flat.len();
        if len == 0 {
            return;
        }
        let mut state = self.state.borrow_mut();
        let cursor = state.search_cursor as isize + direction;
        state.search_cursor = ((cursor).rem_euclid(len as isize)) as usize;
        drop(state);
        self.focus_current_search();
    }

    fn focus_current_search(&self) {
        let state = self.state.borrow();
        let Some((pane, mat)) = state.search_flat.get(state.search_cursor).cloned() else {
            return;
        };
        drop(state);

        let (buffer, view) = match pane {
            IdeaPane::InOut => (&self.in_out_buffer, &self.in_out_view),
            IdeaPane::Verses => (&self.verses_buffer, &self.verses_view),
            IdeaPane::Hooks => (&self.hooks_buffer, &self.hooks_view),
        };

        clear_active_search_highlight(&self.in_out_buffer);
        clear_active_search_highlight(&self.verses_buffer);
        clear_active_search_highlight(&self.hooks_buffer);

        let start = buffer.iter_at_offset(mat.start as i32);
        let end = buffer.iter_at_offset(mat.end as i32);
        let active = ensure_active_search_tag(buffer);
        buffer.apply_tag(&active, &start, &end);
        buffer.select_range(&start, &end);
        let mut iter = start;
        view.scroll_to_iter(&mut iter, 0.12, true, 0.5, 0.5);
    }

    fn clear_search_highlights(&self) {
        for buffer in [&self.in_out_buffer, &self.verses_buffer, &self.hooks_buffer] {
            clear_search_highlights(buffer);
        }
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn transfer_artist_row(workspace: &IdeasWorkspace, artist: Artist) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-row");
    shell.add_css_class("ideas-transfer-list-row-shell");
    shell.set_size_request(-1, 60);

    let image_widget = transfer_image_widget(artist.image.clone(), 60);

    let name = gtk::Label::new(Some(&artist.name));
    name.set_xalign(0.0);
    name.set_margin_start(8);
    name.set_css_classes(&["idea-transfer-artist-name"]);

    let button = gtk::Button::new();
    button.add_css_class("row-open-button");
    button.add_css_class("ideas-transfer-row-open-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.add_css_class("ideas-transfer-row-content");
    content.append(&image_widget);
    content.append(&name);
    button.set_child(Some(&content));

    let this = workspace.clone_handles();
    let artist_for_button = artist.clone();
    button.connect_clicked(move |_| {
        this.show_track_picker_for_artist(&artist_for_button);
    });

    {
        let this = workspace.clone_handles();
        let artist_for_row = artist.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            this.show_track_picker_for_artist(&artist_for_row);
        });
        shell.add_controller(click);
    }

    shell.append(&button);
    row.set_child(Some(&shell));
    row
}

fn transfer_track_row(
    workspace: &IdeasWorkspace,
    track_name: &str,
    track_id: &str,
    artwork: Option<PathBuf>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-row");
    shell.add_css_class("ideas-transfer-list-row-shell");
    shell.add_css_class("ideas-transfer-track-row-shell");
    shell.set_size_request(-1, 60);

    let image_widget = transfer_image_widget(artwork, 60);

    let name = gtk::Label::new(Some(track_name));
    name.set_xalign(0.0);
    name.set_margin_start(8);
    name.set_css_classes(&["idea-transfer-artist-name"]);

    let button = gtk::Button::new();
    button.add_css_class("row-open-button");
    button.add_css_class("ideas-transfer-row-open-button");
    button.add_css_class("ideas-transfer-track-open-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.add_css_class("ideas-transfer-row-content");
    content.append(&image_widget);
    content.append(&name);
    button.set_child(Some(&content));

    let this = workspace.clone_handles();
    let id = track_id.to_string();
    button.connect_clicked(move |_| {
        this.transfer_current_idea_to_track(&id);
    });

    {
        let this = workspace.clone_handles();
        let id = track_id.to_string();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            this.transfer_current_idea_to_track(&id);
        });
        shell.add_controller(click);
    }

    shell.append(&button);
    row.set_child(Some(&shell));
    row
}

fn transfer_image_widget(path: Option<PathBuf>, size: i32) -> gtk::Widget {
    let size = size.max(1);
    if let Some(path) = path {
        let image = gtk::Image::from_file(path);
        image.set_pixel_size(size);
        image.set_size_request(size, size);
        image.set_halign(gtk::Align::Start);
        image.set_valign(gtk::Align::Center);
        image.set_hexpand(false);
        image.set_vexpand(false);
        image.set_can_target(false);
        image.add_css_class("ideas-transfer-thumb");
        image.upcast()
    } else {
        let placeholder = gtk::Label::new(Some(""));
        placeholder.set_size_request(size, size);
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_hexpand(false);
        placeholder.set_vexpand(false);
        placeholder.add_css_class("image-placeholder");
        placeholder.add_css_class("ideas-transfer-thumb");
        placeholder.upcast()
    }
}

fn idea_pane_shell(title: &str, view: &gtk::TextView) -> gtk::Box {
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("pane-shell");
    shell.set_hexpand(true);
    shell.set_vexpand(true);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("pane-title");
    label.set_xalign(0.0);
    shell.append(&label);
    let scrolled = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(view)
        .build();
    shell.append(&scrolled);
    shell
}

fn ideas_structure_tool_widgets() -> (
    gtk::Box,
    gtk::Button,
    gtk::Button,
    gtk::Button,
    gtk::Button,
    gtk::Button,
) {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    root.add_css_class("structure-tool-bar");
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Center);
    root.set_hexpand(false);
    root.set_vexpand(false);

    let intro = structure_tool_button("INTRO", "structure-tool-intro");
    let verse = structure_tool_button("VERSE 1", "structure-tool-verse");
    let hook = structure_tool_button("HOOK 1", "structure-tool-hook");
    let bridge = structure_tool_button("BRIDGE 1", "structure-tool-bridge");
    let outro = structure_tool_button("OUTRO", "structure-tool-outro");

    root.append(&intro);
    root.append(&verse);
    root.append(&hook);
    root.append(&bridge);
    root.append(&outro);

    (root, intro, verse, hook, bridge, outro)
}

fn structure_tool_button(label: &str, css_class: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("structure-tool-bubble");
    button.add_css_class(css_class);
    button.set_size_request(-1, 28);
    button.set_tooltip_text(Some("Insert structure tag at cursor"));
    button
}

fn insert_structure_tag_at_cursor(buffer: &gtk::TextBuffer, tag: &str) {
    let cursor = buffer.cursor_position().max(0) as usize;
    let text = buffer_text(buffer);
    let before: String = text.chars().take(cursor).collect();
    let after: String = text.chars().skip(cursor).collect();

    let mut insertion = String::new();
    if !before.is_empty() && !before.ends_with('\n') {
        insertion.push('\n');
    }
    insertion.push_str(tag);
    insertion.push('\n');
    if !after.is_empty() && !after.starts_with('\n') {
        insertion.push('\n');
    }

    let mut iter = buffer.iter_at_offset(cursor as i32);
    buffer.insert(&mut iter, &insertion);
}

fn paste_plain_text_with_trailing_newline(view: &gtk::TextView) {
    let clipboard = view.clipboard();
    let view = view.clone();
    clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
        let Ok(Some(text)) = result else {
            return;
        };
        if text.is_empty() {
            return;
        }

        let mut insertion = text.to_string();
        if !insertion.ends_with('\n') {
            insertion.push('\n');
        }

        let buffer = view.buffer();
        buffer.begin_user_action();
        buffer.delete_selection(true, view.is_editable());
        buffer.insert_interactive_at_cursor(insertion.as_str(), view.is_editable());
        buffer.end_user_action();
    });
}

fn next_idea_structure_number(text: &str, kind: IdeaStructureKind) -> usize {
    let usage = idea_structure_number_usage(text, kind);
    let next = usage
        .max_number
        .map(|number| number + 1)
        .unwrap_or(usage.unnumbered_count + 1);
    next.clamp(1, 99)
}

fn idea_structure_number_usage(
    text: &str,
    expected_kind: IdeaStructureKind,
) -> IdeaStructureNumberUsage {
    let chars = text.chars().collect::<Vec<_>>();
    let mut usage = IdeaStructureNumberUsage::default();
    let mut offset = 0usize;

    while offset < chars.len() {
        if chars[offset] != '[' {
            offset += 1;
            continue;
        }

        let Some(close_offset) = chars[offset + 1..]
            .iter()
            .position(|ch| *ch == ']')
            .map(|position| offset + 1 + position)
        else {
            offset += 1;
            continue;
        };

        let label = chars[offset + 1..close_offset].iter().collect::<String>();
        if let Some(number) = idea_structure_tag_number_for_kind(&label, expected_kind) {
            usage.max_number = Some(usage.max_number.unwrap_or(0).max(number));
        } else if idea_is_unnumbered_structure_tag_for_kind(&label, expected_kind) {
            usage.unnumbered_count += 1;
        }
        offset = close_offset + 1;
    }

    usage
}

fn idea_structure_tag_number_for_kind(
    label: &str,
    expected_kind: IdeaStructureKind,
) -> Option<usize> {
    let normalized = normalized_structure_label(label);
    let prefix = match expected_kind {
        IdeaStructureKind::Verse => "verse ",
        IdeaStructureKind::Hook => "hook ",
        IdeaStructureKind::Intro | IdeaStructureKind::Outro | IdeaStructureKind::Bridge => {
            return None;
        }
    };
    let number = normalized.strip_prefix(prefix)?.parse::<usize>().ok()?;
    (1..=99).contains(&number).then_some(number)
}

fn idea_is_unnumbered_structure_tag_for_kind(
    label: &str,
    expected_kind: IdeaStructureKind,
) -> bool {
    matches!(
        (normalized_structure_label(label).as_str(), expected_kind),
        ("hook", IdeaStructureKind::Hook)
    )
}

fn normalized_structure_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct IdeaStructureNumberUsage {
    max_number: Option<usize>,
    unnumbered_count: usize,
}

fn idea_text_view(buffer: &gtk::TextBuffer) -> gtk::TextView {
    let view = gtk::TextView::new();
    view.set_buffer(Some(buffer));
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.add_css_class("editor-view");
    view.add_css_class("idea-text-view");
    view
}

fn stat_bubble(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("editor-stat-bubble");
    label
}

fn icon_button(file_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&icon_widget(file_name, 18)));
    button
}

fn icon_text_button(file_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("icon-text-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&icon_widget(file_name, 18));
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

fn icon_widget(file_name: &str, size: i32) -> gtk::Widget {
    let path = app_paths::icon_path(file_name);
    let image = gtk::Image::from_file(path);
    image.set_pixel_size(size);
    image.set_size_request(size, size);
    image.set_hexpand(false);
    image.set_vexpand(false);
    image.set_halign(gtk::Align::Center);
    image.set_valign(gtk::Align::Center);
    image.add_css_class("ideas-icon");
    image.upcast()
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

fn idea_line_count(text: &str) -> usize {
    if text.trim().is_empty() {
        0
    } else {
        text.lines().count()
    }
}

fn idea_word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn format_idea_datetime(unix: u64) -> String {
    let Ok(date_time) = gtk::glib::DateTime::from_unix_local(unix as i64) else {
        return "000000 - 00:00".to_string();
    };
    date_time
        .format("%d%m%y - %H:%M")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "000000 - 00:00".to_string())
}

fn apply_search_highlights(buffer: &gtk::TextBuffer, matches: &[SearchMatch]) {
    clear_search_highlights(buffer);
    let tag = ensure_search_tag(buffer);
    for mat in matches {
        let start = buffer.iter_at_offset(mat.start as i32);
        let end = buffer.iter_at_offset(mat.end as i32);
        buffer.apply_tag(&tag, &start, &end);
    }
}

fn clear_search_highlights(buffer: &gtk::TextBuffer) {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    if buffer.tag_table().lookup("search_match").is_some() {
        buffer.remove_tag_by_name("search_match", &start, &end);
    }
    if buffer.tag_table().lookup("search_active_match").is_some() {
        buffer.remove_tag_by_name("search_active_match", &start, &end);
    }
}

fn clear_active_search_highlight(buffer: &gtk::TextBuffer) {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    if buffer.tag_table().lookup("search_active_match").is_some() {
        buffer.remove_tag_by_name("search_active_match", &start, &end);
    }
}

fn ensure_search_tag(buffer: &gtk::TextBuffer) -> gtk::TextTag {
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup("search_match") {
        return tag;
    }
    let tag = gtk::TextTag::builder().name("search_match").build();
    tag.set_background_rgba(Some(&gtk::gdk::RGBA::new(0.78, 0.00, 1.00, 1.0)));
    tag.set_foreground_rgba(Some(&gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0)));
    tag.set_underline(gtk::pango::Underline::Single);
    table.add(&tag);
    tag
}

fn ensure_active_search_tag(buffer: &gtk::TextBuffer) -> gtk::TextTag {
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup("search_active_match") {
        return tag;
    }
    let tag = gtk::TextTag::builder().name("search_active_match").build();
    tag.set_background_rgba(Some(&gtk::gdk::RGBA::new(0.86, 0.05, 1.00, 1.0)));
    tag.set_foreground_rgba(Some(&gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0)));
    tag.set_underline(gtk::pango::Underline::Double);
    table.add(&tag);
    tag
}

fn build_transferred_lyrics(in_out: &str, verses: &str, hooks: &str) -> String {
    let intro = extract_tag_body(in_out, "INTRO");
    let outro = extract_tag_body(in_out, "OUTRO");
    let plain_in_out = strip_structure_tags(in_out).trim().to_string();

    let numbered_verses = parse_numbered_blocks(verses, "VERSE");
    let ordered_hook_bridges = parse_ordered_hook_bridge_blocks(hooks);

    let mut parts = Vec::new();
    if !intro.trim().is_empty() {
        parts.push(tagged_block("INTRO", &intro));
    } else if !plain_in_out.is_empty() {
        parts.push(plain_in_out.clone());
    }

    if !numbered_verses.is_empty() {
        if !is_strictly_incremental(&numbered_verses) {
            return compose_plain_idea_text(in_out, verses, hooks);
        }

        for (verse_number, verse_body) in numbered_verses.iter() {
            if !verse_body.trim().is_empty() {
                parts.push(tagged_numbered_block("VERSE", *verse_number, verse_body));
            }
        }
        for block in ordered_hook_bridges {
            if block.body.trim().is_empty() {
                continue;
            }
            match block.number {
                Some(number) => parts.push(tagged_numbered_block(block.kind, number, &block.body)),
                None => parts.push(tagged_block(block.kind, &block.body)),
            }
        }
    } else {
        let verse_plain = strip_structure_tags(verses).trim().to_string();

        if !ordered_hook_bridges.is_empty() {
            if !verse_plain.is_empty() {
                parts.push(tagged_block("VERSE", &verse_plain));
            }
            for block in ordered_hook_bridges {
                if block.body.trim().is_empty() {
                    continue;
                }
                match block.number {
                    Some(number) => {
                        parts.push(tagged_numbered_block(block.kind, number, &block.body))
                    }
                    None => parts.push(tagged_block(block.kind, &block.body)),
                }
            }
        } else {
            let hook_blocks = parse_unnumbered_hook_blocks(hooks);
            let many_hooks = hook_blocks.len() > 1;
            if !verse_plain.is_empty() {
                if many_hooks {
                    let segments = split_text_by_parts(&verse_plain, hook_blocks.len());
                    for (index, hook) in hook_blocks.iter().enumerate() {
                        if let Some(segment) = segments.get(index) {
                            if !segment.trim().is_empty() {
                                parts.push(tagged_numbered_block("VERSE", index + 1, segment));
                            }
                        }
                        if !hook.trim().is_empty() {
                            parts.push(tagged_numbered_block("HOOK", index + 1, hook));
                        }
                    }
                } else {
                    parts.push(tagged_block("VERSE", &verse_plain));
                    if let Some(hook) = hook_blocks.first() {
                        if !hook.trim().is_empty() {
                            parts.push(tagged_block("HOOK", hook));
                        }
                    }
                }
            } else {
                for (index, hook) in hook_blocks.iter().enumerate() {
                    if !hook.trim().is_empty() {
                        if many_hooks {
                            parts.push(tagged_numbered_block("HOOK", index + 1, hook));
                        } else {
                            parts.push(tagged_block("HOOK", hook));
                        }
                    }
                }
            }
        }
    }

    if !outro.trim().is_empty() {
        parts.push(tagged_block("OUTRO", &outro));
    }

    let result = parts
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if result.trim().is_empty() {
        compose_plain_idea_text(in_out, verses, hooks)
    } else {
        result
    }
}

struct IdeaStructureSection {
    kind: &'static str,
    number: Option<usize>,
    body: String,
}

fn parse_ordered_hook_bridge_blocks(text: &str) -> Vec<IdeaStructureSection> {
    let mut out = Vec::new();
    let mut current_kind: Option<&'static str> = None;
    let mut current_number: Option<usize> = None;
    let mut current_lines = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((kind, number)) = parse_hook_bridge_tag_line(trimmed) {
            if let Some(kind_prev) = current_kind {
                out.push(IdeaStructureSection {
                    kind: kind_prev,
                    number: current_number,
                    body: current_lines.join("\n").trim().to_string(),
                });
                current_lines.clear();
            }
            current_kind = Some(kind);
            current_number = number;
            continue;
        }
        if current_kind.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some(kind_prev) = current_kind {
        out.push(IdeaStructureSection {
            kind: kind_prev,
            number: current_number,
            body: current_lines.join("\n").trim().to_string(),
        });
    }

    if out.is_empty() {
        let fallback = strip_structure_tags(text).trim().to_string();
        if !fallback.is_empty() {
            out.push(IdeaStructureSection {
                kind: "HOOK",
                number: None,
                body: fallback,
            });
        }
    }

    out
}

fn parse_hook_bridge_tag_line(line: &str) -> Option<(&'static str, Option<usize>)> {
    if !line.starts_with('[') || !line.ends_with(']') {
        return None;
    }
    let inner = line.trim_matches(['[', ']']).trim();
    let lower = inner.to_ascii_lowercase();
    if lower == "hook" {
        return Some(("HOOK", None));
    }
    if lower == "bridge" {
        return Some(("BRIDGE", None));
    }
    if let Some(stripped) = lower.strip_prefix("hook ") {
        if let Ok(number) = stripped.trim().parse::<usize>() {
            return Some(("HOOK", Some(number)));
        }
    }
    if let Some(stripped) = lower.strip_prefix("bridge ") {
        if let Ok(number) = stripped.trim().parse::<usize>() {
            return Some(("BRIDGE", Some(number)));
        }
    }
    None
}

fn tagged_block(tag: &str, body: &str) -> String {
    format!("[{}]\n{}", tag, body.trim())
}

fn tagged_numbered_block(tag: &str, number: usize, body: &str) -> String {
    format!("[{} {}]\n{}", tag, number, body.trim())
}

fn extract_tag_body(text: &str, tag: &str) -> String {
    let mut collecting = false;
    let mut body = Vec::new();
    let target = format!("[{}]", tag);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(&target) {
            collecting = true;
            continue;
        }
        if collecting && trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if collecting {
            body.push(line.to_string());
        }
    }
    body.join("\n").trim().to_string()
}

fn parse_numbered_blocks(text: &str, prefix: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current_number: Option<usize> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some(number) = parse_number_tag(line.trim(), prefix) {
            if let Some(number_prev) = current_number.take() {
                out.push((number_prev, current_lines.join("\n").trim().to_string()));
                current_lines.clear();
            }
            current_number = Some(number);
            continue;
        }
        if current_number.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some(number_prev) = current_number {
        out.push((number_prev, current_lines.join("\n").trim().to_string()));
    }

    out
}

fn parse_unnumbered_hook_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut collecting = false;
    let mut lines = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[HOOK]") {
            if collecting {
                out.push(lines.join("\n").trim().to_string());
                lines.clear();
            }
            collecting = true;
            continue;
        }
        if collecting && trimmed.starts_with('[') && trimmed.ends_with(']') {
            out.push(lines.join("\n").trim().to_string());
            lines.clear();
            collecting = false;
            continue;
        }
        if collecting {
            lines.push(line.to_string());
        }
    }
    if collecting {
        out.push(lines.join("\n").trim().to_string());
    }

    if out.is_empty() {
        let fallback = strip_structure_tags(text).trim().to_string();
        if !fallback.is_empty() {
            out.push(fallback);
        }
    }
    out
}

fn parse_number_tag(line: &str, prefix: &str) -> Option<usize> {
    if !line.starts_with('[') || !line.ends_with(']') {
        return None;
    }
    let inner = line.trim_matches(['[', ']']).trim();
    let lower = inner.to_ascii_lowercase();
    let prefix = format!("{} ", prefix.to_ascii_lowercase());
    let num_text = lower.strip_prefix(&prefix)?;
    num_text.parse::<usize>().ok().filter(|value| *value > 0)
}

fn is_strictly_incremental(blocks: &[(usize, String)]) -> bool {
    blocks
        .iter()
        .enumerate()
        .all(|(index, (number, _))| *number == index + 1)
}

fn strip_structure_tags(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with('[') && trimmed.ends_with(']'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compose_plain_idea_text(in_out: &str, verses: &str, hooks: &str) -> String {
    let mut parts = Vec::new();
    if !in_out.trim().is_empty() {
        parts.push(in_out.trim().to_string());
    }
    if !verses.trim().is_empty() {
        parts.push(verses.trim().to_string());
    }
    if !hooks.trim().is_empty() {
        parts.push(hooks.trim().to_string());
    }
    parts.join("\n\n")
}

fn split_text_by_parts(text: &str, parts: usize) -> Vec<String> {
    if parts == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return vec![String::new(); parts];
    }
    let mut out = Vec::new();
    let chunk_size = (lines.len() as f64 / parts as f64).ceil() as usize;
    for index in 0..parts {
        let start = index * chunk_size;
        let end = ((index + 1) * chunk_size).min(lines.len());
        if start >= lines.len() {
            out.push(String::new());
        } else {
            out.push(lines[start..end].join("\n").trim().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::track_store::{TrackDraft, TrackStore};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn transfer_builds_intro_verses_hooks_outro() {
        let out = build_transferred_lyrics(
            "[INTRO]\nopen\n[OUTRO]\nclose",
            "[VERSE 1]\na\n[VERSE 2]\nb",
            "[HOOK]\nH",
        );
        assert!(out.contains("open"));
        assert!(out.contains("a"));
        assert!(out.contains("b"));
        assert!(out.contains("H"));
        assert!(out.contains("close"));
    }

    #[test]
    fn unordered_verses_are_rejected() {
        let out = build_transferred_lyrics("", "[VERSE 2]\na\n[VERSE 1]\nb", "");
        assert_eq!(out.trim(), "[VERSE 2]\na\n[VERSE 1]\nb");
    }

    #[test]
    fn plain_verses_split_when_multiple_hooks_exist() {
        let out = build_transferred_lyrics("", "l1\nl2\nl3\nl4", "[HOOK]\nH1\n[HOOK]\nH2");
        assert!(out.contains("H1"));
        assert!(out.contains("H2"));
    }

    #[test]
    fn build_transferred_plain_in_out_no_tags() {
        let out = build_transferred_lyrics("Hello world", "", "");
        assert_eq!(out, "Hello world");
    }

    #[test]
    fn build_transferred_intro_outro_body() {
        let out = build_transferred_lyrics("[INTRO]\nHello\n[OUTRO]\nBye", "", "");
        assert!(out.starts_with("[INTRO]"));
        assert!(out.ends_with("Bye"));
        assert!(out.contains("[OUTRO]"));
    }

    #[test]
    fn transfer_writes_to_selected_track_raw() {
        let dir = tempdir().expect("temp dir can be created");
        let working_directory = dir.path().join("track-transfer");
        fs::create_dir_all(working_directory.join("lyrics")).expect("lyrics dir can be created");

        let store = TrackStore::new(dir.path().to_path_buf());
        let (_settings, paths) = store
            .create_track(TrackDraft {
                id: None,
                artist_id: "abcdef123456".to_owned(),
                name: "Transfer Track".to_owned(),
                tempo: 90,
                length: "03:42".to_owned(),
                working_directory: Some(working_directory.clone()),
                artwork_source: None,
            })
            .expect("track can be created");

        let idea_text = "[INTRO]\nHello\n[OUTRO]\nBye";
        let verses_text = "[VERSE 1]\nVerse line";
        let hooks_text = "[HOOK]\nHook line";
        let composed = build_transferred_lyrics(idea_text, verses_text, hooks_text);

        let mut merged = String::new();
        if paths.raw_path.exists() {
            merged = fs::read_to_string(&paths.raw_path).expect("read raw before transfer");
        }
        if !merged.trim().is_empty() {
            merged.push_str("\n\n");
        }
        merged.push_str(&composed);

        store.save_raw(&paths, &merged).expect("save raw transfer");

        let loaded_raw = fs::read_to_string(&paths.raw_path).expect("load raw after transfer");
        assert!(loaded_raw.contains("[INTRO]"));
        assert!(loaded_raw.contains("Hello"));
        assert!(loaded_raw.contains("[OUTRO]"));
        assert!(loaded_raw.contains("Bye"));
        assert!(loaded_raw.contains("[VERSE 1]"));
        assert!(loaded_raw.contains("Verse line"));
        assert!(loaded_raw.contains("[HOOK]"));
        assert!(loaded_raw.contains("Hook line"));
    }
}
