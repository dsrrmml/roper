use crate::app_logging;
use crate::app_paths;
use crate::error_handling::AppError;
use crate::models::{Artist, CasingMode, TrackSettings, UsedMaterial};
use crate::persistence::artist_store::ArtistStore;
use crate::persistence::settings_store::{
    AppSettings, SettingsStore, StartBehavior, VALID_FONT_SIZES,
};
use crate::persistence::track_store::{
    TrackDraft, TrackListItem, TrackPager, TrackPaths, TrackStore,
};
use crate::services::artwork::{import_track_artwork, preferred_track_artwork_in_working_directory};
use crate::services::casing::apply_casing;
use crate::services::live_highlights::{STRUCTURE_BUCKETS, StructureKind, structure_sequence};
use crate::services::material_usage::{
    add_used_material, effective_used_material, material_from_identity, raw_line_identities,
    remove_used_material,
};
use crate::services::search::{SearchMatch, SearchOptions, find_matches};
use crate::services::validation::{
    validate_absolute_path, validate_artwork_path, validate_length, validate_name, validate_tempo,
};
use crate::ui::editor_panes::{
    EditorPanes, RAW_PANE_WIDTH_FRACTION, buffer_text, replace_buffer_text_preserving_cursor,
};
use crate::ui::{
    blur_box::BlurBox,
    confirm,
    ideas_workspace::IdeasWorkspace,
    live_highlights, notifications, raw_gutter,
    row_icons::{self, RowActionIcon},
    splash,
    track_overlay::{OverlayTab, TrackOverlay},
    window_policy,
};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const TRACK_ARTWORK_PICKER_SIZE: i32 = 100;
const LIST_IMAGE_COLUMN_WIDTH: i32 = 160;
const TRACK_ROW_HEIGHT: i32 = LIST_IMAGE_COLUMN_WIDTH;
const PLACEHOLDER_ARTIST_ID: &str = "no-artist-selected";
const TRACK_LIST_THUMBNAIL_SIZE: i32 = LIST_IMAGE_COLUMN_WIDTH;
const ROW_ACTION_BUTTON_SIZE: i32 = TRACK_ROW_HEIGHT / 2;
const STRUCTURE_BUBBLE_MIN_WIDTH: i32 = 10;
const STRUCTURE_BUBBLE_HEIGHT: i32 = 10;
const EDITOR_TOOLBAR_MARGIN: i32 = 0;
const EDITOR_TOOLBAR_FALLBACK_HEIGHT: i32 = 36;
const INFO_SPLASH_PREVIEW_WIDTH: i32 = 320;
const INFO_SPLASH_PREVIEW_HEIGHT: i32 = 200;
const CREDITS_DURATION_SECS: f64 = 10.0;
const CREDIT_FONT_MIN_PT: f64 = 16.0;
const CREDIT_FONT_MAX_PT: f64 = 120.0;
const POINT_TO_PIXEL: f64 = 96.0 / 72.0;
const MATERIAL_REBUILD_DELAY: Duration = Duration::from_millis(140);
const SEARCH_SCROLL_BOTTOM_PADDING_PX: i32 = 360;

#[derive(Clone, Debug, Eq, PartialEq)]
struct InfoMetric {
    section: &'static str,
    label: &'static str,
    value: String,
}

#[derive(Clone)]
struct EditorStatsWidgets {
    root: gtk::Box,
    lines: SplitStatBubbleWidgets,
    words: SplitStatBubbleWidgets,
    chars: SplitStatBubbleWidgets,
}

#[derive(Clone)]
struct TrackStatsWidgets {
    root: gtk::Box,
    lines: SplitStatBubbleWidgets,
    words: SplitStatBubbleWidgets,
    chars: SplitStatBubbleWidgets,
}

#[derive(Clone)]
struct SplitStatBubbleWidgets {
    root: gtk::Box,
    raw: gtk::Label,
    final_pane: gtk::Label,
}

#[derive(Clone)]
struct StructureToolWidgets {
    root: gtk::Box,
    intro: gtk::Button,
    verse: gtk::Button,
    hook: gtk::Button,
    outro: gtk::Button,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct EditorTextStats {
    lines: usize,
    words: usize,
    chars: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PaneTextStats {
    raw: EditorTextStats,
    final_pane: EditorTextStats,
}

struct OpenTrack {
    settings: TrackSettings,
    paths: TrackPaths,
}

#[derive(Clone, Debug)]
struct CreditFlight {
    name: &'static str,
    font_size_pt: f64,
    alpha: f64,
    lane: f64,
    delay: f64,
    phase: f64,
    swirl: f64,
}

#[derive(Clone)]
struct PendingRawLine {
    normalized: String,
    material: UsedMaterial,
}

#[derive(Clone)]
struct PendingRawClipboard {
    lines: Vec<PendingRawLine>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneFocus {
    Final,
    Raw,
}

struct MainState {
    artist: Artist,
    track_store: TrackStore,
    settings_store: SettingsStore,
    app_settings: AppSettings,
    current: Option<OpenTrack>,
    pager: Option<TrackPager>,
    final_save_source: Option<gtk::glib::SourceId>,
    raw_save_source: Option<gtk::glib::SourceId>,
    settings_save_source: Option<gtk::glib::SourceId>,
    material_rebuild_source: Option<gtk::glib::SourceId>,
    draft_casing_mode: CasingMode,
    pending_raw_clipboard: Option<PendingRawClipboard>,
    material_settings_dirty: bool,
    draft_prompt_suppressed: bool,
    last_focus: PaneFocus,
    programmatic_text_change: bool,
    loading_page: bool,
    search_marker_layer: Option<gtk::DrawingArea>,
    track_stats_widgets: HashMap<String, TrackStatsWidgets>,
    ideas_mode_active: bool,
}

pub fn show_in_window(window: &gtk::ApplicationWindow, artist: Artist) {
    let settings_store = match SettingsStore::new_default() {
        Ok(store) => store,
        Err(err) => {
            show_startup_error(window, err);
            return;
        }
    };
    let app_settings = match settings_store.load() {
        Ok(settings) => settings,
        Err(err) => {
            show_startup_error(window, err);
            return;
        }
    };

    let track_store = match TrackStore::new_default() {
        Ok(store) => store,
        Err(err) => {
            show_startup_error(window, err);
            return;
        }
    };
    let state = Rc::new(RefCell::new(MainState {
        artist: artist.clone(),
        track_store,
        settings_store,
        app_settings: app_settings.clone(),
        current: None,
        pager: None,
        final_save_source: None,
        raw_save_source: None,
        settings_save_source: None,
        material_rebuild_source: None,
        draft_casing_mode: app_settings.default_casing_mode,
        pending_raw_clipboard: None,
        material_settings_dirty: false,
        draft_prompt_suppressed: false,
        last_focus: PaneFocus::Final,
        programmatic_text_change: false,
        loading_page: false,
        search_marker_layer: None,
        track_stats_widgets: HashMap::new(),
        ideas_mode_active: startup_uses_ideas_workspace(app_settings.start_behavior),
    }));

    if is_placeholder_artist(&artist) {
        window.set_title(Some("ROPER"));
    } else {
        window.set_title(Some(&format!("ROPER - {}", artist.name)));
    }
    window.add_css_class("surface");
    window_policy::set_fullscreen_enabled(window, app_settings.fullscreen);

    let editors = Rc::new(EditorPanes::new(app_settings.font_size_pt));
    editors.set_track_connection(false);
    let root_overlay = gtk::Overlay::new();
    root_overlay.set_hexpand(true);
    root_overlay.set_vexpand(true);
    let background_blur = BlurBox::new();
    background_blur.set_hexpand(true);
    background_blur.set_vexpand(true);
    let background_overlay = gtk::Overlay::new();
    background_overlay.set_hexpand(true);
    background_overlay.set_vexpand(true);
    let ideas_workspace =
        match IdeasWorkspace::new(app_settings.font_size_pt, app_settings.default_casing_mode) {
            Ok(workspace) => Rc::new(workspace),
            Err(err) => {
                show_startup_error(window, err);
                return;
            }
        };
    let editor_mode_stack = gtk::Stack::new();
    editor_mode_stack.set_hexpand(true);
    editor_mode_stack.set_vexpand(true);
    editor_mode_stack.add_named(&editors.root, Some("tracks-editor"));
    editor_mode_stack.add_named(&ideas_workspace.root, Some("ideas-editor"));
    editor_mode_stack.set_visible_child_name(startup_workspace_child_name(app_settings.start_behavior));
    background_overlay.set_child(Some(&editor_mode_stack));
    background_blur.append(&background_overlay);
    root_overlay.set_child(Some(&background_blur));

    let artwork = gtk::Picture::new();
    artwork.set_can_target(false);
    artwork.set_halign(gtk::Align::Fill);
    artwork.set_valign(gtk::Align::Fill);
    artwork.set_hexpand(true);
    artwork.set_vexpand(true);
    artwork.set_opacity(0.08);
    background_overlay.add_overlay(&artwork);

    let notice = gtk::Label::new(None);
    notice.add_css_class("notification");
    notice.set_wrap(true);
    notice.set_halign(gtk::Align::Center);
    notice.set_valign(gtk::Align::Start);
    notice.set_margin_top(12);
    notice.set_margin_start(18);
    notice.set_margin_end(18);
    notice.set_visible(false);
    root_overlay.add_overlay(&notice);

    let search_revealer = gtk::Revealer::new();
    search_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    search_revealer.set_transition_duration(180);
    search_revealer.set_reveal_child(false);
    search_revealer.set_halign(gtk::Align::Fill);
    search_revealer.set_valign(gtk::Align::Start);
    search_revealer.set_hexpand(true);
    search_revealer.set_vexpand(false);
    root_overlay.add_overlay(&search_revealer);

    let casing_button = gtk::Button::with_label(CasingMode::Preserve.label());
    casing_button.add_css_class("floating-button");
    casing_button.add_css_class("toolbar-control");
    casing_button.set_size_request(36, 36);
    casing_button.set_tooltip_text(Some("preserve / uppercase / lowercase"));
    let font_combo = font_size_combo(app_settings.font_size_pt);
    font_combo.add_css_class("toolbar-control");
    let track_name_label = gtk::Label::new(None);
    track_name_label.add_css_class("current-track-name");
    track_name_label.set_xalign(0.0);
    track_name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    track_name_label.set_max_width_chars(38);
    track_name_label.set_margin_start(28);
    track_name_label.set_margin_end(28);
    track_name_label.set_visible(false);
    let editor_stats = editor_stats_widgets();
    let structure_button = icon_button("structure.svg", "Structure tags");
    structure_button.add_css_class("floating-button");
    structure_button.add_css_class("structure-tool-toggle");
    structure_button.add_css_class("toolbar-control");
    structure_button.set_size_request(36, 36);
    structure_button.set_margin_start(28);
    structure_button.set_halign(gtk::Align::End);
    structure_button.set_child(Some(&icon_image_with_size("structure.svg", "", 18)));
    let structure_tool = structure_tool_widgets();
    let lower_left_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    lower_left_controls.add_css_class("editor-footer-controls");
    lower_left_controls.set_halign(gtk::Align::Start);
    lower_left_controls.set_valign(gtk::Align::Fill);
    lower_left_controls.set_vexpand(true);
    lower_left_controls.set_visible(true);
    lower_left_controls.append(&casing_button);
    lower_left_controls.append(&font_combo);
    lower_left_controls.append(&track_name_label);
    lower_left_controls.append(&editor_stats.root);
    lower_left_controls.append(&structure_button);
    lower_left_controls.append(&structure_tool.root);

    let menu_button = gtk::Button::new();
    menu_button.add_css_class("floating-button");
    menu_button.add_css_class("hamburger-menu-button");
    menu_button.add_css_class("toolbar-control");
    menu_button.set_size_request(36, 36);
    menu_button.set_margin_end(10);
    menu_button.set_tooltip_text(Some("Tracks"));
    menu_button.set_child(Some(&icon_image_with_size("menu.svg", "☰", 18)));
    menu_button.set_visible(true);

    let editor_chrome_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    editor_chrome_spacer.set_hexpand(false);
    editor_chrome_spacer.add_css_class("cspacer");
    editor_chrome_spacer.set_vexpand(false);
    let editor_chrome_push = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    editor_chrome_push.set_hexpand(true);

    let editor_footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    editor_footer.add_css_class("editor-footer");
    editor_footer.set_hexpand(true);
    editor_footer.set_vexpand(false);
    editor_footer.set_halign(gtk::Align::Fill);
    editor_footer.set_valign(gtk::Align::Fill);
    editor_footer.set_height_request(36);
    editor_footer.set_margin_start(EDITOR_TOOLBAR_MARGIN);
    editor_footer.set_margin_end(EDITOR_TOOLBAR_MARGIN);
    editor_footer.append(&lower_left_controls);
    editor_footer.append(&editor_chrome_push);
    editor_footer.append(&menu_button);

    let editor_chrome = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    editor_chrome.add_css_class("editor-footer-layer");
    editor_chrome.set_halign(gtk::Align::Fill);
    editor_chrome.set_valign(gtk::Align::Start);
    editor_chrome.set_hexpand(true);
    editor_chrome.set_vexpand(false);
    editor_chrome.set_visible(true);
    editor_chrome.append(&editor_chrome_spacer);
    editor_chrome.append(&editor_footer);

    let overlay_holder: Rc<RefCell<Option<Rc<TrackOverlay>>>> = Rc::new(RefCell::new(None));
    let overlay = {
        let holder = overlay_holder.clone();
        let background_blur = background_blur.clone();
        let state = state.clone();
        let artwork = artwork.clone();
        Rc::new(TrackOverlay::new(
            move || {
                if let Some(overlay) = holder.borrow().as_ref() {
                    overlay.hide();
                }
                update_artwork(&state, &artwork);
            },
            move |visible| background_blur.set_blurred(visible),
        ))
    };
    *overlay_holder.borrow_mut() = Some(overlay.clone());
    root_overlay.add_overlay(&overlay.layer);
    root_overlay.add_overlay(&editor_chrome);

    window.set_child(Some(&root_overlay));

    let casing_button_clone = casing_button.clone();
    let artwork_clone = artwork.clone();
    let track_name_label_clone = track_name_label.clone();
    ideas_workspace.set_transfer_complete_handler({
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        let overlay = overlay.clone();
        let editor_mode_stack = editor_mode_stack.clone();
        let editor_chrome = editor_chrome.clone();
        move |track_id: &str| {
            set_workspace_mode(&state, &overlay, &editor_mode_stack, &editor_chrome, false);
            overlay.hide();
            open_track_by_id_after_transfer(
                &state,
                &editors,
                &notice,
                &casing_button_clone,
                &artwork_clone,
                &track_name_label_clone,
                track_id,
            );
            editors.raw_view.grab_focus();
        }
    });

    wire_callbacks(
        window,
        &root_overlay,
        &state,
        &editors,
        &overlay,
        &search_revealer,
        &notice,
        &casing_button,
        &font_combo,
        &track_name_label,
        &menu_button,
        &ideas_workspace,
        &editor_mode_stack,
        &artwork,
        &editor_chrome,
        &editor_chrome_spacer,
    );
    wire_editor_stats(&state, &editors, &editor_stats);
    update_editor_stats(&editor_stats, &editors);
    wire_structure_tool(&editors, &structure_button, &structure_tool);

    window.present();
    window_policy::reassert_fullscreen(window);
    editors.keep_ratio();
    update_editor_chrome_layout(window, &root_overlay, &editor_chrome, &editor_chrome_spacer);
    apply_startup_behavior(
        window,
        &state,
        &editors,
        &overlay,
        &notice,
        &ideas_workspace,
        &editor_mode_stack,
        &editor_chrome,
        &casing_button,
        &artwork,
        &track_name_label,
    );
}

fn apply_startup_behavior(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    ideas_workspace: &Rc<IdeasWorkspace>,
    editor_mode_stack: &gtk::Stack,
    editor_chrome: &gtk::Box,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    let behavior = state.borrow().app_settings.start_behavior;
    match behavior {
        StartBehavior::FreshIdea => {
            set_workspace_mode(state, overlay, editor_mode_stack, editor_chrome, true);
            ideas_workspace.clear_current_idea();
            overlay.hide();
            editors.set_track_connection(false);
            update_casing_button(state, casing_button);
            update_track_name_label(state, track_name_label);
            ideas_workspace.focus_verses();
        }
        StartBehavior::LastIdea => {
            set_workspace_mode(state, overlay, editor_mode_stack, editor_chrome, true);
            overlay.hide();
            editors.set_track_connection(false);
            ideas_workspace.restore_latest_idea();
            ideas_workspace.focus_verses();
        }
        StartBehavior::LastTrack => {
            set_workspace_mode(state, overlay, editor_mode_stack, editor_chrome, false);
            overlay.hide();
            if is_placeholder_artist(&state.borrow().artist) {
                editors.set_track_connection(false);
                update_casing_button(state, casing_button);
                update_track_name_label(state, track_name_label);
            } else {
                open_initial_track(
                    state,
                    editors,
                    notice,
                    casing_button,
                    artwork,
                    track_name_label,
                );
            }
        }
        StartBehavior::TrackList => {
            set_workspace_mode(state, overlay, editor_mode_stack, editor_chrome, false);
            overlay.hide();
            editors.set_track_connection(false);
            open_track_overlay(
                state,
                editors,
                overlay,
                notice,
                window,
                casing_button,
                artwork,
                track_name_label,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn wire_callbacks(
    window: &gtk::ApplicationWindow,
    root_overlay: &gtk::Overlay,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    search_revealer: &gtk::Revealer,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    font_combo: &gtk::ComboBoxText,
    track_name_label: &gtk::Label,
    menu_button: &gtk::Button,
    ideas_workspace: &Rc<IdeasWorkspace>,
    editor_mode_stack: &gtk::Stack,
    artwork: &gtk::Picture,
    editor_chrome: &gtk::Box,
    editor_chrome_spacer: &gtk::Box,
) {
    {
        let editors = editors.clone();
        let window_for_callback = window.clone();
        let root_overlay_for_signal = root_overlay.clone();
        let root_overlay_for_callback = root_overlay.clone();
        let chrome = editor_chrome.clone();
        let chrome_spacer = editor_chrome_spacer.clone();
        root_overlay_for_signal.connect_width_request_notify(move |_| {
            editors.keep_ratio();
            update_editor_chrome_layout(
                &window_for_callback,
                &root_overlay_for_callback,
                &chrome,
                &chrome_spacer,
            );
        });
    }
    {
        let editors = editors.clone();
        let window = window.clone();
        let root_overlay = root_overlay.clone();
        let chrome = editor_chrome.clone();
        let chrome_spacer = editor_chrome_spacer.clone();
        let overlay = overlay.clone();
        let state = state.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
            editors.keep_ratio();
            update_editor_chrome_layout(&window, &root_overlay, &chrome, &chrome_spacer);
            chrome.set_visible(editor_footer_visible_for_workspace(
                overlay.is_visible(),
                state.borrow().ideas_mode_active,
            ));
            gtk::glib::ControlFlow::Continue
        });
    }
    {
        let chrome = editor_chrome.clone();
        let state = state.clone();
        overlay.layer.connect_visible_notify(move |layer| {
            chrome.set_visible(editor_footer_visible_for_workspace(
                layer.is_visible(),
                state.borrow().ideas_mode_active,
            ));
        });
    }

    {
        let state = state.clone();
        let editors_for_signal = editors.clone();
        editors_for_signal.final_buffer.connect_insert_text(
            move |buffer, location, inserted_text| {
                if state.borrow().programmatic_text_change {
                    return;
                }
                if let Some(cased) = cased_insert_text(inserted_text, current_casing(&state)) {
                    buffer.stop_signal_emission_by_name("insert-text");
                    buffer.insert(location, &cased);
                    return;
                }
                consume_pending_raw_clipboard_insert(&state, inserted_text);
            },
        );
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let raw_view = editors.raw_view.clone();
        raw_view.connect_copy_clipboard(move |_| {
            record_raw_clipboard_selection(&state, &editors);
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let raw_view = editors.raw_view.clone();
        raw_view.connect_cut_clipboard(move |_| {
            record_raw_clipboard_selection(&state, &editors);
        });
    }

    {
        let state = state.clone();
        editors.final_view.connect_copy_clipboard(move |_| {
            state.borrow_mut().pending_raw_clipboard = None;
        });
    }

    {
        let state = state.clone();
        editors.final_view.connect_cut_clipboard(move |_| {
            state.borrow_mut().pending_raw_clipboard = None;
        });
    }

    {
        let state = state.clone();
        let editors_for_signal = editors.clone();
        let editors_for_callback = editors.clone();
        let notice = notice.clone();
        let overlay = overlay.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let window = window.clone();
        editors_for_signal.final_buffer.connect_changed(move |_| {
            on_final_changed(
                &window,
                &state,
                &editors_for_callback,
                &notice,
                &overlay,
                &casing_button,
                &artwork,
                &track_name_label,
            );
        });
    }

    {
        let state = state.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            state.borrow_mut().last_focus = PaneFocus::Final;
        });
        editors.final_view.add_controller(focus);
    }

    {
        let state = state.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            state.borrow_mut().last_focus = PaneFocus::Raw;
        });
        editors.raw_view.add_controller(focus);
    }

    {
        let state = state.clone();
        let editors_for_signal = editors.clone();
        let editors_for_callback = editors.clone();
        let notice = notice.clone();
        let overlay = overlay.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        editors_for_signal.raw_buffer.connect_changed(move |_| {
            on_raw_changed(
                &window,
                &state,
                &editors_for_callback,
                &notice,
                &overlay,
                &casing_button,
                &artwork,
                &track_name_label,
            );
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        let casing_button_for_signal = casing_button.clone();
        let casing_button_for_callback = casing_button.clone();
        let overlay = overlay.clone();
        let artwork = artwork.clone();
        casing_button_for_signal.connect_clicked(move |_| {
            cycle_casing(
                &state,
                &editors,
                &notice,
                &overlay,
                &casing_button_for_callback,
                &artwork,
            );
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let ideas_workspace = ideas_workspace.clone();
        font_combo.connect_changed(move |combo| {
            let Some(text) = combo.active_text() else {
                return;
            };
            let Ok(font_size) = text.as_str().parse::<u16>() else {
                return;
            };
            if !VALID_FONT_SIZES.contains(&font_size) {
                return;
            }
            {
                let mut state_ref = state.borrow_mut();
                state_ref.app_settings.font_size_pt = font_size;
                if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                    notifications::show_error(&notice, err.to_string());
                }
            }
            editors.set_font_size(font_size);
            ideas_workspace.set_font_size(font_size);
            rebuild_material_ui(&state, &editors, &overlay, &notice);
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        menu_button.connect_clicked(move |_| {
            open_track_overlay(
                &state,
                &editors,
                &overlay,
                &notice,
                &window,
                &casing_button,
                &artwork,
                &track_name_label,
            );
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        overlay_for_signal.create_button.connect_clicked(move |_| {
            let use_current_buffers = state.borrow().current.is_none();
            show_create_track_panel(
                &window,
                &state,
                &editors,
                &overlay_for_callback,
                &notice,
                &casing_button,
                &artwork,
                &track_name_label,
                use_current_buffers,
            );
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button_for_form = casing_button.clone();
        let artwork_for_form = artwork.clone();
        let track_name_label_for_form = track_name_label.clone();
        overlay_for_signal
            .create_artist_button
            .connect_clicked(move |_| {
                show_artist_form(
                    &window,
                    &state,
                    &editors,
                    &overlay_for_callback,
                    &notice,
                    None,
                    &casing_button_for_form,
                    &artwork_for_form,
                    &track_name_label_for_form,
                );
            });
    }

    {
        let state = state.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let ideas_workspace = ideas_workspace.clone();
        let editor_mode_stack = editor_mode_stack.clone();
        let editor_chrome = editor_chrome.clone();
        overlay_for_signal
            .ideas_tab_button
            .connect_clicked(move |_| {
                show_ideas_tab(
                    &state,
                    &overlay_for_callback,
                    &ideas_workspace,
                    &editor_mode_stack,
                    &editor_chrome,
                    &notice,
                );
            });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let ideas_workspace = ideas_workspace.clone();
        let overlay = overlay.clone();
        let editor_mode_stack = editor_mode_stack.clone();
        let editor_chrome = editor_chrome.clone();
        ideas_workspace.leave_button.connect_clicked(move |_| {
            set_workspace_mode(&state, &overlay, &editor_mode_stack, &editor_chrome, false);
            open_track_overlay(
                &state,
                &editors,
                &overlay,
                &notice,
                &window,
                &casing_button,
                &artwork,
                &track_name_label,
            );
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button_for_artists = casing_button.clone();
        let artwork_for_artists = artwork.clone();
        let track_name_label_for_artists = track_name_label.clone();
        overlay_for_signal
            .artists_tab_button
            .connect_clicked(move |_| {
                show_artists_tab(
                    &window,
                    &state,
                    &editors,
                    &overlay_for_callback,
                    &notice,
                    &casing_button_for_artists,
                    &artwork_for_artists,
                    &track_name_label_for_artists,
                );
            });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        let window = window.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        overlay_for_signal
            .tracks_tab_button
            .connect_clicked(move |_| {
                open_track_overlay(
                    &state,
                    &editors,
                    &overlay_for_callback,
                    &notice,
                    &window,
                    &casing_button,
                    &artwork,
                    &track_name_label,
                );
            });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let font_combo = font_combo.clone();
        let ideas_workspace = ideas_workspace.clone();
        overlay_for_signal
            .settings_tab_button
            .connect_clicked(move |_| {
                show_settings_tab(
                    &state,
                    &editors,
                    &overlay_for_callback,
                    &notice,
                    &window,
                    &casing_button,
                    &font_combo,
                    &ideas_workspace,
                );
            });
    }

    {
        let root_overlay = root_overlay.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        overlay_for_signal
            .info_tab_button
            .connect_clicked(move |_| {
                show_info_tab(&overlay_for_callback, &root_overlay);
            });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay_for_signal = overlay.clone();
        let overlay_for_callback = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        overlay_for_signal
            .exit_tab_button
            .connect_clicked(move |_| {
                show_exit_tab(&window, &state, &editors, &overlay_for_callback, &notice);
            });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        let overlay = overlay.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let adjustment = overlay.scrolled.vadjustment();
        adjustment.connect_value_changed(move |adjustment| {
            if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 96.0 {
                load_more_tracks(
                    &state,
                    &editors,
                    &overlay,
                    &notice,
                    &window,
                    &casing_button,
                    &artwork,
                    &track_name_label,
                );
            }
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        window.connect_close_request(move |_| {
            flush_current(&state, &editors, &notice);
            gtk::glib::Propagation::Proceed
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                flush_current(&state, &editors, &notice);
            }
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let search_revealer = search_revealer.clone();
        let notice = notice.clone();
        let window_for_callback = window.clone();
        let root_overlay_for_key = root_overlay.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, modifiers| {
            handle_key(
                &window_for_callback,
                &root_overlay_for_key,
                &state,
                &editors,
                &overlay,
                &search_revealer,
                &notice,
                keyval,
                modifiers,
            )
        });
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        root_overlay.add_controller(key);
    }
}

fn open_initial_track(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    let store = state.borrow().track_store.clone();
    match store.latest_opened_track() {
        Ok(Some(item)) => {
            open_track_by_id(
                state,
                editors,
                notice,
                casing_button,
                artwork,
                track_name_label,
                &item.settings.id,
            );
        }
        Ok(None) => {
            editors.set_track_connection(false);
            update_casing_button(state, casing_button);
            update_track_name_label(state, track_name_label);
        }
        Err(err) => notifications::show_error(notice, err.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn maybe_prompt_for_draft_track(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    overlay: &Rc<TrackOverlay>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    if state.borrow().current.is_some() || !draft_is_ready(editors) {
        return;
    }
    {
        let mut state_ref = state.borrow_mut();
        if state_ref.draft_prompt_suppressed {
            state_ref.draft_prompt_suppressed = false;
            return;
        }
        state_ref.draft_prompt_suppressed = true;
    }
    notifications::show_info(
        notice,
        "This draft is not stored yet. Create a track to persist it.",
    );
    show_create_track_panel(
        window,
        state,
        editors,
        overlay,
        notice,
        casing_button,
        artwork,
        track_name_label,
        true,
    );
}

fn draft_is_ready(editors: &EditorPanes) -> bool {
    [editors.final_text(), editors.raw_text()]
        .iter()
        .any(|text| text.trim().len() > 1 && text.contains('\n'))
}

#[allow(clippy::too_many_arguments)]
fn show_create_track_panel(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    use_current_buffers: bool,
) {
    overlay.clear_edit();
    overlay.show(window.width());

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Create Track"));
    title.add_css_class("pane-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let close = icon_button("close.svg", "Close track editor");
    close.add_css_class("overlay-close-button");
    close.set_size_request(48, 48);
    close.set_halign(gtk::Align::End);
    header.append(&close);

    let form = gtk::Box::new(gtk::Orientation::Vertical, 10);
    form.add_css_class("editor-form");

    let name = gtk::Entry::builder().placeholder_text("Track name").build();
    name.add_css_class("form-field");
    name.add_css_class("name-field");
    name.set_hexpand(true);
    name.set_halign(gtk::Align::Fill);

    let tempo = gtk::Entry::builder()
        .placeholder_text("Tempo")
        .text("90")
        .build();
    tempo.add_css_class("form-field");
    tempo.set_hexpand(true);
    tempo.set_halign(gtk::Align::Fill);

    let length = gtk::Entry::builder()
        .placeholder_text("Length")
        .text("03:42")
        .build();
    length.add_css_class("form-field");
    length.set_hexpand(true);
    length.set_halign(gtk::Align::Fill);

    let working_directory = gtk::Entry::builder()
        .placeholder_text("Working directory")
        .build();
    working_directory.add_css_class("form-field");
    working_directory.set_hexpand(true);
    working_directory.set_halign(gtk::Align::Fill);
    working_directory.set_editable(false);
    let working_directory_source: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let choose_directory = icon_button("edit.svg", "Choose working directory");
    choose_directory.add_css_class("floating-button");
    choose_directory.set_size_request(32, 32);
    let directory_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    directory_row.append(&working_directory);
    directory_row.append(&choose_directory);

    let artwork_source: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let artwork_selection_is_manual = Rc::new(Cell::new(false));
    let (artwork_picker, artwork_preview, artwork_placeholder) =
        track_artwork_picker(None, "Choose artwork");

    let fields = gtk::Box::new(gtk::Orientation::Vertical, 10);
    fields.set_hexpand(true);
    fields.set_halign(gtk::Align::Fill);
    fields.append(&name);
    fields.append(&tempo);
    fields.append(&length);
    fields.append(&directory_row);

    let content_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content_row.set_hexpand(true);
    content_row.set_halign(gtk::Align::Fill);
    content_row.append(&fields);
    content_row.append(&artwork_picker);

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
    let create = icon_button("done.svg", "Create track");
    create.add_css_class("overlay-close-button");
    create.add_css_class("track-editor-done-button");
    create.set_size_request(48, 48);
    create.set_halign(gtk::Align::End);
    create.set_valign(gtk::Align::End);
    bottom_bar.append(&bottom_spacer);
    bottom_bar.append(&create);

    form.append(&content_row);
    form.append(&error);
    form.append(&bottom_bar);

    overlay.edit_box.append(&header);
    overlay.edit_box.append(&form);
    overlay.show_edit(true);

    let update_validity: Rc<dyn Fn()> = {
        let name = name.clone();
        let tempo = tempo.clone();
        let length = length.clone();
        let working_directory_source = working_directory_source.clone();
        let create = create.clone();
        Rc::new(move || {
            create.set_sensitive(
                validate_name(&name.text(), "track.name").is_ok()
                    && parse_tempo(&tempo).is_ok()
                    && validate_length(&length.text()).is_ok()
                    && working_directory_source.borrow().is_some(),
            );
        })
    };
    update_validity();
    {
        let update = update_validity.clone();
        name.connect_changed(move |_| update());
    }
    {
        let update = update_validity.clone();
        tempo.connect_changed(move |_| update());
    }
    {
        let update = update_validity.clone();
        length.connect_changed(move |_| update());
    }

    {
        let state = state.clone();
        let artwork = artwork.clone();
        let overlay = overlay.clone();
        close.connect_clicked(move |_| {
            update_artwork(&state, &artwork);
            overlay.clear_edit();
        });
    }

    {
        let window = window.clone();
        let working_directory = working_directory.clone();
        let working_directory_source = working_directory_source.clone();
        let artwork_source = artwork_source.clone();
        let artwork_selection_is_manual = artwork_selection_is_manual.clone();
        let artwork_preview = artwork_preview.clone();
        let artwork_placeholder = artwork_placeholder.clone();
        let artwork = artwork.clone();
        let state = state.clone();
        let error = error.clone();
        let update = update_validity.clone();
        let chooser_button = choose_directory.clone();
        chooser_button.connect_clicked(move |_| {
            let chooser = gtk::FileChooserNative::new(
                Some("Choose track working directory"),
                Some(&window),
                gtk::FileChooserAction::SelectFolder,
                Some("Choose"),
                Some("Cancel"),
            );
            let working_directory = working_directory.clone();
            let working_directory_source = working_directory_source.clone();
            let artwork_source = artwork_source.clone();
            let artwork_selection_is_manual = artwork_selection_is_manual.clone();
            let artwork_preview = artwork_preview.clone();
            let artwork_placeholder = artwork_placeholder.clone();
            let artwork = artwork.clone();
            let state = state.clone();
            let error = error.clone();
            let update = update.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        if validate_absolute_path(&path, "working_directory").is_ok() {
                            working_directory.set_text(&path.to_string_lossy());
                            *working_directory_source.borrow_mut() = Some(path.clone());
                            if !artwork_selection_is_manual.get() {
                                let auto_artwork = preferred_track_artwork_in_working_directory(&path);
                                set_track_artwork_preview(
                                    &artwork_preview,
                                    &artwork_placeholder,
                                    &artwork_source,
                                    &artwork,
                                    auto_artwork.as_deref(),
                                );
                                if auto_artwork.is_some() {
                                    notifications::clear(&error);
                                } else {
                                    update_artwork(&state, &artwork);
                                }
                            }
                            notifications::clear(&error);
                            update();
                        } else {
                            notifications::show_error(
                                &error,
                                "working directory must be an absolute local path",
                            );
                        }
                    }
                }
                chooser.destroy();
            });
            chooser.show();
        });
    }

    {
        let window = window.clone();
        let artwork_source = artwork_source.clone();
        let artwork_selection_is_manual = artwork_selection_is_manual.clone();
        let artwork_preview = artwork_preview.clone();
        let artwork_placeholder = artwork_placeholder.clone();
        let artwork = artwork.clone();
        let error = error.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let chooser = gtk::FileChooserNative::new(
                Some("Choose track artwork"),
                Some(&window),
                gtk::FileChooserAction::Open,
                Some("Choose"),
                Some("Cancel"),
            );
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            chooser.add_filter(&filter);
            let artwork_source = artwork_source.clone();
            let artwork_selection_is_manual = artwork_selection_is_manual.clone();
            let artwork_preview = artwork_preview.clone();
            let artwork_placeholder = artwork_placeholder.clone();
            let artwork = artwork.clone();
            let error = error.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        match validate_artwork_path(&path) {
                            Ok(()) => {
                                artwork_selection_is_manual.set(true);
                                set_track_artwork_preview(
                                    &artwork_preview,
                                    &artwork_placeholder,
                                    &artwork_source,
                                    &artwork,
                                    Some(&path),
                                );
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
        artwork_picker.add_controller(click);
    }

    let state = state.clone();
    let editors = editors.clone();
    let overlay = overlay.clone();
    let notice = notice.clone();
    let casing_button = casing_button.clone();
    let artwork = artwork.clone();
    let track_name_label = track_name_label.clone();
    {
        let name = name.clone();
        let tempo = tempo.clone();
        let length = length.clone();
        let working_directory_source = working_directory_source.clone();
        let artwork_source = artwork_source.clone();
        let error = error.clone();
        create.connect_clicked(move |_| {
            let tempo_value = match parse_tempo(&tempo) {
                Ok(value) => value,
                Err(err) => {
                    notifications::show_error(&error, err.to_string());
                    return;
                }
            };
            let store = state.borrow().track_store.clone();
            let created = store.create_track(TrackDraft {
                id: None,
                artist_id: state.borrow().artist.id.clone(),
                name: name.text().to_string(),
                tempo: tempo_value,
                length: length.text().to_string(),
                working_directory: working_directory_source.borrow().clone(),
                artwork_source: artwork_source.borrow().clone(),
            });
            let (mut settings, paths) = match created {
                Ok(created) => created,
                Err(err) => {
                    notifications::show_error(&error, err.to_string());
                    return;
                }
            };
            let final_text = if use_current_buffers {
                editors.final_text()
            } else {
                String::new()
            };
            let raw_text = if use_current_buffers {
                editors.raw_text()
            } else {
                String::new()
            };
            settings.casing_mode = new_track_casing(&state);
            let store = state.borrow().track_store.clone();
            if let Err(err) = store.save_final(&paths, &final_text) {
                notifications::show_error(&notice, err.to_string());
            }
            if let Err(err) = store.save_raw(&paths, &raw_text) {
                notifications::show_error(&notice, err.to_string());
            }
            if let Err(err) = store.save_settings(&paths, &settings) {
                notifications::show_error(&notice, err.to_string());
            }
            state.borrow_mut().draft_prompt_suppressed = false;
            set_open_track(
                &state,
                &editors,
                &notice,
                &overlay,
                &casing_button,
                &artwork,
                &track_name_label,
                settings,
                final_text,
                raw_text,
                paths,
            );
            overlay.hide();
            editors.final_view.grab_focus();
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn open_track_overlay(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    flush_current(state, editors, notice);
    overlay.select_tab(OverlayTab::Tracks);
    state.borrow_mut().track_stats_widgets.clear();
    overlay.clear_list();
    overlay.clear_edit();
    let active_artist = state.borrow().artist.clone();
    if is_placeholder_artist(&active_artist) {
        overlay.create_button.set_sensitive(false);
        overlay.create_button.remove_css_class("tab-action-blink");
        state.borrow_mut().pager = None;
        let width = window.width();
        overlay.show(width);
        return;
    }
    overlay.create_button.set_sensitive(true);
    overlay.create_button.remove_css_class("tab-action-blink");
    overlay
        .create_button
        .set_tooltip_text(Some(&format!("Create track for {}", active_artist.name)));
    let width = window.width();
    overlay.show(width);
    let store = state.borrow().track_store.clone();
    let artist_id = state.borrow().artist.id.clone();
    match TrackPager::new(store, &artist_id) {
        Ok(pager) => {
            state.borrow_mut().pager = Some(pager);
            load_more_tracks(
                state,
                editors,
                overlay,
                notice,
                window,
                casing_button,
                artwork,
                track_name_label,
            );
        }
        Err(err) => notifications::show_error(notice, err.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn load_more_tracks(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    {
        let mut state_ref = state.borrow_mut();
        if state_ref.loading_page {
            return;
        }
        if state_ref
            .pager
            .as_ref()
            .map(|pager| pager.is_exhausted())
            .unwrap_or(true)
        {
            return;
        }
        state_ref.loading_page = true;
    }

    let page = {
        let mut state_ref = state.borrow_mut();
        state_ref
            .pager
            .as_mut()
            .map(|pager| pager.load_next(10))
            .unwrap_or_else(|| Ok(Vec::new()))
    };
    state.borrow_mut().loading_page = false;

    match page {
        Ok(items) => {
            if items.is_empty() && overlay_track_list_is_empty(overlay) {
                overlay.append_track_row(&track_empty_row(window.height()));
                overlay.create_button.add_css_class("tab-action-blink");
                return;
            }
            overlay.create_button.remove_css_class("tab-action-blink");
            for item in items {
                overlay.append_track_row(&track_row(
                    item,
                    state,
                    editors,
                    overlay,
                    notice,
                    window,
                    casing_button,
                    artwork,
                    track_name_label,
                ));
            }
        }
        Err(err) => notifications::show_error(notice, err.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn track_row(
    item: TrackListItem,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) -> gtk::Box {
    let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
    wrapper.add_css_class("track-list-row");
    wrapper.set_size_request(-1, TRACK_ROW_HEIGHT);
    wrapper.set_hexpand(true);
    wrapper.set_vexpand(false);
    wrapper.set_valign(gtk::Align::Start);
    wrapper.set_overflow(gtk::Overflow::Hidden);

    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("track-row");
    if state
        .borrow()
        .current
        .as_ref()
        .is_some_and(|track| track.settings.id == item.settings.id)
    {
        shell.add_css_class("track-row-selected");
    }
    shell.set_size_request(-1, TRACK_ROW_HEIGHT);
    shell.set_hexpand(true);
    shell.set_vexpand(false);
    shell.set_valign(gtk::Align::Start);
    shell.set_overflow(gtk::Overflow::Hidden);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_vexpand(false);
    labels.set_valign(gtk::Align::Start);
    let name = gtk::Label::new(Some(&item.settings.name));
    name.set_xalign(0.0);
    name.set_vexpand(false);
    name.set_valign(gtk::Align::Start);
    name.add_css_class("track-row-name");
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    labels.append(&name);
    labels.append(&track_meta_bubbles(&item.settings));
    let stats = track_text_stats_bubbles(&item, state, editors);
    labels.append(&stats.root);
    state
        .borrow_mut()
        .track_stats_widgets
        .insert(item.settings.id.clone(), stats.clone());
    let structures = track_structure_bubbles(&item.paths);
    if structures.first_child().is_some() {
        labels.append(&structures);
    }

    let open_button = gtk::Button::new();
    open_button.add_css_class("row-open-button");
    open_button.set_size_request(-1, TRACK_ROW_HEIGHT);
    open_button.set_child(Some(&labels));
    open_button.set_hexpand(true);
    open_button.set_vexpand(true);
    open_button.set_valign(gtk::Align::Fill);
    open_button.set_overflow(gtk::Overflow::Hidden);
    let id = item.settings.id.clone();
    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        open_button.connect_clicked(move |_| {
            open_track_by_id(
                &state,
                &editors,
                &notice,
                &casing_button,
                &artwork,
                &track_name_label,
                &id,
            );
            overlay.hide();
            editors.final_view.grab_focus();
        });
    }

    let edit = gtk::Button::new();
    edit.add_css_class("row-action-button");
    edit.add_css_class("row-edit-button");
    edit.set_size_request(ROW_ACTION_BUTTON_SIZE, ROW_ACTION_BUTTON_SIZE);
    edit.set_tooltip_text(Some("Edit track"));
    let edit_icon = row_icons::icon(RowActionIcon::Edit);
    edit.set_child(Some(&edit_icon));
    {
        let state = state.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let item = item.clone();
        let window = window.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        edit.connect_clicked(move |_| {
            show_track_edit(
                &state,
                &overlay,
                &notice,
                &window,
                &artwork,
                &track_name_label,
                item.clone(),
            )
        });
    }
    let remove = gtk::Button::new();
    remove.add_css_class("row-action-button");
    remove.add_css_class("row-remove-button");
    remove.set_size_request(ROW_ACTION_BUTTON_SIZE, ROW_ACTION_BUTTON_SIZE);
    remove.set_tooltip_text(Some("Remove track"));
    let remove_icon = row_icons::icon(RowActionIcon::Remove);
    remove.set_child(Some(&remove_icon));
    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let window = window.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let item = item.clone();
        remove.connect_clicked(move |_| {
            request_remove_track(
                &window,
                &state,
                &editors,
                &overlay,
                &notice,
                &casing_button,
                &artwork,
                &track_name_label,
                item.clone(),
            );
        });
    }

    let actions = row_action_stack(edit, remove, TRACK_ROW_HEIGHT);
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

    let thumbnail = track_artwork_thumbnail(&item.settings);
    thumbnail.set_size_request(LIST_IMAGE_COLUMN_WIDTH, TRACK_ROW_HEIGHT);
    thumbnail.set_vexpand(false);
    thumbnail.set_valign(gtk::Align::Center);
    thumbnail.set_margin_end(0);

    shell.append(&open_button);
    shell.append(&edit_revealer);
    shell.append(&thumbnail);
    wrapper.append(&shell);
    wrapper
}

#[allow(clippy::too_many_arguments)]
fn request_remove_track(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    item: TrackListItem,
) {
    let message = format!("Remove track \"{}\"?", item.settings.name);
    let window_for_confirm = window.clone();
    let window = window.clone();
    let state = state.clone();
    let editors = editors.clone();
    let overlay = overlay.clone();
    let notice = notice.clone();
    let casing_button = casing_button.clone();
    let artwork = artwork.clone();
    let track_name_label = track_name_label.clone();
    confirm::confirm_remove(&window_for_confirm, "Remove Track", &message, move || {
        flush_current(&state, &editors, &notice);
        let removed_current = state
            .borrow()
            .current
            .as_ref()
            .is_some_and(|track| track.settings.id == item.settings.id);
        let store = state.borrow().track_store.clone();
        match store.remove_track(&item.settings.id) {
            Ok(()) => {
                if removed_current {
                    {
                        let mut state_ref = state.borrow_mut();
                        state_ref.current = None;
                        state_ref.programmatic_text_change = true;
                    }
                    editors.set_track_connection(false);
                    state.borrow_mut().programmatic_text_change = false;
                    update_casing_button(&state, &casing_button);
                    update_artwork(&state, &artwork);
                    update_track_name_label(&state, &track_name_label);
                    rebuild_material_ui(&state, &editors, &overlay, &notice);
                }
                notifications::show_info(&notice, "Track removed.");
                open_track_overlay(
                    &state,
                    &editors,
                    &overlay,
                    &notice,
                    &window,
                    &casing_button,
                    &artwork,
                    &track_name_label,
                );
            }
            Err(err) => notifications::show_error(&notice, err.to_string()),
        }
    });
}

fn startup_uses_ideas_workspace(behavior: StartBehavior) -> bool {
    matches!(behavior, StartBehavior::FreshIdea | StartBehavior::LastIdea)
}

fn startup_workspace_child_name(behavior: StartBehavior) -> &'static str {
    if startup_uses_ideas_workspace(behavior) {
        "ideas-editor"
    } else {
        "tracks-editor"
    }
}

fn track_meta_bubbles(settings: &TrackSettings) -> gtk::Box {
    let meta = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    meta.add_css_class("track-meta-bubbles");
    meta.set_size_request(-1, 28);
    meta.set_hexpand(false);
    meta.set_vexpand(false);
    meta.set_halign(gtk::Align::Start);
    meta.set_valign(gtk::Align::Start);
    meta.append(&track_meta_bubble(&format!("{} BPM", settings.tempo)));
    meta.append(&track_meta_bubble(&settings.length));
    meta
}

fn track_meta_bubble(text: &str) -> gtk::Label {
    let bubble = gtk::Label::new(Some(text));
    bubble.add_css_class("track-meta-bubble");
    bubble.set_xalign(0.5);
    bubble.set_halign(gtk::Align::Start);
    bubble.set_vexpand(false);
    bubble.set_wrap(true);
    bubble
}

fn track_text_stats_bubbles(
    item: &TrackListItem,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
) -> TrackStatsWidgets {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root.add_css_class("track-stats-bubbles");
    root.set_size_request(-1, 28);
    root.set_hexpand(false);
    root.set_vexpand(false);
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Start);

    let lines = split_stat_bubble("Lines — left raw / right final", "track-stat-bubble");
    let words = split_stat_bubble("Words — left raw / right final", "track-stat-bubble");
    let chars = split_stat_bubble("Characters — left raw / right final", "track-stat-bubble");
    root.append(&lines.root);
    root.append(&words.root);
    root.append(&chars.root);

    let widgets = TrackStatsWidgets {
        root,
        lines,
        words,
        chars,
    };
    update_track_stats_widgets(&widgets, track_row_stats(item, state, editors));
    widgets
}

fn row_action_stack(edit: gtk::Button, remove: gtk::Button, row_height: i32) -> gtk::Box {
    let stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_css_class("row-action-stack");
    stack.set_size_request(ROW_ACTION_BUTTON_SIZE, row_height);
    stack.set_hexpand(false);
    stack.set_vexpand(true);
    stack.set_valign(gtk::Align::Fill);
    stack.append(&edit);
    stack.append(&remove);
    stack
}

fn track_structure_bubbles(paths: &TrackPaths) -> gtk::Box {
    let bubbles = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bubbles.add_css_class("track-structure-bubbles");
    bubbles.set_size_request(-1, 14);
    bubbles.set_hexpand(true);
    bubbles.set_vexpand(false);
    bubbles.set_halign(gtk::Align::Fill);
    bubbles.set_valign(gtk::Align::Center);

    let Ok(final_text) = fs::read_to_string(&paths.final_path) else {
        return bubbles;
    };

    let sections = structure_sequence(&final_text);
    let total_length = sections
        .iter()
        .map(|section| section.range.end.saturating_sub(section.range.start))
        .sum::<usize>();

    let mut bubble_specs = Vec::new();
    for section in sections {
        let length = section.range.end.saturating_sub(section.range.start);
        let bubble = structure_bubble(section.kind, section.bucket, STRUCTURE_BUBBLE_MIN_WIDTH, length);
        bubbles.append(&bubble);
        bubble_specs.push((bubble, length));
    }

    let bubbles_for_update = bubbles.clone();
    let bubble_specs_for_update = bubble_specs;
    let total_length_for_update = total_length;
    bubbles.connect_realize(move |_| {
        let available_width = bubbles_for_update.width().max(STRUCTURE_BUBBLE_MIN_WIDTH);
        for (bubble, length) in bubble_specs_for_update.iter() {
            let width = structure_bubble_width(*length, total_length_for_update, available_width);
            bubble.set_size_request(width, STRUCTURE_BUBBLE_HEIGHT);
        }
    });

    bubbles
}

fn structure_bubble(
    kind: StructureKind,
    bucket: usize,
    width: i32,
    length: usize,
) -> gtk::DrawingArea {
    let bubble = gtk::DrawingArea::new();
    bubble.add_css_class("structure-bubble");
    bubble.set_size_request(width, STRUCTURE_BUBBLE_HEIGHT);
    bubble.set_hexpand(true);
    bubble.set_vexpand(false);
    bubble.set_halign(gtk::Align::Fill);
    bubble.set_valign(gtk::Align::Center);
    bubble.set_tooltip_text(Some(&format!(
        "{} · {} chars",
        structure_kind_label(kind),
        length
    )));
    let (red, green, blue, alpha) = structure_bubble_color(kind, bucket);
    bubble.set_draw_func(move |_, cr, width, height| {
        if width <= 0 || height <= 0 {
            return;
        }
        let width = width as f64;
        let height = height as f64;
        cr.set_source_rgba(red, green, blue, alpha);
        cr.rectangle(0.0, 0.0, width, height);
        cr.fill().ok();
    });
    bubble
}

fn structure_bubble_width(length: usize, total_length: usize, available_width: i32) -> i32 {
    let available_width = available_width.max(STRUCTURE_BUBBLE_MIN_WIDTH);
    if total_length == 0 || length == 0 {
        return STRUCTURE_BUBBLE_MIN_WIDTH.min(available_width);
    }

    let scaled = (available_width as f64 * length as f64 / total_length as f64).round();
    (scaled as i32).clamp(STRUCTURE_BUBBLE_MIN_WIDTH, available_width)
}

fn structure_bubble_color(kind: StructureKind, bucket: usize) -> (f64, f64, f64, f64) {
    let level = bucket.min(STRUCTURE_BUCKETS - 1) as f64 / (STRUCTURE_BUCKETS - 1) as f64;
    match kind {
        StructureKind::Intro => (0.56, 0.78, 1.0, 0.90),
        StructureKind::Verse => (0.32, 0.66 + level * 0.28, 0.40, 0.90),
        StructureKind::Hook => (1.0, 0.54 + level * 0.24, 0.22, 0.92),
        StructureKind::Outro => (0.10, 0.22, 0.50, 0.94),
    }
}

fn structure_kind_label(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Intro => "intro",
        StructureKind::Verse => "verse",
        StructureKind::Hook => "hook",
        StructureKind::Outro => "outro",
    }
}

fn track_artwork_thumbnail(settings: &TrackSettings) -> gtk::Widget {
    let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    frame.add_css_class("track-list-artwork");
    frame.set_size_request(LIST_IMAGE_COLUMN_WIDTH, TRACK_ROW_HEIGHT);
    frame.set_hexpand(false);
    frame.set_vexpand(false);
    frame.set_halign(gtk::Align::End);
    frame.set_valign(gtk::Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    frame.append(&spacer);

    if let Some(path) = &settings.artwork {
        let image = gtk::Image::from_file(path);
        image.set_pixel_size(TRACK_LIST_THUMBNAIL_SIZE);
        image.set_size_request(TRACK_LIST_THUMBNAIL_SIZE, TRACK_LIST_THUMBNAIL_SIZE);
        image.set_halign(gtk::Align::End);
        image.set_valign(gtk::Align::Center);
        image.set_vexpand(false);
        image.set_overflow(gtk::Overflow::Hidden);
        image.add_css_class("artwork-thumb");
        frame.append(&image);
    } else {
        let placeholder = gtk::Label::new(Some(""));
        placeholder.set_size_request(TRACK_LIST_THUMBNAIL_SIZE, TRACK_LIST_THUMBNAIL_SIZE);
        placeholder.set_halign(gtk::Align::End);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_vexpand(false);
        placeholder.set_overflow(gtk::Overflow::Hidden);
        placeholder.add_css_class("image-placeholder");
        frame.append(&placeholder);
    }

    frame.upcast()
}

fn track_artwork_picker(
    artwork: Option<&PathBuf>,
    placeholder_text: &str,
) -> (gtk::Overlay, gtk::Image, gtk::Label) {
    image_picker(artwork, placeholder_text, "Choose track artwork")
}

fn artist_image_picker(artist_image: Option<&PathBuf>) -> (gtk::Overlay, gtk::Image, gtk::Label) {
    image_picker(artist_image, "Choose image", "Choose artist image")
}

fn image_picker(
    artwork: Option<&PathBuf>,
    placeholder_text: &str,
    tooltip: &str,
) -> (gtk::Overlay, gtk::Image, gtk::Label) {
    let picker = gtk::Overlay::new();
    picker.add_css_class("artwork-picker");
    picker.set_size_request(TRACK_ARTWORK_PICKER_SIZE, TRACK_ARTWORK_PICKER_SIZE);
    picker.set_vexpand(false);
    picker.set_hexpand(false);
    picker.set_halign(gtk::Align::End);
    picker.set_valign(gtk::Align::Start);
    picker.set_tooltip_text(Some(tooltip));

    let picture = gtk::Image::new();
    picture.set_pixel_size(TRACK_ARTWORK_PICKER_SIZE);
    picture.set_size_request(TRACK_ARTWORK_PICKER_SIZE, TRACK_ARTWORK_PICKER_SIZE);
    picture.set_halign(gtk::Align::Fill);
    picture.set_valign(gtk::Align::Fill);
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.add_css_class("artwork-thumb");
    picker.set_child(Some(&picture));

    let placeholder = gtk::Label::new(Some(placeholder_text));
    placeholder.add_css_class("artwork-placeholder");
    placeholder.set_wrap(true);
    placeholder.set_justify(gtk::Justification::Center);
    placeholder.set_halign(gtk::Align::Center);
    placeholder.set_valign(gtk::Align::Center);
    placeholder.set_can_target(false);
    picker.add_overlay(&placeholder);

    if let Some(path) = artwork {
        picture.set_from_file(Some(path));
        placeholder.set_visible(false);
    }

    (picker, picture, placeholder)
}

#[allow(clippy::too_many_arguments)]
fn set_open_track(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    overlay: &Rc<TrackOverlay>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    mut settings: TrackSettings,
    final_text: String,
    raw_text: String,
    paths: TrackPaths,
) {
    {
        let mut state_ref = state.borrow_mut();
        state_ref.programmatic_text_change = true;
    }

    let cased_final = apply_casing(&final_text, settings.casing_mode);
    editors.set_texts(&cased_final, &raw_text);
    {
        let mut state_ref = state.borrow_mut();
        state_ref.programmatic_text_change = false;
    }

    let store = state.borrow().track_store.clone();
    if cased_final != final_text {
        if let Err(err) = store.save_final(&paths, &cased_final) {
            notifications::show_error(notice, err.to_string());
        }
    }
    if let Err(err) = store.mark_opened(&paths, &mut settings) {
        notifications::show_error(notice, err.to_string());
    }

    {
        let mut state_ref = state.borrow_mut();
        state_ref.current = Some(OpenTrack { settings, paths });
    }
    editors.set_track_connection(true);
    update_casing_button(state, casing_button);
    update_artwork(state, artwork);
    update_track_name_label(state, track_name_label);
    rebuild_material_ui(state, editors, overlay, notice);
    notifications::clear(notice);
}

fn open_track_by_id(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    id: &str,
) {
    open_track_by_id_with_options(
        state,
        editors,
        notice,
        casing_button,
        artwork,
        track_name_label,
        id,
        false,
    );
}

fn open_track_by_id_after_transfer(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    id: &str,
) {
    open_track_by_id_with_options(
        state,
        editors,
        notice,
        casing_button,
        artwork,
        track_name_label,
        id,
        true,
    );
}

#[allow(clippy::too_many_arguments)]
fn open_track_by_id_with_options(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    id: &str,
    skip_flush_if_same_track: bool,
) {
    let should_skip_flush = {
        let state_ref = state.borrow();
        let current_track_id = state_ref
            .current
            .as_ref()
            .map(|open| open.settings.id.as_str());
        should_skip_flush_for_track_reopen(current_track_id, id, skip_flush_if_same_track)
    };
    if should_skip_flush {
        clear_pending_save_sources(state);
    } else {
        flush_current(state, editors, notice);
    }

    let store = state.borrow().track_store.clone();
    match store.load_track(id) {
        Ok((settings, final_text, raw_text, paths)) => {
            let dummy_overlay = Rc::new(TrackOverlay::new(|| {}, |_| {}));
            set_open_track(
                state,
                editors,
                notice,
                &dummy_overlay,
                casing_button,
                artwork,
                track_name_label,
                settings,
                final_text,
                raw_text,
                paths,
            );
        }
        Err(err) => notifications::show_error(notice, err.to_string()),
    }
}

fn should_skip_flush_for_track_reopen(
    current_track_id: Option<&str>,
    target_track_id: &str,
    skip_flush_if_same_track: bool,
) -> bool {
    skip_flush_if_same_track && current_track_id.is_some_and(|current| current == target_track_id)
}

#[allow(clippy::too_many_arguments)]
fn on_final_changed(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    overlay: &Rc<TrackOverlay>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    if state.borrow().programmatic_text_change {
        return;
    }

    schedule_material_rebuild(state, editors, overlay, notice);
    if state.borrow().current.is_some() {
        schedule_final_save(state, editors, notice);
        if state.borrow().material_settings_dirty {
            state.borrow_mut().material_settings_dirty = false;
            save_settings_now(state, notice);
        }
    } else {
        maybe_prompt_for_draft_track(
            window,
            state,
            editors,
            notice,
            overlay,
            casing_button,
            artwork,
            track_name_label,
        );
    }
    update_casing_button(state, casing_button);
    update_artwork(state, artwork);
}

fn cased_insert_text(inserted_text: &str, mode: CasingMode) -> Option<String> {
    let cased = apply_casing(inserted_text, mode);
    (cased != inserted_text).then_some(cased)
}

fn consume_pending_raw_clipboard_insert(state: &Rc<RefCell<MainState>>, inserted_text: &str) {
    let mode = current_casing(state);
    let mut state_ref = state.borrow_mut();
    let Some(mut pending) = state_ref.pending_raw_clipboard.take() else {
        return;
    };

    let consumed = consume_pending_material_for_insert(&mut pending, inserted_text, mode);
    if consumed.is_empty() {
        return;
    }

    let mut changed = false;
    if let Some(open) = &mut state_ref.current {
        for material in consumed {
            changed |= add_used_material(&mut open.settings.used_material, material.clone());
            changed |= remove_used_material(&mut open.settings.dismissed_material, &material);
        }
    }

    if !pending.lines.is_empty() {
        state_ref.pending_raw_clipboard = Some(pending);
    }
    state_ref.material_settings_dirty |= changed;
}

fn consume_pending_material_for_insert(
    pending: &mut PendingRawClipboard,
    inserted_text: &str,
    mode: CasingMode,
) -> Vec<UsedMaterial> {
    let mut consumed = Vec::new();
    for inserted_line in normalized_material_lines(inserted_text, mode) {
        let Some(position) = pending
            .lines
            .iter()
            .position(|line| line.normalized == inserted_line)
        else {
            continue;
        };
        consumed.push(pending.lines.remove(position).material);
    }
    consumed
}

fn normalized_material_lines(text: &str, mode: CasingMode) -> Vec<String> {
    raw_line_identities(text, mode)
        .into_iter()
        .map(|identity| identity.normalized)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn on_raw_changed(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    overlay: &Rc<TrackOverlay>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    if state.borrow().programmatic_text_change {
        return;
    }
    schedule_material_rebuild(state, editors, overlay, notice);
    if state.borrow().current.is_some() {
        schedule_raw_save(state, editors, notice);
    } else {
        maybe_prompt_for_draft_track(
            window,
            state,
            editors,
            notice,
            overlay,
            casing_button,
            artwork,
            track_name_label,
        );
    }
}

fn cycle_casing(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
    overlay: &Rc<TrackOverlay>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
) {
    {
        let mut state_ref = state.borrow_mut();
        if let Some(open) = &mut state_ref.current {
            open.settings.casing_mode = open.settings.casing_mode.next();
        } else {
            state_ref.draft_casing_mode = state_ref.draft_casing_mode.next();
        }
    }
    let mode = current_casing(state);
    let final_text = apply_casing(&editors.final_text(), mode);
    {
        let mut state_ref = state.borrow_mut();
        state_ref.programmatic_text_change = true;
    }
    replace_buffer_text_preserving_cursor(&editors.final_buffer, &final_text);
    state.borrow_mut().programmatic_text_change = false;
    rebuild_material_ui(state, editors, overlay, notice);
    if state.borrow().current.is_some() {
        save_final_now(state, editors, notice);
        save_settings_now(state, notice);
    }
    update_casing_button(state, casing_button);
    update_artwork(state, artwork);
}

fn rebuild_material_ui(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
) {
    if let Some(source) = state.borrow_mut().material_rebuild_source.take() {
        source.remove();
    }

    let (mode, manual, dismissed, font_size) = {
        let state_ref = state.borrow();
        match &state_ref.current {
            Some(open) => (
                open.settings.casing_mode,
                open.settings.used_material.clone(),
                open.settings.dismissed_material.clone(),
                state_ref.app_settings.font_size_pt,
            ),
            None => (
                state_ref.draft_casing_mode,
                Vec::new(),
                Vec::new(),
                state_ref.app_settings.font_size_pt,
            ),
        }
    };
    let raw_text = editors.raw_text();
    let final_text = editors.final_text();
    let used = effective_used_material(&raw_text, &final_text, mode, &manual, &dismissed);
    let transfer_state = state.clone();
    let transfer_editors = editors.clone();
    let transfer_overlay = overlay.clone();
    let transfer = Rc::new(move |line: String, entry: UsedMaterial| {
        transfer_raw_line(
            &transfer_state,
            &transfer_editors,
            &transfer_overlay,
            line,
            entry,
        );
    });
    let unmark_state = state.clone();
    let unmark_editors = editors.clone();
    let unmark_overlay = overlay.clone();
    let unmark_notice = notice.clone();
    let unmark = Rc::new(move |entry: UsedMaterial| {
        {
            let mut state_ref = unmark_state.borrow_mut();
            if let Some(open) = &mut state_ref.current {
                remove_used_material(&mut open.settings.used_material, &entry);
                add_used_material(&mut open.settings.dismissed_material, entry.clone());
            }
        }
        save_settings_now(&unmark_state, &unmark_notice);
        rebuild_material_ui(
            &unmark_state,
            &unmark_editors,
            &unmark_overlay,
            &unmark_notice,
        );
    });
    raw_gutter::rebuild(
        &editors.raw_gutter,
        &editors.raw_view,
        &raw_text,
        mode,
        &used,
        font_size,
        transfer,
        unmark,
    );
    live_highlights::apply(
        &editors.raw_buffer,
        &editors.final_buffer,
        &editors.final_view,
        &editors.final_warning_layer,
        &raw_text,
        &final_text,
    );
    raw_gutter::apply_used_highlights(&editors.raw_buffer, &raw_text, mode, &used);
}

fn schedule_material_rebuild(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
) {
    if let Some(source) = state.borrow_mut().material_rebuild_source.take() {
        source.remove();
    }
    let state = state.clone();
    let editors = editors.clone();
    let overlay = overlay.clone();
    let notice = notice.clone();
    let state_for_source = state.clone();
    let source = gtk::glib::timeout_add_local_once(MATERIAL_REBUILD_DELAY, move || {
        state_for_source.borrow_mut().material_rebuild_source = None;
        rebuild_material_ui(&state_for_source, &editors, &overlay, &notice);
    });
    state.borrow_mut().material_rebuild_source = Some(source);
}

fn transfer_raw_line(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    line: String,
    entry: UsedMaterial,
) {
    let mode = current_casing(state);
    let insertion = apply_casing(&line, mode);
    insert_into_final(&editors.final_buffer, &insertion);
    {
        let mut state_ref = state.borrow_mut();
        if let Some(open) = &mut state_ref.current {
            add_used_material(&mut open.settings.used_material, entry.clone());
            remove_used_material(&mut open.settings.dismissed_material, &entry);
        }
    }
    rebuild_material_ui(state, editors, overlay, &gtk::Label::new(None));
    save_final_now(state, editors, &gtk::Label::new(None));
    save_settings_now(state, &gtk::Label::new(None));
    editors.final_view.grab_focus();
}

fn insert_into_final(buffer: &gtk::TextBuffer, line: &str) {
    let cursor = buffer.cursor_position().max(0) as usize;
    let text = buffer_text(buffer);
    let before: String = text.chars().take(cursor).collect();
    let after: String = text.chars().skip(cursor).collect();
    let mut insertion = String::new();
    if !before.is_empty() && !before.ends_with('\n') {
        insertion.push('\n');
    }
    insertion.push_str(line);
    if !after.is_empty() && !after.starts_with('\n') {
        insertion.push('\n');
    }
    let mut iter = buffer.iter_at_offset(cursor as i32);
    buffer.insert(&mut iter, &insertion);
}

fn schedule_final_save(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
) {
    if let Some(source) = state.borrow_mut().final_save_source.take() {
        source.remove();
    }
    let state = state.clone();
    let editors = editors.clone();
    let notice = notice.clone();
    let state_for_source = state.clone();
    let source = gtk::glib::timeout_add_local_once(Duration::from_millis(90), move || {
        save_final_now(&state_for_source, &editors, &notice);
    });
    state.borrow_mut().final_save_source = Some(source);
}

fn schedule_raw_save(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
) {
    if let Some(source) = state.borrow_mut().raw_save_source.take() {
        source.remove();
    }
    let state = state.clone();
    let editors = editors.clone();
    let notice = notice.clone();
    let state_for_source = state.clone();
    let source = gtk::glib::timeout_add_local_once(Duration::from_millis(90), move || {
        save_raw_now(&state_for_source, &editors, &notice);
    });
    state.borrow_mut().raw_save_source = Some(source);
}

fn save_final_now(state: &Rc<RefCell<MainState>>, editors: &Rc<EditorPanes>, notice: &gtk::Label) {
    let (store, paths) = {
        let mut state_ref = state.borrow_mut();
        state_ref.final_save_source = None;
        let Some(open) = &state_ref.current else {
            return;
        };
        (state_ref.track_store.clone(), open.paths.clone())
    };
    if let Err(err) = store.save_final(&paths, &editors.final_text()) {
        notifications::show_error(notice, format!("Could not autosave final: {}", err));
    }
}

fn save_raw_now(state: &Rc<RefCell<MainState>>, editors: &Rc<EditorPanes>, notice: &gtk::Label) {
    let (store, paths) = {
        let mut state_ref = state.borrow_mut();
        state_ref.raw_save_source = None;
        let Some(open) = &state_ref.current else {
            return;
        };
        (state_ref.track_store.clone(), open.paths.clone())
    };
    if let Err(err) = store.save_raw(&paths, &editors.raw_text()) {
        notifications::show_error(notice, format!("Could not autosave raw: {}", err));
    }
}

fn save_settings_now(state: &Rc<RefCell<MainState>>, notice: &gtk::Label) {
    let (store, paths, settings) = {
        let mut state_ref = state.borrow_mut();
        state_ref.settings_save_source = None;
        let Some(open) = &state_ref.current else {
            return;
        };
        (
            state_ref.track_store.clone(),
            open.paths.clone(),
            open.settings.clone(),
        )
    };
    if let Err(err) = store.save_settings(&paths, &settings) {
        notifications::show_error(notice, format!("Could not autosave settings: {}", err));
    }
}

fn flush_current(state: &Rc<RefCell<MainState>>, editors: &Rc<EditorPanes>, notice: &gtk::Label) {
    clear_pending_save_sources(state);
    save_final_now(state, editors, notice);
    save_raw_now(state, editors, notice);
    save_settings_now(state, notice);
}

fn clear_pending_save_sources(state: &Rc<RefCell<MainState>>) {
    let mut state_ref = state.borrow_mut();
    if let Some(source) = state_ref.final_save_source.take() {
        source.remove();
    }
    if let Some(source) = state_ref.raw_save_source.take() {
        source.remove();
    }
    if let Some(source) = state_ref.settings_save_source.take() {
        source.remove();
    }
    if let Some(source) = state_ref.material_rebuild_source.take() {
        source.remove();
    }
}

fn current_casing(state: &Rc<RefCell<MainState>>) -> CasingMode {
    let state_ref = state.borrow();
    state_ref
        .current
        .as_ref()
        .map(|open| open.settings.casing_mode)
        .unwrap_or(state_ref.draft_casing_mode)
}

fn new_track_casing(state: &Rc<RefCell<MainState>>) -> CasingMode {
    let state_ref = state.borrow();
    if state_ref.current.is_none() {
        state_ref.draft_casing_mode
    } else {
        state_ref.app_settings.default_casing_mode
    }
}

fn update_casing_button(state: &Rc<RefCell<MainState>>, button: &gtk::Button) {
    let mode = current_casing(state);
    button.set_label(mode.label());
    button.remove_css_class("casing-active");
    if mode != CasingMode::Preserve {
        button.add_css_class("casing-active");
    }
}

fn update_artwork(state: &Rc<RefCell<MainState>>, picture: &gtk::Picture) {
    let artwork = state
        .borrow()
        .current
        .as_ref()
        .and_then(|open| open.settings.artwork.clone());
    set_background_artwork_preview(picture, artwork.as_deref());
}

fn set_background_artwork_preview(picture: &gtk::Picture, artwork: Option<&Path>) {
    if let Some(path) = artwork {
        picture.set_file(Option::<&gtk::gio::File>::None);
        let file = gtk::gio::File::for_path(path);
        picture.set_file(Some(&file));
        picture.set_visible(true);
    } else {
        picture.set_file(Option::<&gtk::gio::File>::None);
        picture.set_visible(false);
    }
}

fn set_track_artwork_preview(
    artwork_preview: &gtk::Image,
    artwork_placeholder: &gtk::Label,
    artwork_source: &Rc<RefCell<Option<PathBuf>>>,
    artwork_background: &gtk::Picture,
    artwork: Option<&Path>,
) {
    if let Some(path) = artwork {
        artwork_preview.set_from_file(Some(path));
        artwork_preview.set_visible(true);
        artwork_placeholder.set_visible(false);
        *artwork_source.borrow_mut() = Some(path.to_path_buf());
        set_background_artwork_preview(artwork_background, Some(path));
    } else {
        artwork_preview.set_visible(false);
        artwork_placeholder.set_visible(true);
        *artwork_source.borrow_mut() = None;
    }
}

fn update_track_name_label(state: &Rc<RefCell<MainState>>, label: &gtk::Label) {
    let name = state
        .borrow()
        .current
        .as_ref()
        .map(|open| open.settings.name.clone());
    if let Some(name) = name {
        label.set_label(&name);
        label.set_visible(true);
    } else {
        label.set_label("");
        label.set_visible(false);
    }
}

fn update_editor_chrome_layout(
    window: &gtk::ApplicationWindow,
    root_overlay: &gtk::Overlay,
    chrome: &gtk::Box,
    raw_spacer: &gtk::Box,
) {
    let width = visible_editor_width(window, root_overlay);
    if width > 0 {
        raw_spacer.set_size_request(editor_chrome_raw_width(width), -1);
    }
    let height = visible_editor_height(window, root_overlay);
    if height > 0 {
        chrome.set_margin_top(editor_chrome_top(height, chrome.height()));
    }
}

fn editor_chrome_raw_width(width: i32) -> i32 {
    ((width.max(0) as f64) * RAW_PANE_WIDTH_FRACTION).round() as i32
}

fn editor_chrome_top(viewport_height: i32, chrome_height: i32) -> i32 {
    let chrome_height = if chrome_height > 0 {
        chrome_height
    } else {
        EDITOR_TOOLBAR_FALLBACK_HEIGHT
    };
    (viewport_height - chrome_height - EDITOR_TOOLBAR_MARGIN).max(0)
}

fn visible_editor_width(window: &gtk::ApplicationWindow, root_overlay: &gtk::Overlay) -> i32 {
    let monitor_width = primary_monitor_size().map(|(width, _)| width).unwrap_or(0);
    smallest_positive_dimension([window.width(), root_overlay.width(), monitor_width])
}

fn visible_editor_height(window: &gtk::ApplicationWindow, root_overlay: &gtk::Overlay) -> i32 {
    let monitor_height = primary_monitor_size()
        .map(|(_, height)| height)
        .unwrap_or(0);
    smallest_positive_dimension([window.height(), root_overlay.height(), monitor_height])
}

fn primary_monitor_size() -> Option<(i32, i32)> {
    let display = gtk::gdk::Display::default()?;
    let monitor = display
        .monitors()
        .item(0)?
        .downcast::<gtk::gdk::Monitor>()
        .ok()?;
    let geometry = monitor.geometry();
    Some((geometry.width(), geometry.height()))
}

fn smallest_positive_dimension<const N: usize>(values: [i32; N]) -> i32 {
    values
        .into_iter()
        .filter(|value| *value > 0)
        .min()
        .unwrap_or(0)
}

fn editor_footer_visible_for_workspace(overlay_visible: bool, ideas_mode_active: bool) -> bool {
    !overlay_visible && !ideas_mode_active
}

fn editor_stats_widgets() -> EditorStatsWidgets {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root.add_css_class("editor-stat-bubbles");
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Center);
    root.set_hexpand(false);
    root.set_vexpand(false);

    let lines = split_stat_bubble("Lines — left raw / right final", "editor-stat-bubble");
    let words = split_stat_bubble("Words — left raw / right final", "editor-stat-bubble");
    let chars = split_stat_bubble("Characters — left raw / right final", "editor-stat-bubble");
    root.append(&lines.root);
    root.append(&words.root);
    root.append(&chars.root);

    EditorStatsWidgets {
        root,
        lines,
        words,
        chars,
    }
}

fn split_stat_bubble(tooltip: &str, css_class: &str) -> SplitStatBubbleWidgets {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class(css_class);
    root.set_hexpand(false);
    root.set_vexpand(false);
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Center);
    root.set_tooltip_text(Some(tooltip));

    let raw = gtk::Label::new(None);
    raw.add_css_class("stat-bubble-segment");
    raw.add_css_class("stat-bubble-raw");

    let final_pane = gtk::Label::new(None);
    final_pane.add_css_class("stat-bubble-segment");
    final_pane.add_css_class("stat-bubble-final");

    if css_class == "editor-stat-bubble" {
        raw.add_css_class("editor-stat-segment");
        final_pane.add_css_class("editor-stat-segment");
    } else {
        raw.add_css_class("track-stat-segment");
        final_pane.add_css_class("track-stat-segment");
    }

    root.append(&raw);
    root.append(&final_pane);

    SplitStatBubbleWidgets {
        root,
        raw,
        final_pane,
    }
}

fn wire_editor_stats(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    widgets: &EditorStatsWidgets,
) {
    {
        let state = state.clone();
        let editors_for_signal = editors.clone();
        let editors_for_callback = editors.clone();
        let widgets = widgets.clone();
        editors_for_signal.final_buffer.connect_changed(move |_| {
            update_editor_stats(&widgets, &editors_for_callback);
            update_current_track_stats(&state, &editors_for_callback);
        });
    }
    {
        let state = state.clone();
        let editors_for_signal = editors.clone();
        let editors_for_callback = editors.clone();
        let widgets = widgets.clone();
        editors_for_signal.raw_buffer.connect_changed(move |_| {
            update_editor_stats(&widgets, &editors_for_callback);
            update_current_track_stats(&state, &editors_for_callback);
        });
    }
}

fn update_editor_stats(widgets: &EditorStatsWidgets, editors: &EditorPanes) {
    update_editor_stats_widgets(widgets, pane_text_stats(&editors.raw_text(), &editors.final_text()));
}

fn editor_text_stats(text: &str) -> EditorTextStats {
    EditorTextStats {
        lines: editor_line_count(text),
        words: editor_word_count(text),
        chars: text.chars().count(),
    }
}

fn editor_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split('\n').count()
    }
}

fn editor_word_count(text: &str) -> usize {
    let mut words = 0usize;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if !in_word {
                words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    words
}

fn pane_text_stats(raw_text: &str, final_text: &str) -> PaneTextStats {
    PaneTextStats {
        raw: editor_text_stats(raw_text),
        final_pane: editor_text_stats(final_text),
    }
}

fn text_stats_from_path(path: &PathBuf) -> EditorTextStats {
    fs::read_to_string(path)
        .map(|text| editor_text_stats(&text))
        .unwrap_or_default()
}

fn track_text_stats(paths: &TrackPaths) -> PaneTextStats {
    PaneTextStats {
        raw: text_stats_from_path(&paths.raw_path),
        final_pane: text_stats_from_path(&paths.final_path),
    }
}

fn resolved_track_row_stats(
    row_track_id: &str,
    current_track_id: Option<&str>,
    current_texts: Option<(&str, &str)>,
    saved_stats: PaneTextStats,
) -> PaneTextStats {
    if current_track_id.is_some_and(|current| current == row_track_id) {
        if let Some((raw_text, final_text)) = current_texts {
            return pane_text_stats(raw_text, final_text);
        }
    }
    saved_stats
}

fn track_row_stats(
    item: &TrackListItem,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
) -> PaneTextStats {
    let saved_stats = track_text_stats(&item.paths);
    let current_track_id = state
        .borrow()
        .current
        .as_ref()
        .map(|open| open.settings.id.clone());
    let raw_text = editors.raw_text();
    let final_text = editors.final_text();
    resolved_track_row_stats(
        &item.settings.id,
        current_track_id.as_deref(),
        Some((&raw_text, &final_text)),
        saved_stats,
    )
}

fn update_split_stat_bubble(widgets: &SplitStatBubbleWidgets, label: &str, raw_count: usize, final_count: usize) {
    widgets.raw.set_label(&split_stat_text(label, raw_count));
    widgets
        .final_pane
        .set_label(&split_stat_text(label, final_count));
}

fn split_stat_text(label: &str, count: usize) -> String {
    format!("{label} {count}")
}

fn update_editor_stats_widgets(widgets: &EditorStatsWidgets, stats: PaneTextStats) {
    update_split_stat_bubble(&widgets.lines, "L", stats.raw.lines, stats.final_pane.lines);
    update_split_stat_bubble(&widgets.words, "W", stats.raw.words, stats.final_pane.words);
    update_split_stat_bubble(&widgets.chars, "C", stats.raw.chars, stats.final_pane.chars);
}

fn update_track_stats_widgets(widgets: &TrackStatsWidgets, stats: PaneTextStats) {
    update_split_stat_bubble(&widgets.lines, "L", stats.raw.lines, stats.final_pane.lines);
    update_split_stat_bubble(&widgets.words, "W", stats.raw.words, stats.final_pane.words);
    update_split_stat_bubble(&widgets.chars, "C", stats.raw.chars, stats.final_pane.chars);
}

fn update_current_track_stats(state: &Rc<RefCell<MainState>>, editors: &EditorPanes) {
    let widgets = {
        let state_ref = state.borrow();
        let Some(open) = &state_ref.current else {
            return;
        };
        state_ref.track_stats_widgets.get(&open.settings.id).cloned()
    };
    let Some(widgets) = widgets else {
        return;
    };
    update_track_stats_widgets(&widgets, pane_text_stats(&editors.raw_text(), &editors.final_text()));
}

fn structure_tool_widgets() -> StructureToolWidgets {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    root.add_css_class("structure-tool-bar");
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Center);
    root.set_hexpand(false);
    root.set_vexpand(false);
    root.set_visible(false);

    let intro = structure_tool_bubble("INTRO", "structure-tool-intro");
    let verse = structure_tool_bubble("VERSE 1", "structure-tool-verse");
    let hook = structure_tool_bubble("HOOK 1", "structure-tool-hook");
    let outro = structure_tool_bubble("OUTRO", "structure-tool-outro");

    root.append(&intro);
    root.append(&verse);
    root.append(&hook);
    root.append(&outro);

    StructureToolWidgets {
        root,
        intro,
        verse,
        hook,
        outro,
    }
}

fn structure_tool_bubble(label: &str, css_class: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("structure-tool-bubble");
    button.add_css_class(css_class);
    button.set_size_request(-1, 28);
    button.set_tooltip_text(Some("Insert structure tag at cursor"));
    button
}

fn wire_structure_tool(
    editors: &Rc<EditorPanes>,
    toggle: &gtk::Button,
    widgets: &StructureToolWidgets,
) {
    update_structure_tool_labels(widgets, &editors.final_text());
    {
        let root = widgets.root.clone();
        toggle.connect_clicked(move |_| {
            root.set_visible(!root.is_visible());
        });
    }
    {
        let editors_for_signal = editors.clone();
        let editors_for_callback = editors.clone();
        let widgets = widgets.clone();
        editors_for_signal.final_buffer.connect_changed(move |_| {
            update_structure_tool_labels(&widgets, &editors_for_callback.final_text());
        });
    }

    connect_structure_insert(&widgets.intro, editors, StructureKind::Intro);
    connect_structure_insert(&widgets.verse, editors, StructureKind::Verse);
    connect_structure_insert(&widgets.hook, editors, StructureKind::Hook);
    connect_structure_insert(&widgets.outro, editors, StructureKind::Outro);
}

fn connect_structure_insert(button: &gtk::Button, editors: &Rc<EditorPanes>, kind: StructureKind) {
    let editors = editors.clone();
    button.connect_clicked(move |_| {
        let label = structure_tool_label_for_kind(&editors.final_text(), kind);
        insert_structure_tag_at_cursor(&editors.final_buffer, &label);
        editors.final_view.grab_focus();
    });
}

fn update_structure_tool_labels(widgets: &StructureToolWidgets, final_text: &str) {
    let sanitize = |s: String| s.replace(['[', ']'], "");

    widgets
        .intro
        .set_label(&sanitize(structure_tool_label_for_kind(
            final_text,
            StructureKind::Intro,
        )));

    widgets
        .verse
        .set_label(&sanitize(structure_tool_label_for_kind(
            final_text,
            StructureKind::Verse,
        )));

    widgets
        .hook
        .set_label(&sanitize(structure_tool_label_for_kind(
            final_text,
            StructureKind::Hook,
        )));

    widgets
        .outro
        .set_label(&sanitize(structure_tool_label_for_kind(
            final_text,
            StructureKind::Outro,
        )));
}

fn structure_tool_label_for_kind(final_text: &str, kind: StructureKind) -> String {
    match kind {
        StructureKind::Intro => "[INTRO]".to_owned(),
        StructureKind::Verse => format!(
            "[VERSE {}]",
            next_structure_number(final_text, StructureKind::Verse)
        ),
        StructureKind::Hook => format!(
            "[HOOK {}]",
            next_structure_number(final_text, StructureKind::Hook)
        ),
        StructureKind::Outro => "[OUTRO]".to_owned(),
    }
}

fn next_structure_number(final_text: &str, kind: StructureKind) -> usize {
    let usage = structure_number_usage(final_text, kind);
    let next = usage
        .max_number
        .map(|number| number + 1)
        .unwrap_or(usage.unnumbered_count + 1);
    next.clamp(1, 99)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StructureNumberUsage {
    max_number: Option<usize>,
    unnumbered_count: usize,
}

fn structure_number_usage(final_text: &str, expected_kind: StructureKind) -> StructureNumberUsage {
    let chars = final_text.chars().collect::<Vec<_>>();
    let mut usage = StructureNumberUsage::default();
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
        if let Some(number) = structure_tag_number_for_kind(&label, expected_kind) {
            usage.max_number = Some(usage.max_number.unwrap_or(0).max(number));
        } else if is_unnumbered_structure_tag_for_kind(&label, expected_kind) {
            usage.unnumbered_count += 1;
        }
        offset = close_offset + 1;
    }

    usage
}

fn structure_tag_number_for_kind(label: &str, expected_kind: StructureKind) -> Option<usize> {
    let normalized = normalized_structure_label(label);
    let prefix = match expected_kind {
        StructureKind::Verse => "verse ",
        StructureKind::Hook => "hook ",
        StructureKind::Intro | StructureKind::Outro => return None,
    };
    let number = normalized.strip_prefix(prefix)?.parse::<usize>().ok()?;
    (1..=99).contains(&number).then_some(number)
}

fn is_unnumbered_structure_tag_for_kind(label: &str, expected_kind: StructureKind) -> bool {
    matches!(
        (normalized_structure_label(label).as_str(), expected_kind),
        ("hook", StructureKind::Hook)
    )
}

fn normalized_structure_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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

fn set_workspace_mode(
    state: &Rc<RefCell<MainState>>,
    overlay: &Rc<TrackOverlay>,
    editor_mode_stack: &gtk::Stack,
    editor_chrome: &gtk::Box,
    ideas_mode_active: bool,
) {
    {
        let mut state_ref = state.borrow_mut();
        state_ref.ideas_mode_active = ideas_mode_active;
        state_ref.app_settings.last_workspace_mode = if ideas_mode_active {
            "ideas".to_owned()
        } else {
            "tracks".to_owned()
        };
        let _ = state_ref.settings_store.save(&state_ref.app_settings);
    }

    if ideas_mode_active {
        editor_mode_stack.set_visible_child_name("ideas-editor");
        editor_chrome.set_visible(false);
    } else {
        editor_mode_stack.set_visible_child_name("tracks-editor");
        editor_chrome.set_visible(editor_footer_visible_for_workspace(
            overlay.is_visible(),
            false,
        ));
    }
}

fn show_ideas_tab(
    state: &Rc<RefCell<MainState>>,
    overlay: &Rc<TrackOverlay>,
    ideas_workspace: &Rc<IdeasWorkspace>,
    editor_mode_stack: &gtk::Stack,
    editor_chrome: &gtk::Box,
    notice: &gtk::Label,
) {
    let (default_casing, font_size) = {
        let state_ref = state.borrow();
        (
            state_ref.app_settings.default_casing_mode,
            state_ref.app_settings.font_size_pt,
        )
    };
    ideas_workspace.set_default_casing(default_casing);
    ideas_workspace.set_font_size(font_size);
    notifications::clear(notice);
    set_workspace_mode(state, overlay, editor_mode_stack, editor_chrome, true);
    overlay.hide();
    ideas_workspace.focus_verses();
}

fn show_artists_tab(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    overlay.show(window.width());
    overlay.select_tab(OverlayTab::Artists);
    overlay.clear_artists();

    let store = match ArtistStore::new_default() {
        Ok(store) => Rc::new(store),
        Err(err) => {
            notifications::show_error(notice, err.to_string());
            return;
        }
    };

    let track_counts = state
        .borrow()
        .track_store
        .track_counts_by_artist()
        .unwrap_or_default();

    match store.load() {
        Ok(file) => {
            notifications::clear(notice);
            if file.artists.is_empty() {
                overlay.append_artist_row(&artist_empty_row(window.height()));
                overlay
                    .create_artist_button
                    .add_css_class("tab-action-blink");
                return;
            }
            overlay
                .create_artist_button
                .remove_css_class("tab-action-blink");
            for artist in file.artists {
                let track_count = *track_counts.get(&artist.id).unwrap_or(&0);
                overlay.append_artist_row(&artist_menu_row(
                    window,
                    state,
                    editors,
                    overlay,
                    notice,
                    casing_button,
                    artwork,
                    track_name_label,
                    artist,
                    track_count,
                ));
            }
        }
        Err(err) => {
            overlay
                .create_artist_button
                .remove_css_class("tab-action-blink");
            notifications::show_error(notice, err.to_string())
        }
    }
}

fn select_artist_and_show_tracks(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    artist: Artist,
) {
    flush_current(state, editors, notice);
    {
        let mut state_ref = state.borrow_mut();
        state_ref.artist = artist;
    }
    open_track_overlay(
        state,
        editors,
        overlay,
        notice,
        window,
        casing_button,
        artwork,
        track_name_label,
    );
}

fn artist_menu_row(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    artist: Artist,
    track_count: usize,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_size_request(-1, 160);

    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-row");
    if state.borrow().artist.id == artist.id {
        shell.add_css_class("artist-row-selected");
    }
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
    description.set_lines(2);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    labels.append(&name);
    labels.append(&description);
    labels.append(&spacer);
    labels.append(&artist_stats_bubbles(track_count));

    let open_button = gtk::Button::new();
    open_button.add_css_class("row-open-button");
    open_button.set_child(Some(&labels));
    open_button.set_hexpand(true);
    open_button.set_vexpand(true);
    open_button.set_valign(gtk::Align::Fill);
    {
        let window = window.clone();
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let artist = artist.clone();
        open_button.connect_clicked(move |_| {
            select_artist_and_show_tracks(
                &state,
                &editors,
                &overlay,
                &notice,
                &window,
                &casing_button,
                &artwork,
                &track_name_label,
                artist.clone(),
            );
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
        let window = window.clone();
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let artist = artist.clone();
        edit.connect_clicked(move |_| {
            show_artist_form(
                &window,
                &state,
                &editors,
                &overlay,
                &notice,
                Some(artist.clone()),
                &casing_button,
                &artwork,
                &track_name_label,
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
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let casing_button = casing_button.clone();
        let artwork = artwork.clone();
        let track_name_label = track_name_label.clone();
        let artist = artist.clone();
        remove.connect_clicked(move |_| {
            request_remove_artist(
                &window,
                &state,
                &editors,
                &overlay,
                &notice,
                &casing_button,
                &artwork,
                &track_name_label,
                artist.clone(),
            );
        });
    }

    let actions = row_action_stack(edit, remove, 160);
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
    shell.append(&artist_tab_image_widget(&artist));
    row.set_child(Some(&shell));
    row
}

fn artist_stats_bubbles(track_count: usize) -> gtk::Box {
    let bubbles = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bubbles.add_css_class("artist-stats-bubbles");
    bubbles.set_hexpand(false);
    bubbles.set_vexpand(false);
    bubbles.set_halign(gtk::Align::Start);
    bubbles.set_valign(gtk::Align::End);
    let noun = if track_count == 1 { "track" } else { "tracks" };
    bubbles.append(&track_meta_bubble(&format!("{} {}", track_count, noun)));
    bubbles
}

fn artist_empty_row(window_height: i32) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("artist-empty-row");
    row.set_activatable(false);
    row.set_selectable(false);
    row.set_vexpand(true);
    row.set_valign(gtk::Align::Fill);
    let minimum_height = (window_height - 88).max(TRACK_ROW_HEIGHT);
    row.set_size_request(-1, minimum_height);

    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-empty-shell");
    shell.set_hexpand(true);
    shell.set_vexpand(true);
    shell.set_halign(gtk::Align::Fill);
    shell.set_valign(gtk::Align::Fill);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_halign(gtk::Align::Start);
    content.set_valign(gtk::Align::Start);
    content.set_margin_start(22);
    content.set_margin_end(16);
    content.set_margin_top(24);
    content.set_margin_bottom(16);

    let title = gtk::Label::new(Some("No artists available"));
    title.add_css_class("artist-empty-hero");
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Start);

    let subtitle = gtk::Label::new(Some("Create an artist to begin."));
    subtitle.add_css_class("artist-empty-invite");
    subtitle.set_xalign(0.0);
    subtitle.set_halign(gtk::Align::Start);
    subtitle.set_wrap(true);

    content.append(&title);
    content.append(&subtitle);

    shell.append(&content);
    row.set_child(Some(&shell));
    row
}

fn track_empty_row(window_height: i32) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0);
    row.add_css_class("track-list-row");
    row.add_css_class("artist-empty-row");
    row.set_hexpand(true);
    row.set_vexpand(true);
    row.set_valign(gtk::Align::Fill);
    let minimum_height = (window_height - 88).max(TRACK_ROW_HEIGHT);
    row.set_size_request(-1, minimum_height);

    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("artist-empty-shell");
    shell.set_hexpand(true);
    shell.set_vexpand(true);
    shell.set_halign(gtk::Align::Fill);
    shell.set_valign(gtk::Align::Fill);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_halign(gtk::Align::Start);
    content.set_valign(gtk::Align::Start);
    content.set_margin_start(22);
    content.set_margin_end(16);
    content.set_margin_top(24);
    content.set_margin_bottom(16);

    let title = gtk::Label::new(Some("No tracks available"));
    title.add_css_class("artist-empty-hero");
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Start);

    let subtitle = gtk::Label::new(Some("Create a track to begin."));
    subtitle.add_css_class("artist-empty-invite");
    subtitle.set_xalign(0.0);
    subtitle.set_halign(gtk::Align::Start);
    subtitle.set_wrap(true);

    content.append(&title);
    content.append(&subtitle);
    shell.append(&content);
    row.append(&shell);
    row
}

fn overlay_track_list_is_empty(overlay: &TrackOverlay) -> bool {
    overlay
        .scrolled
        .child()
        .is_some_and(|child| child.first_child().is_none())
}

pub fn startup_artist() -> Artist {
    ArtistStore::new_default()
        .and_then(|store| store.load())
        .ok()
        .and_then(|file| file.artists.into_iter().next())
        .unwrap_or_else(placeholder_artist)
}

fn placeholder_artist() -> Artist {
    Artist {
        id: PLACEHOLDER_ARTIST_ID.to_owned(),
        name: "No artist selected".to_owned(),
        description: String::new(),
        image: None,
    }
}

fn is_placeholder_artist(artist: &Artist) -> bool {
    artist.id == PLACEHOLDER_ARTIST_ID
}

fn request_remove_artist(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
    artist: Artist,
) {
    let message = format!("Remove artist \"{}\" from the catalog?", artist.name);
    let window_for_confirm = window.clone();
    let window = window.clone();
    let state = state.clone();
    let editors = editors.clone();
    let overlay = overlay.clone();
    let notice = notice.clone();
    let casing_button_for_remove = casing_button.clone();
    let artwork_for_remove = artwork.clone();
    let track_name_label_for_remove = track_name_label.clone();
    confirm::confirm_remove(&window_for_confirm, "Remove Artist", &message, move || {
        flush_current(&state, &editors, &notice);
        let store = match ArtistStore::new_default() {
            Ok(store) => store,
            Err(err) => {
                notifications::show_error(&notice, err.to_string());
                return;
            }
        };
        match store.remove_artist(&artist.id) {
            Ok(_) => {
                notifications::show_info(&notice, "Artist removed.");
                if state.borrow().artist.id == artist.id {
                    show_in_window(&window, startup_artist());
                } else {
                    show_artists_tab(
                        &window,
                        &state,
                        &editors,
                        &overlay,
                        &notice,
                        &casing_button_for_remove,
                        &artwork_for_remove,
                        &track_name_label_for_remove,
                    );
                }
            }
            Err(err) => notifications::show_error(&notice, err.to_string()),
        }
    });
}

fn show_artist_form(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    artist: Option<Artist>,
    casing_button: &gtk::Button,
    artwork: &gtk::Picture,
    track_name_label: &gtk::Label,
) {
    let is_edit = artist.is_some();
    overlay.clear_edit();
    overlay.show(window.width());

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some(if is_edit {
        "Edit Artist"
    } else {
        "Create Artist"
    }));
    title.add_css_class("pane-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let close = icon_button("close.svg", "Close artist editor");
    close.add_css_class("overlay-close-button");
    close.set_size_request(48, 48);
    close.set_halign(gtk::Align::End);
    header.append(&close);

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

    let (description_frame, description) = artist_description_field(
        artist
            .as_ref()
            .map(|artist| artist.description.as_str())
            .unwrap_or(""),
    );
    description_frame.set_hexpand(true);
    description_frame.set_halign(gtk::Align::Fill);

    let image_source: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let (image_picker, image_preview, image_placeholder) =
        artist_image_picker(artist.as_ref().and_then(|artist| artist.image.as_ref()));

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

    overlay.edit_box.append(&header);
    overlay.edit_box.append(&form);
    overlay.show_edit(true);

    {
        let overlay = overlay.clone();
        close.connect_clicked(move |_| overlay.clear_edit());
    }

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
        let window = window.clone();
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let name = name.clone();
        let description = description.clone();
        let image_source = image_source.clone();
        let error = error.clone();
        let artist = artist.clone();
        let casing_button_for_save = casing_button.clone();
        let artwork_for_save = artwork.clone();
        let track_name_label_for_save = track_name_label.clone();
        save.connect_clicked(move |_| {
            let store = match ArtistStore::new_default() {
                Ok(store) => store,
                Err(err) => {
                    notifications::show_error(&error, err.to_string());
                    return;
                }
            };
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
                    let should_open_saved = match artist.as_ref() {
                        Some(artist) => artist.id == state.borrow().artist.id,
                        None => true,
                    };
                    flush_current(&state, &editors, &notice);
                    overlay.clear_edit();
                    if should_open_saved {
                        show_in_window(&window, saved_artist);
                    } else {
                        show_artists_tab(
                            &window,
                            &state,
                            &editors,
                            &overlay,
                            &notice,
                            &casing_button_for_save,
                            &artwork_for_save,
                            &track_name_label_for_save,
                        );
                    }
                }
                Err(err) => notifications::show_error(&error, err.to_string()),
            }
        });
    }
}

fn artist_description_field(initial: &str) -> (gtk::Overlay, gtk::TextView) {
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

fn artist_tab_image_widget(artist: &Artist) -> gtk::Widget {
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

#[allow(clippy::too_many_arguments)]
fn show_settings_tab(
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    lower_left_casing_button: &gtk::Button,
    lower_left_font_combo: &gtk::ComboBoxText,
    ideas_workspace: &Rc<IdeasWorkspace>,
) {
    overlay.select_tab(OverlayTab::Settings);
    overlay.clear_settings();

    let font_combo = font_size_combo(state.borrow().app_settings.font_size_pt);
    font_combo.set_halign(gtk::Align::Start);

    let start_behavior_combo = start_behavior_combo(state.borrow().app_settings.start_behavior);
    start_behavior_combo.set_halign(gtk::Align::Start);

    let casing_combo = gtk::ComboBoxText::new();
    casing_combo.append_text("preserve");
    casing_combo.append_text("uppercase");
    casing_combo.append_text("lowercase");
    casing_combo.set_active(Some(casing_mode_index(
        state.borrow().app_settings.default_casing_mode,
    )));
    casing_combo.set_halign(gtk::Align::Start);

    let fullscreen_enabled = state.borrow().app_settings.fullscreen;
    let fullscreen_toggle = settings_icon_toggle_button(fullscreen_enabled);
    fullscreen_toggle.set_halign(gtk::Align::End);

    let settings_path = {
        let state_ref = state.borrow();
        state_ref.settings_store.path().display().to_string()
    };
    let settings_path_label = gtk::Label::new(Some(&settings_path));
    settings_path_label.add_css_class("settings-value");
    settings_path_label.set_xalign(0.0);
    settings_path_label.set_wrap(true);

    overlay
        .settings_box
        .append(&settings_row("editor font size", &font_combo));
    overlay
        .settings_box
        .append(&settings_row("start behaviour", &start_behavior_combo));
    overlay
        .settings_box
        .append(&settings_row("new track casing", &casing_combo));
    overlay
        .settings_box
        .append(&settings_row("fullscreen", &fullscreen_toggle));
    overlay
        .settings_box
        .append(&settings_row("settings file", &settings_path_label));

    {
        let state = state.clone();
        let notice = notice.clone();
        let window = window.clone();
        let fullscreen_state = Rc::new(Cell::new(fullscreen_enabled));
        let fullscreen_state_for_click = fullscreen_state.clone();
        let fullscreen_toggle_for_click = fullscreen_toggle.clone();
        fullscreen_toggle.connect_clicked(move |_| {
            let enabled = !fullscreen_state_for_click.get();
            fullscreen_state_for_click.set(enabled);
            set_settings_toggle_icon(&fullscreen_toggle_for_click, enabled);
            {
                let mut state_ref = state.borrow_mut();
                state_ref.app_settings.fullscreen = enabled;
                if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                    notifications::show_error(&notice, err.to_string());
                }
            }
            window_policy::set_fullscreen_enabled(&window, enabled);
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let lower_left_font_combo = lower_left_font_combo.clone();
        let ideas_workspace = ideas_workspace.clone();
        font_combo.connect_changed(move |combo| {
            let Some(text) = combo.active_text() else {
                return;
            };
            let Ok(font_size) = text.as_str().parse::<u16>() else {
                return;
            };
            if !VALID_FONT_SIZES.contains(&font_size) {
                return;
            }
            {
                let mut state_ref = state.borrow_mut();
                state_ref.app_settings.font_size_pt = font_size;
                if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                    notifications::show_error(&notice, err.to_string());
                }
            }
            editors.set_font_size(font_size);
            ideas_workspace.set_font_size(font_size);
            if let Some(index) = VALID_FONT_SIZES.iter().position(|size| *size == font_size) {
                lower_left_font_combo.set_active(Some(index as u32));
            }
            rebuild_material_ui(&state, &editors, &overlay, &notice);
        });
    }

    {
        let state = state.clone();
        let notice = notice.clone();
        start_behavior_combo.connect_changed(move |combo| {
            let Some(text) = combo.active_text() else {
                return;
            };
            let Some(behavior) = StartBehavior::from_label(text.as_str()) else {
                return;
            };
            let mut state_ref = state.borrow_mut();
            state_ref.app_settings.start_behavior = behavior;
            if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                notifications::show_error(&notice, err.to_string());
            }
        });
    }

    {
        let state = state.clone();
        let editors = editors.clone();
        let overlay = overlay.clone();
        let notice = notice.clone();
        let lower_left_casing_button = lower_left_casing_button.clone();
        let ideas_workspace = ideas_workspace.clone();
        casing_combo.connect_changed(move |combo| {
            let Some(index) = combo.active() else {
                return;
            };
            let Some(mode) = casing_mode_from_index(index) else {
                return;
            };
            let mut state_ref = state.borrow_mut();
            state_ref.app_settings.default_casing_mode = mode;
            state_ref.draft_casing_mode = mode;
            if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                notifications::show_error(&notice, err.to_string());
            }
            let should_apply_to_draft = state_ref.current.is_none();
            drop(state_ref);

            if should_apply_to_draft {
                let final_text = apply_casing(&editors.final_text(), mode);
                {
                    let mut state_ref = state.borrow_mut();
                    state_ref.programmatic_text_change = true;
                }
                replace_buffer_text_preserving_cursor(&editors.final_buffer, &final_text);
                state.borrow_mut().programmatic_text_change = false;
                rebuild_material_ui(&state, &editors, &overlay, &notice);
            }
            ideas_workspace.set_default_casing(mode);
            update_casing_button(&state, &lower_left_casing_button);
        });
    }
}

fn settings_row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("settings-row");
    let name = gtk::Label::new(Some(label));
    name.add_css_class("settings-label");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_valign(gtk::Align::Fill);
    let control_widget: &gtk::Widget = control.as_ref();
    control_widget.add_css_class("settings-control");
    control_widget.set_halign(gtk::Align::End);
    control_widget.set_valign(gtk::Align::Fill);
    row.append(&name);
    row.append(control);
    row
}

fn casing_mode_index(mode: CasingMode) -> u32 {
    match mode {
        CasingMode::Preserve => 0,
        CasingMode::Uppercase => 1,
        CasingMode::Lowercase => 2,
    }
}

fn casing_mode_from_index(index: u32) -> Option<CasingMode> {
    match index {
        0 => Some(CasingMode::Preserve),
        1 => Some(CasingMode::Uppercase),
        2 => Some(CasingMode::Lowercase),
        _ => None,
    }
}

fn show_info_tab(overlay: &Rc<TrackOverlay>, root_overlay: &gtk::Overlay) {
    overlay.select_tab(OverlayTab::Info);
    overlay.clear_info();

    let info_view = gtk::Box::new(gtk::Orientation::Vertical, 0);
    info_view.add_css_class("info-view");
    info_view.set_hexpand(true);
    info_view.set_vexpand(false);
    info_view.set_overflow(gtk::Overflow::Hidden);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    header.add_css_class("info-header");
    header.set_hexpand(true);
    header.set_vexpand(false);
    header.append(&info_splash_preview(root_overlay));
    info_view.append(&header);

    let sections = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sections.add_css_class("info-sections");
    append_info_metric_sections(&sections, &build_information_metrics());
    info_view.append(&sections);

    let viewport = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(false)
        .propagate_natural_width(false)
        .hexpand(true)
        .vexpand(true)
        .child(&info_view)
        .build();
    viewport.add_css_class("info-viewport");
    overlay.info_box.append(&viewport);
}

fn info_splash_preview(root_overlay: &gtk::Overlay) -> gtk::Widget {
    let frame = gtk::Overlay::new();
    frame.add_css_class("info-splash-frame");
    frame.set_size_request(INFO_SPLASH_PREVIEW_WIDTH, INFO_SPLASH_PREVIEW_HEIGHT);
    frame.set_hexpand(true);
    frame.set_vexpand(false);
    frame.set_halign(gtk::Align::Fill);
    frame.set_valign(gtk::Align::Start);
    frame.set_overflow(gtk::Overflow::Hidden);

    let picture = gtk::Picture::for_filename(splash::splash_path());
    picture.add_css_class("info-splash-image");
    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);
    picture.set_size_request(INFO_SPLASH_PREVIEW_WIDTH, INFO_SPLASH_PREVIEW_HEIGHT);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    frame.set_child(Some(&picture));

    {
        let root_overlay = root_overlay.clone();
        let click_count = Rc::new(Cell::new(0u8));
        let click_count_for_trigger = click_count.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let count = click_count_for_trigger.get().saturating_add(1);
            if count >= 5 {
                click_count_for_trigger.set(0);
                show_credits_easter_egg(&root_overlay);
            } else {
                click_count_for_trigger.set(count);
            }
        });
        frame.add_controller(click);
    }

    let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shade.add_css_class("info-splash-shade");
    shade.set_hexpand(true);
    shade.set_vexpand(true);
    frame.add_overlay(&shade);

    frame.upcast()
}

fn append_info_metric_sections(parent: &gtk::Box, metrics: &[InfoMetric]) {
    for section in ["Application", "Runtime", "System"] {
        let section_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        section_box.add_css_class("info-section");
        section_box.set_hexpand(true);

        let title = gtk::Label::new(Some(section));
        title.add_css_class("info-section-title");
        title.set_xalign(0.0);
        section_box.append(&title);

        let grid = gtk::Grid::new();
        grid.add_css_class("info-metric-grid");
        grid.set_column_spacing(18);
        grid.set_row_spacing(6);
        grid.set_hexpand(true);

        for (row, metric) in metrics
            .iter()
            .filter(|metric| metric.section == section)
            .enumerate()
        {
            let label = gtk::Label::new(Some(info_metric_display_label(metric.label)));
            label.add_css_class("info-metric-label");
            label.set_xalign(0.0);
            label.set_valign(gtk::Align::Start);

            let value = gtk::Label::new(Some(&metric.value));
            value.add_css_class("info-metric-value");
            value.set_xalign(0.0);
            value.set_hexpand(true);
            value.set_selectable(true);
            value.set_wrap(true);

            grid.attach(&label, 0, row as i32, 1, 1);
            grid.attach(&value, 1, row as i32, 1, 1);
        }

        section_box.append(&grid);
        parent.append(&section_box);
    }
}

fn info_metric_display_label(label: &str) -> &str {
    match label {
        "version" => "Version",
        "profile" => "Profile",
        "target" => "Target",
        "gtk" => "GTK",
        "loaded libraries" => "Loaded Libraries",
        "build size" => "Build Size",
        "memory" => "Memory",
        _ => label,
    }
}

#[cfg(test)]
fn build_information_lines() -> Vec<String> {
    build_information_metrics()
        .into_iter()
        .map(|metric| format!("{} {}", metric.label, metric.value))
        .collect()
}

fn build_information_metrics() -> Vec<InfoMetric> {
    vec![
        InfoMetric {
            section: "Application",
            label: "version",
            value: env!("CARGO_PKG_VERSION").to_owned(),
        },
        InfoMetric {
            section: "Application",
            label: "profile",
            value: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_owned(),
        },
        InfoMetric {
            section: "System",
            label: "target",
            value: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        },
        InfoMetric {
            section: "Runtime",
            label: "gtk",
            value: format!(
                "{}.{}.{}",
                gtk::major_version(),
                gtk::minor_version(),
                gtk::micro_version()
            ),
        },
        InfoMetric {
            section: "Runtime",
            label: "loaded libraries",
            value: loaded_library_count_label(),
        },
        InfoMetric {
            section: "System",
            label: "build size",
            value: current_executable_size_label(),
        },
        InfoMetric {
            section: "System",
            label: "memory",
            value: current_memory_label(),
        },
    ]
}

fn loaded_library_count_label() -> String {
    loaded_library_count()
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn loaded_library_count() -> Option<usize> {
    let maps = fs::read_to_string("/proc/self/maps").ok()?;
    let libraries = maps
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter(|path| path.contains(".so"))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    Some(libraries.len())
}

fn current_executable_size_label() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| format_bytes(metadata.len()))
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn current_memory_label() -> String {
    let Some((rss_kib, vms_kib)) = current_memory_kib() else {
        return "unavailable".to_owned();
    };
    format!("rss {} / vms {}", format_kib(rss_kib), format_kib(vms_kib))
}

fn current_memory_kib() -> Option<(u64, u64)> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let rss = status_value_kib(&status, "VmRSS:")?;
    let vms = status_value_kib(&status, "VmSize:")?;
    Some((rss, vms))
}

fn status_value_kib(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.trim();
        rest.split_whitespace().next()?.parse::<u64>().ok()
    })
}

fn format_kib(kib: u64) -> String {
    format_bytes(kib.saturating_mul(1024))
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn show_credits_easter_egg(root_overlay: &gtk::Overlay) {
    let layer = splash::view();
    layer.add_css_class("credits-easteregg");
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    layer.set_halign(gtk::Align::Fill);
    layer.set_valign(gtk::Align::Fill);

    let flights = Rc::new(credit_flights(&credits_names()));
    let started = Rc::new(Instant::now());
    let credits_layer = gtk::DrawingArea::new();
    credits_layer.add_css_class("credits-swarm");
    credits_layer.set_hexpand(true);
    credits_layer.set_vexpand(true);
    credits_layer.set_halign(gtk::Align::Fill);
    credits_layer.set_valign(gtk::Align::Fill);
    {
        let flights = flights.clone();
        let started = started.clone();
        credits_layer.set_draw_func(move |_, cr, width, height| {
            let progress = (started.elapsed().as_secs_f64() / CREDITS_DURATION_SECS).min(1.0);
            draw_credit_flights(cr, &flights, width as f64, height as f64, progress);
        });
    }
    layer.add_overlay(&credits_layer);

    root_overlay.add_overlay(&layer);

    let closed = Rc::new(Cell::new(false));
    {
        let root_overlay = root_overlay.clone();
        let layer_for_click = layer.clone();
        let closed = closed.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            close_credits_easter_egg(&root_overlay, &layer_for_click, &closed);
        });
        layer.add_controller(click);
    }

    let root_overlay = root_overlay.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
        if closed.get() {
            return gtk::glib::ControlFlow::Break;
        }

        let progress = (started.elapsed().as_secs_f64() / CREDITS_DURATION_SECS).min(1.0);
        credits_layer.queue_draw();

        if progress >= 1.0 {
            close_credits_easter_egg(&root_overlay, &layer, &closed);
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn close_credits_easter_egg(
    root_overlay: &gtk::Overlay,
    layer: &gtk::Overlay,
    closed: &Rc<Cell<bool>>,
) {
    if closed.replace(true) {
        return;
    }
    root_overlay.remove_overlay(layer);
}

fn credit_flights(names: &[&'static str]) -> Vec<CreditFlight> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| CreditFlight {
            name,
            font_size_pt: random_between(name, index, 11, CREDIT_FONT_MIN_PT, CREDIT_FONT_MAX_PT),
            alpha: random_between(name, index, 23, 0.24, 0.68),
            lane: random_between(name, index, 37, 0.16, 0.84),
            delay: random_between(name, index, 41, 0.0, 0.36),
            phase: random_between(name, index, 53, 0.0, TAU),
            swirl: random_between(name, index, 67, 0.72, 1.55),
        })
        .collect()
}

fn draw_credit_flights(
    cr: &gtk::cairo::Context,
    flights: &[CreditFlight],
    width: f64,
    height: f64,
    progress: f64,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }

    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Bold,
    );

    for flight in flights {
        let local = local_credit_progress(progress, flight.delay);
        let font_px = flight.font_size_pt * POINT_TO_PIXEL;
        cr.set_font_size(font_px);

        let extents = cr
            .text_extents(flight.name)
            .unwrap_or_else(|_| gtk::cairo::TextExtents::new(0.0, 0.0, font_px, font_px, 0.0, 0.0));
        let text_width = extents.width().max(font_px);
        let text_height = extents.height().max(font_px * 0.7);
        let eased = ease_in_out(local);
        let travel = width + text_width + font_px * 2.0;
        let swirl_x = (local * TAU * (1.0 + flight.swirl) + flight.phase).sin() * font_px * 0.42;
        let x = -text_width - font_px + eased * travel + swirl_x;
        let base_y = height * flight.lane;
        let amplitude = (height * 0.13 * flight.swirl).clamp(font_px * 0.35, height * 0.32);
        let swirl_y = (local * TAU * (2.0 + flight.swirl) + flight.phase).sin() * amplitude
            + (local * TAU * 4.0 + flight.phase * 0.7).cos() * amplitude * 0.34;
        let y = (base_y + swirl_y).clamp(text_height, height - font_px * 0.25);
        let fade = credit_fade(local);

        cr.set_source_rgba(1.0, 1.0, 1.0, flight.alpha * fade);
        cr.move_to(x - extents.x_bearing(), y);
        cr.show_text(flight.name).ok();
    }
}

fn local_credit_progress(progress: f64, delay: f64) -> f64 {
    let delay = delay.clamp(0.0, 0.86);
    if progress <= delay {
        0.0
    } else {
        ((progress - delay) / (1.0 - delay)).clamp(0.0, 1.0)
    }
}

fn credit_fade(local_progress: f64) -> f64 {
    (local_progress.clamp(0.0, 1.0) * PI).sin().clamp(0.0, 1.0)
}

fn random_between(name: &str, index: usize, salt: u64, min: f64, max: f64) -> f64 {
    min + seeded_unit(name, index, salt) * (max - min)
}

fn seeded_unit(name: &str, index: usize, salt: u64) -> f64 {
    let mut hash = 0xcbf29ce484222325_u64 ^ salt ^ index as u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51afd7ed558ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
    hash ^= hash >> 33;
    hash as f64 / u64::MAX as f64
}

fn credits_names() -> Vec<&'static str> {
    include_str!("../resources/credits")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn ease_in_out(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        2.0 * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0
    }
}

fn show_exit_tab(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    notice: &gtk::Label,
) {
    overlay.select_tab(OverlayTab::Exit);
    overlay.clear_exit();

    let title = gtk::Label::new(Some("Exit application?"));
    title.add_css_class("tab-action-question");
    title.add_css_class("exit-tab-question");
    title.set_xalign(1.0);
    title.set_valign(gtk::Align::Center);

    let cancel = icon_text_button("close.svg", "CANCEL");
    cancel.add_css_class("tab-action-button");
    cancel.add_css_class("exit-tab-action");
    cancel.set_valign(gtk::Align::Fill);

    let confirm = icon_text_button("remove.svg", "EXIT");
    confirm.add_css_class("tab-action-button");
    confirm.add_css_class("danger-button");
    confirm.add_css_class("exit-tab-danger");
    confirm.add_css_class("tab-action-last");
    confirm.add_css_class("exit-tab-action");
    confirm.set_valign(gtk::Align::Fill);

    overlay.clear_tab_actions();
    overlay.append_tab_action(&title);
    overlay.append_tab_action(&cancel);
    overlay.append_tab_action(&confirm);

    {
        let overlay = overlay.clone();
        cancel.connect_clicked(move |_| {
            overlay.select_tab(OverlayTab::Tracks);
        });
    }

    {
        let window = window.clone();
        let state = state.clone();
        let editors = editors.clone();
        let notice = notice.clone();
        confirm.connect_clicked(move |_| {
            exit_application(&window, &state, &editors, &notice);
        });
    }
}

fn show_track_edit(
    state: &Rc<RefCell<MainState>>,
    overlay: &Rc<TrackOverlay>,
    _notice: &gtk::Label,
    window: &gtk::ApplicationWindow,
    artwork_picture: &gtk::Picture,
    track_name_label: &gtk::Label,
    item: TrackListItem,
) {
    overlay.clear_edit();
    overlay.show(window.width());
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Track Details"));
    title.add_css_class("pane-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let name = gtk::Entry::builder()
        .placeholder_text("Track name")
        .text(&item.settings.name)
        .build();
    name.add_css_class("form-field");
    name.add_css_class("name-field");
    name.set_hexpand(true);
    name.set_halign(gtk::Align::Fill);
    let tempo = gtk::Entry::builder()
        .placeholder_text("Tempo")
        .text(item.settings.tempo.to_string())
        .build();
    tempo.add_css_class("form-field");
    tempo.set_hexpand(true);
    tempo.set_halign(gtk::Align::Fill);
    let length = gtk::Entry::builder()
        .placeholder_text("Length")
        .text(&item.settings.length)
        .build();
    length.add_css_class("form-field");
    length.set_hexpand(true);
    length.set_halign(gtk::Align::Fill);
    let (artwork_picker, artwork_preview, artwork_placeholder) =
        track_artwork_picker(item.settings.artwork.as_ref(), "Choose artwork");

    let fields = gtk::Box::new(gtk::Orientation::Vertical, 10);
    fields.set_hexpand(true);
    fields.set_halign(gtk::Align::Fill);
    fields.append(&name);
    fields.append(&tempo);
    fields.append(&length);

    let content_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content_row.set_hexpand(true);
    content_row.set_halign(gtk::Align::Fill);
    content_row.append(&fields);
    content_row.append(&artwork_picker);

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
    let done = icon_button("done.svg", "Done");
    done.add_css_class("overlay-close-button");
    done.add_css_class("track-editor-done-button");
    done.set_size_request(48, 48);
    done.set_halign(gtk::Align::End);
    done.set_valign(gtk::Align::End);
    bottom_bar.append(&bottom_spacer);
    bottom_bar.append(&done);

    overlay.edit_box.append(&header);
    overlay.edit_box.append(&content_row);
    overlay.edit_box.append(&error);
    overlay.edit_box.append(&bottom_bar);
    overlay.show_edit(true);

    {
        let overlay = overlay.clone();
        done.connect_clicked(move |_| overlay.clear_edit());
    }

    let artwork_path: Rc<RefCell<Option<PathBuf>>> =
        Rc::new(RefCell::new(item.settings.artwork.clone()));
    let saving = Rc::new(Cell::new(false));
    let save_metadata: Rc<dyn Fn()> = {
        let state = state.clone();
        let name = name.clone();
        let tempo = tempo.clone();
        let length = length.clone();
        let artwork_path = artwork_path.clone();
        let error = error.clone();
        let paths = item.paths.clone();
        let saving = saving.clone();
        let artwork_picture = artwork_picture.clone();
        let track_name_label = track_name_label.clone();
        Rc::new(move || {
            if saving.get() {
                return;
            }
            saving.set(true);
            let result = (|| {
                let tempo_value = parse_tempo(&tempo)?;
                let name_value = validate_name(&name.text(), "track.name")?;
                validate_length(&length.text())?;
                let artwork = artwork_path.borrow().clone();
                let store = state.borrow().track_store.clone();
                let mut current_save = None;
                {
                    let mut state_ref = state.borrow_mut();
                    if let Some(open) = state_ref
                        .current
                        .as_mut()
                        .filter(|open| open.settings.id == item.settings.id)
                    {
                        open.settings.name = name_value.clone();
                        open.settings.tempo = tempo_value;
                        open.settings.length = length.text().to_string();
                        open.settings.artwork = artwork.clone();
                        current_save = Some((open.paths.clone(), open.settings.clone()));
                    }
                }
                if let Some((paths, settings)) = current_save {
                    store.save_settings(&paths, &settings)?;
                } else {
                    let mut settings = store.load_settings(&paths)?;
                    settings.name = name_value;
                    settings.tempo = tempo_value;
                    settings.length = length.text().to_string();
                    settings.artwork = artwork;
                    store.save_settings(&paths, &settings)?;
                }
                Ok::<(), AppError>(())
            })();
            match result {
                Ok(()) => {
                    notifications::clear(&error);
                    update_artwork(&state, &artwork_picture);
                    update_track_name_label(&state, &track_name_label);
                }
                Err(err) => notifications::show_error(&error, err.to_string()),
            }
            saving.set(false);
        })
    };

    {
        let save = save_metadata.clone();
        name.connect_changed(move |_| save());
    }
    {
        let save = save_metadata.clone();
        length.connect_changed(move |_| save());
    }
    {
        let save = save_metadata.clone();
        tempo.connect_changed(move |_| save());
    }
    {
        let window = window.clone();
        let artwork_path = artwork_path.clone();
        let artwork_preview = artwork_preview.clone();
        let artwork_placeholder = artwork_placeholder.clone();
        let save = save_metadata.clone();
        let error = error.clone();
        let import_paths = item.paths.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let chooser = gtk::FileChooserNative::new(
                Some("Choose track artwork"),
                Some(&window),
                gtk::FileChooserAction::Open,
                Some("Choose"),
                Some("Cancel"),
            );
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            chooser.add_filter(&filter);
            let artwork_path = artwork_path.clone();
            let artwork_preview = artwork_preview.clone();
            let artwork_placeholder = artwork_placeholder.clone();
            let save = save.clone();
            let error = error.clone();
            let import_paths = import_paths.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        match import_track_artwork(&path, &import_paths) {
                            Ok(processed_path) => {
                                artwork_preview.set_from_file(Some(&processed_path));
                                artwork_placeholder.set_visible(false);
                                *artwork_path.borrow_mut() = Some(processed_path);
                                save();
                            }
                            Err(err) => notifications::show_error(&error, err.to_string()),
                        }
                    }
                }
                chooser.destroy();
            });
            chooser.show();
        });
        artwork_picker.add_controller(click);
    }
}

fn show_search_panel(
    state: &Rc<RefCell<MainState>>,
    root_overlay: &gtk::Overlay,
    editors: &Rc<EditorPanes>,
    search_revealer: &gtk::Revealer,
    viewport_width: i32,
) {
    clear_search_marker_layer(state, root_overlay);

    editors
        .final_view
        .set_bottom_margin(SEARCH_SCROLL_BOTTOM_PADDING_PX);
    editors
        .raw_view
        .set_bottom_margin(SEARCH_SCROLL_BOTTOM_PADDING_PX);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.add_css_class("search-panel");
    content.set_halign(gtk::Align::Fill);
    content.set_valign(gtk::Align::Start);
    content.set_hexpand(true);
    content.set_height_request(36);
    content.set_margin_top(0);
    content.set_margin_start(0);
    content.set_margin_end(0);
    content.set_margin_bottom(0);

    let left_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    left_spacer.set_hexpand(true);

    let centered_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    centered_row.add_css_class("search-panel-row");
    centered_row.set_halign(gtk::Align::End);
    centered_row.set_valign(gtk::Align::Fill);
    centered_row.set_vexpand(true);
    centered_row.set_margin_top(0);

    let query = gtk::Entry::builder().placeholder_text("Search").build();
    query.add_css_class("search-field");
    query.set_hexpand(false);
    query.set_halign(gtk::Align::Start);
    let max_query_width = ((viewport_width.max(320) as f64) * 0.225).round() as i32;
    query.set_size_request(max_query_width, -1);

    let fuzzy_active = Rc::new(Cell::new(false));
    let fuzzy = gtk::Button::with_label("F");
    fuzzy.add_css_class("search-fuzzy-button");
    fuzzy.set_can_focus(false);
    fuzzy.set_tooltip_text(Some("Fuzzy search"));
    fuzzy.set_halign(gtk::Align::Start);
    fuzzy.set_valign(gtk::Align::Fill);
    fuzzy.set_vexpand(true);

    centered_row.append(&query);
    centered_row.append(&fuzzy);
    content.append(&left_spacer);
    content.append(&centered_row);

    let final_matches: Rc<RefCell<Vec<SearchMatch>>> = Rc::new(RefCell::new(Vec::new()));
    let raw_matches: Rc<RefCell<Vec<SearchMatch>>> = Rc::new(RefCell::new(Vec::new()));
    let marker_pulse = Rc::new(Cell::new(1.0f64));

    let marker_layer = gtk::DrawingArea::new();
    marker_layer.add_css_class("search-marker-layer");
    marker_layer.set_can_target(false);
    marker_layer.set_halign(gtk::Align::Fill);
    marker_layer.set_valign(gtk::Align::Fill);
    marker_layer.set_hexpand(true);
    marker_layer.set_vexpand(true);
    root_overlay.add_overlay(&marker_layer);
    state.borrow_mut().search_marker_layer = Some(marker_layer.clone());

    let active_pane = Rc::new(Cell::new(state.borrow().last_focus));
    let final_index = Rc::new(Cell::new(0usize));
    let raw_index = Rc::new(Cell::new(0usize));
    let final_wrap_pending = Rc::new(Cell::new(false));
    let raw_wrap_pending = Rc::new(Cell::new(false));

    let editors_for_search = editors.clone();

    let refresh: Rc<dyn Fn()> = {
        let final_buffer = editors_for_search.final_buffer.clone();
        let raw_buffer = editors_for_search.raw_buffer.clone();
        let query = query.clone();
        let fuzzy_active = fuzzy_active.clone();
        let final_matches = final_matches.clone();
        let raw_matches = raw_matches.clone();
        let final_index = final_index.clone();
        let raw_index = raw_index.clone();
        let final_wrap_pending = final_wrap_pending.clone();
        let raw_wrap_pending = raw_wrap_pending.clone();

        Rc::new(move || {
            let options = SearchOptions {
                case_sensitive: false,
                fuzzy: fuzzy_active.get(),
            };

            let found_final = find_matches(&buffer_text(&final_buffer), &query.text(), &options);
            let found_raw = find_matches(&buffer_text(&raw_buffer), &query.text(), &options);

            apply_search_highlights(&final_buffer, &found_final);
            apply_search_highlights(&raw_buffer, &found_raw);

            final_index.set(0);
            raw_index.set(0);
            final_wrap_pending.set(false);
            raw_wrap_pending.set(false);

            *final_matches.borrow_mut() = found_final;
            *raw_matches.borrow_mut() = found_raw;

            if !final_matches.borrow().is_empty() {
                focus_search_match(
                    &editors_for_search,
                    PaneFocus::Final,
                    &final_matches.borrow(),
                    &final_index,
                    &final_wrap_pending,
                    0,
                    None,
                );
            }
            if !raw_matches.borrow().is_empty() {
                focus_search_match(
                    &editors_for_search,
                    PaneFocus::Raw,
                    &raw_matches.borrow(),
                    &raw_index,
                    &raw_wrap_pending,
                    0,
                    None,
                );
            }
        })
    };

    {
        let editors = editors.clone();
        let final_matches = final_matches.clone();
        let raw_matches = raw_matches.clone();
        let marker_pulse = marker_pulse.clone();
        marker_layer.set_draw_func(move |layer, cr, _w, h| {
            draw_search_breakpoint_markers(
                layer,
                cr,
                h as f64,
                &editors.final_view,
                &editors.raw_view,
                &final_matches.borrow(),
                &raw_matches.borrow(),
                marker_pulse.get(),
            );
        });
    }

    {
        let refresh = refresh.clone();
        query.connect_changed(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        let fuzzy_active = fuzzy_active.clone();
        let fuzzy = fuzzy.clone();
        fuzzy.clone().connect_clicked(move |_| {
            let active = !fuzzy_active.get();
            fuzzy_active.set(active);
            if active {
                fuzzy.add_css_class("active");
            } else {
                fuzzy.remove_css_class("active");
            }
            refresh();
        });
    }

    {
        let editors = editors.clone();
        let final_matches = final_matches.clone();
        let raw_matches = raw_matches.clone();
        let active_pane = active_pane.clone();
        let final_index = final_index.clone();
        let raw_index = raw_index.clone();
        let final_wrap_pending = final_wrap_pending.clone();
        let raw_wrap_pending = raw_wrap_pending.clone();

        query.connect_activate(move |_| {
            advance_search_match(
                &editors,
                &final_matches,
                &raw_matches,
                &active_pane,
                &final_index,
                &raw_index,
                &final_wrap_pending,
                &raw_wrap_pending,
                1,
                None,
            );
        });
    }

    search_revealer.set_child(Some(&content));
    search_revealer.set_reveal_child(true);

    {
        let search_revealer = search_revealer.clone();
        let marker_layer = marker_layer.clone();
        let marker_pulse = marker_pulse.clone();
        let phase = Rc::new(Cell::new(0.0f64));
        let phase_for_tick = phase.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(120), move || {
            if !search_revealer.reveals_child() {
                return gtk::glib::ControlFlow::Break;
            }
            let next_phase = (phase_for_tick.get() + 0.95) % TAU;
            phase_for_tick.set(next_phase);
            let pulse = ((next_phase.sin() * 0.5) + 0.5).powf(1.25).clamp(0.15, 1.0);
            marker_pulse.set(pulse);
            marker_layer.queue_draw();
            gtk::glib::ControlFlow::Continue
        });
    }

    query.grab_focus();
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

#[allow(clippy::too_many_arguments)]
fn draw_search_breakpoint_markers(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    height: f64,
    final_view: &gtk::TextView,
    raw_view: &gtk::TextView,
    final_matches: &[SearchMatch],
    raw_matches: &[SearchMatch],
    pulse: f64,
) {
    for mat in final_matches {
        draw_breakpoint_marker_for_match(layer, cr, height, final_view, mat, pulse);
    }
    for mat in raw_matches {
        draw_breakpoint_marker_for_match(layer, cr, height, raw_view, mat, pulse);
    }
}

fn draw_breakpoint_marker_for_match(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    height: f64,
    view: &gtk::TextView,
    mat: &SearchMatch,
    pulse: f64,
) {
    let buffer = view.buffer();
    let iter = buffer.iter_at_offset(mat.start as i32);
    let location = view.iter_location(&iter);
    let (line_y, line_height) = view.line_yrange(&iter);
    let marker_gap = 5.0;
    let marker_radius = 3.2 + pulse * 0.8;
    let buffer_x = location.x();
    let buffer_y = line_y + line_height / 2;
    let (window_x, window_y) =
        view.buffer_to_window_coords(gtk::TextWindowType::Widget, buffer_x, buffer_y);
    let Some((layer_x, layer_y)) =
        view.translate_coordinates(layer, window_x as f64, window_y as f64)
    else {
        return;
    };
    if layer_y < -10.0 || layer_y > height + 10.0 {
        return;
    }

    let cx = layer_x - marker_gap - marker_radius;
    let cy = layer_y;

    cr.set_source_rgba(0.82, 0.05, 1.0, 0.34 + 0.66 * pulse);
    cr.arc(cx, cy, marker_radius, 0.0, TAU);
    cr.fill().ok();
}

fn clear_search_marker_layer(state: &Rc<RefCell<MainState>>, root_overlay: &gtk::Overlay) {
    if let Some(layer) = state.borrow_mut().search_marker_layer.take() {
        root_overlay.remove_overlay(&layer);
    }
}

fn focus_and_center_search_match(editors: &EditorPanes, pane: PaneFocus, mat: &SearchMatch) {
    clear_active_search_highlight(&editors.final_buffer);
    clear_active_search_highlight(&editors.raw_buffer);

    let (buffer, view) = match pane {
        PaneFocus::Final => (&editors.final_buffer, &editors.final_view),
        PaneFocus::Raw => (&editors.raw_buffer, &editors.raw_view),
    };

    let start = buffer.iter_at_offset(mat.start as i32);
    let end = buffer.iter_at_offset(mat.end as i32);

    let active_tag = ensure_active_search_tag(buffer);
    buffer.apply_tag(&active_tag, &start, &end);
    buffer.select_range(&start, &end);

    let mut scroll_iter = start;
    view.scroll_to_iter(&mut scroll_iter, 0.15, true, 0.5, 0.5);
}

fn focus_search_match(
    editors: &EditorPanes,
    pane: PaneFocus,
    matches: &[SearchMatch],
    index_cell: &Rc<Cell<usize>>,
    wrap_pending_cell: &Rc<Cell<bool>>,
    index: usize,
    status_label: Option<&gtk::Label>,
) {
    if matches.is_empty() {
        return;
    }

    let target_index = index.min(matches.len().saturating_sub(1));
    index_cell.set(target_index);
    wrap_pending_cell.set(false);

    focus_and_center_search_match(editors, pane, &matches[target_index]);

    if let Some(label) = status_label {
        notifications::clear(label);
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_search_match(
    editors: &EditorPanes,
    final_matches: &Rc<RefCell<Vec<SearchMatch>>>,
    raw_matches: &Rc<RefCell<Vec<SearchMatch>>>,
    active_pane: &Rc<Cell<PaneFocus>>,
    final_index: &Rc<Cell<usize>>,
    raw_index: &Rc<Cell<usize>>,
    final_wrap_pending: &Rc<Cell<bool>>,
    raw_wrap_pending: &Rc<Cell<bool>>,
    direction: isize,
    status_label: Option<&gtk::Label>,
) {
    let final_matches = final_matches.borrow();
    let raw_matches = raw_matches.borrow();

    let pane = if editors.final_view.has_focus() {
        PaneFocus::Final
    } else if editors.raw_view.has_focus() {
        PaneFocus::Raw
    } else {
        active_pane.get()
    };
    active_pane.set(pane);

    let advanced = match pane {
        PaneFocus::Final => advance_search_for_pane(
            editors,
            PaneFocus::Final,
            &final_matches[..],
            final_index,
            final_wrap_pending,
            direction,
            status_label,
        ),
        PaneFocus::Raw => advance_search_for_pane(
            editors,
            PaneFocus::Raw,
            &raw_matches[..],
            raw_index,
            raw_wrap_pending,
            direction,
            status_label,
        ),
    };

    if advanced {
        return;
    }

    let fallback_advanced = match pane {
        PaneFocus::Final => advance_search_for_pane(
            editors,
            PaneFocus::Raw,
            &raw_matches[..],
            raw_index,
            raw_wrap_pending,
            direction,
            status_label,
        ),
        PaneFocus::Raw => advance_search_for_pane(
            editors,
            PaneFocus::Final,
            &final_matches[..],
            final_index,
            final_wrap_pending,
            direction,
            status_label,
        ),
    };

    if fallback_advanced {
        let fallback_pane = match pane {
            PaneFocus::Final => PaneFocus::Raw,
            PaneFocus::Raw => PaneFocus::Final,
        };
        active_pane.set(fallback_pane);
    }
}

fn advance_search_for_pane(
    editors: &EditorPanes,
    pane: PaneFocus,
    matches: &[SearchMatch],
    index_cell: &Rc<Cell<usize>>,
    wrap_pending_cell: &Rc<Cell<bool>>,
    direction: isize,
    _status_label: Option<&gtk::Label>,
) -> bool {
    if matches.is_empty() {
        return false;
    }

    let current_index = index_cell.get();
    let next_index = next_search_index(
        current_index,
        matches.len(),
        direction,
        wrap_pending_cell.get(),
    );

    match next_index {
        Some(index) => {
            index_cell.set(index);
            wrap_pending_cell.set(false);
            let mat = &matches[index];
            focus_and_center_search_match(editors, pane, mat);
            true
        }
        None => {
            wrap_pending_cell.set(true);
            false
        }
    }
}

fn next_search_index(
    current_index: usize,
    match_count: usize,
    direction: isize,
    wrap_pending: bool,
) -> Option<usize> {
    if match_count == 0 {
        return None;
    }

    if direction >= 0 {
        if current_index + 1 < match_count {
            Some(current_index + 1)
        } else if wrap_pending {
            Some(0)
        } else {
            None
        }
    } else if current_index > 0 {
        Some(current_index - 1)
    } else if wrap_pending {
        Some(match_count.saturating_sub(1))
    } else {
        None
    }
}

#[cfg(test)]
mod search_tests {
    use super::next_search_index;

    #[test]
    fn next_search_index_wraps_after_end() {
        assert_eq!(next_search_index(1, 2, 1, false), None);
        assert_eq!(next_search_index(1, 2, 1, true), Some(0));
    }

    #[test]
    fn next_search_index_wraps_after_beginning() {
        assert_eq!(next_search_index(0, 2, -1, false), None);
        assert_eq!(next_search_index(0, 2, -1, true), Some(1));
    }
}

fn parse_tempo(entry: &gtk::Entry) -> Result<u16, AppError> {
    let value = entry
        .text()
        .trim()
        .parse::<u16>()
        .map_err(|_| AppError::validation("tempo", "must be an integer BPM value"))?;
    validate_tempo(value)?;
    Ok(value)
}

fn copy_plain_text_selection(view: &gtk::TextView) -> bool {
    let buffer = view.buffer();
    let Some((start, end)) = buffer.selection_bounds() else {
        return false;
    };

    let text = buffer.text(&start, &end, true);
    view.clipboard().set_text(text.as_str());
    true
}

fn cut_plain_text_selection(view: &gtk::TextView) -> bool {
    if !copy_plain_text_selection(view) {
        return false;
    }

    view.buffer().delete_selection(true, view.is_editable());
    true
}

fn paste_plain_text(view: &gtk::TextView) {
    let clipboard = view.clipboard();
    let view = view.clone();
    clipboard.read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
        let Ok(Some(text)) = result else {
            return;
        };
        if text.is_empty() {
            return;
        }

        let buffer = view.buffer();
        let mut insertion = text.to_string();
        if !insertion.ends_with('\n') {
            insertion.push('\n');
        }
        buffer.begin_user_action();
        buffer.delete_selection(true, view.is_editable());
        buffer.insert_interactive_at_cursor(insertion.as_str(), view.is_editable());
        buffer.end_user_action();
    });
}

fn record_raw_clipboard_selection(state: &Rc<RefCell<MainState>>, editors: &EditorPanes) {
    let Some((start, end)) = editors.raw_buffer.selection_bounds() else {
        state.borrow_mut().pending_raw_clipboard = None;
        return;
    };

    if editors
        .raw_buffer
        .text(&start, &end, true)
        .trim()
        .is_empty()
    {
        state.borrow_mut().pending_raw_clipboard = None;
        return;
    }

    let mode = current_casing(state);
    let (start_line, end_line) = selected_line_range(&start, &end);
    let raw_text = editors.raw_text();
    let lines = pending_raw_lines_for_range(&raw_text, mode, start_line, end_line);

    state.borrow_mut().pending_raw_clipboard = if lines.is_empty() {
        None
    } else {
        Some(PendingRawClipboard { lines })
    };
}

fn pending_raw_lines_for_range(
    raw_text: &str,
    mode: CasingMode,
    start_line: usize,
    end_line: usize,
) -> Vec<PendingRawLine> {
    raw_line_identities(raw_text, mode)
        .into_iter()
        .filter(|identity| (start_line..=end_line).contains(&identity.line_index))
        .map(|identity| PendingRawLine {
            normalized: identity.normalized.clone(),
            material: material_from_identity(&identity),
        })
        .collect()
}

fn selected_line_range(start: &gtk::TextIter, end: &gtk::TextIter) -> (usize, usize) {
    let mut start_line = start.line().min(end.line()).max(0) as usize;
    let mut end_line = start.line().max(end.line()).max(0) as usize;
    let selection_ends_at_line_start = end.line_offset() == 0 && end.line() > start.line();
    if selection_ends_at_line_start && end_line > start_line {
        end_line -= 1;
    }
    if start_line > end_line {
        std::mem::swap(&mut start_line, &mut end_line);
    }
    (start_line, end_line)
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    window: &gtk::ApplicationWindow,
    root_overlay: &gtk::Overlay,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    overlay: &Rc<TrackOverlay>,
    search_revealer: &gtk::Revealer,
    notice: &gtk::Label,
    keyval: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> gtk::glib::Propagation {
    let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
    let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);

    if keyval == gtk::gdk::Key::F11 {
        flush_current(state, editors, notice);
        let enabled = !state.borrow().app_settings.fullscreen;
        window_policy::set_fullscreen_enabled(window, enabled);
        {
            let mut state_ref = state.borrow_mut();
            state_ref.app_settings.fullscreen = enabled;
            if let Err(err) = state_ref.settings_store.save(&state_ref.app_settings) {
                notifications::show_error(notice, err.to_string());
            }
        }
        return gtk::glib::Propagation::Stop;
    }

    if keyval == gtk::gdk::Key::Escape {
        if overlay.is_visible() {
            overlay.hide();
            return gtk::glib::Propagation::Stop;
        }
        if search_revealer.reveals_child() {
            search_revealer.set_reveal_child(false);
            clear_search_highlights(&editors.final_buffer);
            clear_search_highlights(&editors.raw_buffer);
            editors.final_view.set_bottom_margin(0);
            editors.raw_view.set_bottom_margin(0);
            clear_search_marker_layer(state, root_overlay);
            return gtk::glib::Propagation::Stop;
        }
        window_policy::reassert_fullscreen(window);
        return gtk::glib::Propagation::Stop;
    }

    if ctrl {
        if let Some(ch) = keyval.to_unicode() {
            match ch {
                '\n' | '\r' => {
                    if editors.raw_view.has_focus() {
                        if let Some((line, entry)) = raw_gutter::current_raw_line_identity(
                            &editors.raw_buffer,
                            current_casing(state),
                        ) {
                            transfer_raw_line(state, editors, overlay, line, entry);
                        }
                        return gtk::glib::Propagation::Stop;
                    }
                }
                'l' | 'L' => {
                    editors.final_view.grab_focus();
                    return gtk::glib::Propagation::Stop;
                }
                'r' | 'R' => {
                    editors.raw_view.grab_focus();
                    return gtk::glib::Propagation::Stop;
                }
                'f' | 'F' => {
                    show_search_panel(
                        state,
                        root_overlay,
                        editors,
                        search_revealer,
                        window.width(),
                    );
                    return gtk::glib::Propagation::Stop;
                }
                'c' | 'C' | 'x' | 'X' => {
                    let last_focus = state.borrow().last_focus;
                    let is_raw = editors.raw_view.has_focus() || last_focus == PaneFocus::Raw;
                    if is_raw {
                        record_raw_clipboard_selection(state, editors);
                    } else if editors.final_view.has_focus() || last_focus == PaneFocus::Final {
                        state.borrow_mut().pending_raw_clipboard = None;
                    }
                    let view = focus_view(editors);
                    if matches!(ch, 'c' | 'C') {
                        copy_plain_text_selection(&view);
                    } else {
                        cut_plain_text_selection(&view);
                    }
                    return gtk::glib::Propagation::Stop;
                }
                'v' | 'V' => {
                    paste_plain_text(&focus_view(editors));
                    return gtk::glib::Propagation::Stop;
                }
                'q' | 'Q' => {
                    exit_application(window, state, editors, notice);
                    return gtk::glib::Propagation::Stop;
                }
                'z' | 'Z' if shift => {
                    focus_buffer(editors).redo();
                    return gtk::glib::Propagation::Stop;
                }
                'z' | 'Z' => {
                    focus_buffer(editors).undo();
                    return gtk::glib::Propagation::Stop;
                }
                'y' | 'Y' => {
                    focus_buffer(editors).redo();
                    return gtk::glib::Propagation::Stop;
                }
                'a' | 'A' => {
                    let buffer = focus_buffer(editors);
                    buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
                    return gtk::glib::Propagation::Stop;
                }
                _ => {}
            }
        }
    }

    gtk::glib::Propagation::Proceed
}

fn exit_application(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<MainState>>,
    editors: &Rc<EditorPanes>,
    notice: &gtk::Label,
) {
    flush_current(state, editors, notice);
    if let Some(app) = window.application() {
        app.quit();
    } else {
        window.close();
    }
}

fn focus_buffer(editors: &EditorPanes) -> gtk::TextBuffer {
    if editors.raw_view.has_focus() {
        editors.raw_buffer.clone()
    } else {
        editors.final_buffer.clone()
    }
}

fn focus_view(editors: &EditorPanes) -> gtk::TextView {
    if editors.raw_view.has_focus() {
        editors.raw_view.clone()
    } else {
        editors.final_view.clone()
    }
}

fn font_size_combo(active_size: u16) -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    combo.add_css_class("font-size-combo");
    for size in VALID_FONT_SIZES {
        combo.append_text(&size.to_string());
    }
    let active_index = VALID_FONT_SIZES
        .iter()
        .position(|size| *size == active_size)
        .unwrap_or(3);
    combo.set_active(Some(active_index as u32));
    combo.set_tooltip_text(Some("Editor font size"));
    combo
}

fn start_behavior_combo(active_behavior: StartBehavior) -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    combo.add_css_class("font-size-combo");
    for label in [
        StartBehavior::FreshIdea.label(),
        StartBehavior::LastIdea.label(),
        StartBehavior::LastTrack.label(),
        StartBehavior::TrackList.label(),
    ] {
        combo.append_text(label);
    }
    combo.set_active(Some(active_behavior.combo_index()));
    combo.set_tooltip_text(Some("Start behaviour"));
    combo
}

fn settings_icon_toggle_button(active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("settings-toggle-button");
    button.set_size_request(36, 36);
    set_settings_toggle_icon(&button, active);
    button
}

fn set_settings_toggle_icon(button: &gtk::Button, active: bool) {
    let icon_name = if active {
        "toggle-check-green.svg"
    } else {
        "toggle-close-gray.svg"
    };
    button.set_child(Some(&icon_image_with_size(icon_name, "", 18)));
}

fn icon_text_button(file_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("icon-text-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let icon = icon_image_with_size(file_name, "", 18);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

fn icon_button(file_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&icon_image_with_size(file_name, "", 18)));
    button
}

fn icon_image_with_size(file_name: &str, fallback: &str, pixel_size: i32) -> gtk::Widget {
    let path = app_paths::icon_path(file_name);
    if path.exists() {
        let size = pixel_size.max(1);
        let image = gtk::Image::from_file(path);
        image.set_pixel_size(size);
        image.set_size_request(size, size);
        image.set_halign(gtk::Align::Center);
        image.set_valign(gtk::Align::Center);
        image.set_can_target(false);
        image.upcast()
    } else {
        gtk::Label::new(Some(fallback)).upcast()
    }
}

fn show_startup_error(window: &gtk::ApplicationWindow, err: AppError) {
    app_logging::log_error(err.to_string());
    window.set_title(Some("ROPER"));
    window_policy::reassert_fullscreen(window);
    let message = if let Some(log_path) = app_logging::log_path() {
        format!("{}\n\nLog file: {}", err, log_path.display())
    } else {
        err.to_string()
    };
    let label = gtk::Label::new(Some(&err.to_string()));
    label.add_css_class("notification");
    label.set_wrap(true);
    label.set_margin_top(18);
    label.set_margin_bottom(18);
    label.set_margin_start(18);
    label.set_margin_end(18);
    window.set_child(Some(&label));
    window.present();

    let dialog = gtk::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk::MessageType::Error)
        .buttons(gtk::ButtonsType::Close)
        .text("ROPER could not start")
        .secondary_text(&message)
        .build();
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_reopen_skips_flush_only_for_same_track() {
        assert!(should_skip_flush_for_track_reopen(
            Some("track-1"),
            "track-1",
            true,
        ));
        assert!(!should_skip_flush_for_track_reopen(
            Some("track-1"),
            "track-2",
            true,
        ));
        assert!(!should_skip_flush_for_track_reopen(None, "track-1", true));
        assert!(!should_skip_flush_for_track_reopen(
            Some("track-1"),
            "track-1",
            false,
        ));
    }

    #[test]
    fn raw_clipboard_line_consumption_is_casing_independent() {
        let mut pending = PendingRawClipboard {
            lines: pending_raw_lines_for_range("Straße\ncafé", CasingMode::Preserve, 0, 1),
        };

        let first =
            consume_pending_material_for_insert(&mut pending, "STRASSE", CasingMode::Uppercase);
        assert_eq!(first.len(), 1);
        assert_eq!(pending.lines.len(), 1);

        let second =
            consume_pending_material_for_insert(&mut pending, "CAFÉ", CasingMode::Uppercase);
        assert_eq!(second.len(), 1);
        assert!(pending.lines.is_empty());
    }

    #[test]
    fn raw_clipboard_line_consumption_does_not_require_whole_block() {
        let mut pending = PendingRawClipboard {
            lines: pending_raw_lines_for_range(
                "first\n\nsecond\nthird",
                CasingMode::Preserve,
                0,
                3,
            ),
        };

        let consumed =
            consume_pending_material_for_insert(&mut pending, "SECOND", CasingMode::Uppercase);
        assert_eq!(consumed.len(), 1);
        assert_eq!(pending.lines.len(), 2);
    }

    #[test]
    fn raw_clipboard_line_consumption_respects_duplicate_occurrences() {
        let mut pending = PendingRawClipboard {
            lines: pending_raw_lines_for_range("hook\nhook\nhook", CasingMode::Preserve, 0, 2),
        };

        let first =
            consume_pending_material_for_insert(&mut pending, "HOOK", CasingMode::Uppercase);
        let second =
            consume_pending_material_for_insert(&mut pending, "HOOK", CasingMode::Uppercase);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].occurrence, second[0].occurrence);
    }

    #[test]
    fn inserted_text_is_cased_without_full_buffer_rewrite() {
        assert_eq!(
            cased_insert_text("du spast", CasingMode::Uppercase),
            Some("DU SPAST".to_owned())
        );
        assert_eq!(
            cased_insert_text("DU SPAST", CasingMode::Lowercase),
            Some("du spast".to_owned())
        );
        assert_eq!(cased_insert_text("du spast", CasingMode::Preserve), None);
        assert_eq!(cased_insert_text("DU SPAST", CasingMode::Uppercase), None);
    }

    #[test]
    fn info_build_lines_include_runtime_details_without_credits() {
        let lines = build_information_lines();
        assert!(lines.iter().all(|line| !line.starts_with("application ")));
        assert!(lines.iter().any(|line| line.starts_with("gtk ")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("loaded libraries "))
        );
        assert!(lines.iter().any(|line| line.starts_with("build size ")));
        assert!(lines.iter().any(|line| line.starts_with("memory ")));
        assert!(!lines.iter().any(|line| line.contains("credits")));
        assert!(!lines.iter().any(|line| line.contains("GIPPZ")));
    }

    #[test]
    fn info_metrics_are_grouped_for_formatted_info_view() {
        let metrics = build_information_metrics();
        assert!(metrics.iter().any(|metric| metric.section == "Application"));
        assert!(!metrics.iter().any(|metric| metric.label == "application"));
        assert!(metrics.iter().any(|metric| metric.section == "Runtime"));
        assert!(metrics.iter().any(|metric| metric.section == "System"));
        assert!(metrics.iter().any(|metric| metric.label == "gtk"));
        assert!(
            metrics
                .iter()
                .any(|metric| metric.label == "loaded libraries")
        );
        assert!(metrics.iter().any(|metric| metric.label == "build size"));
        assert!(metrics.iter().any(|metric| metric.label == "memory"));
    }

    #[test]
    fn info_splash_preview_is_bounded_and_keeps_native_aspect() {
        assert_eq!(INFO_SPLASH_PREVIEW_WIDTH, 320);
        assert_eq!(INFO_SPLASH_PREVIEW_HEIGHT, 200);
        assert_eq!(
            INFO_SPLASH_PREVIEW_WIDTH * 10,
            INFO_SPLASH_PREVIEW_HEIGHT * 16
        );
    }

    #[test]
    fn startup_workspace_mode_prefers_ideas_for_idea_start_behaviors() {
        assert!(startup_uses_ideas_workspace(StartBehavior::FreshIdea));
        assert!(startup_uses_ideas_workspace(StartBehavior::LastIdea));
        assert!(!startup_uses_ideas_workspace(StartBehavior::LastTrack));
        assert!(!startup_uses_ideas_workspace(StartBehavior::TrackList));
    }

    #[test]
    fn track_list_artwork_cannot_expand_track_rows() {
        assert_eq!(LIST_IMAGE_COLUMN_WIDTH, 160);
        assert_eq!(TRACK_ROW_HEIGHT, LIST_IMAGE_COLUMN_WIDTH);
        assert_eq!(TRACK_LIST_THUMBNAIL_SIZE, LIST_IMAGE_COLUMN_WIDTH);
    }

    #[test]
    fn editor_footer_starts_on_final_pane_side() {
        assert_eq!(editor_chrome_raw_width(1000), 400);
        assert_eq!(editor_chrome_raw_width(0), 0);
    }

    #[test]
    fn editor_footer_top_keeps_original_bottom_gap_inside_viewport() {
        assert_eq!(editor_chrome_top(1080, 36), 1044);
        assert_eq!(editor_chrome_top(540, 36), 504);
        assert_eq!(editor_chrome_top(36, 36), 0);
    }

    #[test]
    fn visible_dimension_prefers_smallest_positive_value() {
        assert_eq!(smallest_positive_dimension([0, 2400, 1080]), 1080);
        assert_eq!(smallest_positive_dimension([720, 2400, 1080]), 720);
        assert_eq!(smallest_positive_dimension([0, 0, 0]), 0);
    }

    #[test]
    fn editor_footer_is_hidden_while_overlay_is_visible() {
        assert!(!editor_footer_visible_for_workspace(true, false));
        assert!(editor_footer_visible_for_workspace(false, false));
        assert!(!editor_footer_visible_for_workspace(false, true));
    }

    #[test]
    fn track_structure_bubble_colors_brighten_for_repeated_section_types() {
        let (_, early_verse_green, _, _) = structure_bubble_color(StructureKind::Verse, 0);
        let (_, late_verse_green, _, _) =
            structure_bubble_color(StructureKind::Verse, STRUCTURE_BUCKETS - 1);
        let (_, early_hook_green, _, _) = structure_bubble_color(StructureKind::Hook, 0);
        let (_, late_hook_green, _, _) =
            structure_bubble_color(StructureKind::Hook, STRUCTURE_BUCKETS - 1);

        assert!(late_verse_green > early_verse_green);
        assert!(late_hook_green > early_hook_green);
    }

    #[test]
    fn track_structure_bubble_width_represents_section_length() {
        assert_eq!(structure_bubble_width(100, 100, 400), 400);
        assert_eq!(structure_bubble_width(0, 100, 400), STRUCTURE_BUBBLE_MIN_WIDTH);
        assert_eq!(structure_bubble_width(50, 100, 400), 200);
        assert!(structure_bubble_width(25, 100, 400) < structure_bubble_width(75, 100, 400));
    }

    #[test]
    fn track_structure_bubble_width_scales_by_total_track_length() {
        assert_eq!(structure_bubble_width(50, 200, 400), 100);
        assert_eq!(structure_bubble_width(50, 300, 400), 67);
        assert_eq!(structure_bubble_width(100, 300, 400), 133);
    }

    #[test]
    fn track_structure_bubble_width_is_capped_at_available_width() {
        assert_eq!(
            structure_bubble_width(usize::MAX, usize::MAX, 400),
            400
        );
    }

    #[test]
    fn editor_text_stats_count_lines_words_and_characters() {
        assert_eq!(
            editor_text_stats("eins zwei\nthree!"),
            EditorTextStats {
                lines: 2,
                words: 3,
                chars: 16,
            }
        );
        assert_eq!(
            editor_text_stats(""),
            EditorTextStats {
                lines: 0,
                words: 0,
                chars: 0,
            }
        );
    }

    #[test]
    fn pane_text_stats_count_raw_and_final_independently() {
        assert_eq!(
            pane_text_stats("raw line\nraw two", "final line"),
            PaneTextStats {
                raw: EditorTextStats {
                    lines: 2,
                    words: 4,
                    chars: 16,
                },
                final_pane: EditorTextStats {
                    lines: 1,
                    words: 2,
                    chars: 10,
                },
            }
        );
    }

    #[test]
    fn current_track_row_stats_prefer_live_editor_buffers() {
        let saved = PaneTextStats {
            raw: EditorTextStats {
                lines: 1,
                words: 1,
                chars: 3,
            },
            final_pane: EditorTextStats {
                lines: 1,
                words: 1,
                chars: 5,
            },
        };

        assert_eq!(
            resolved_track_row_stats(
                "track-1",
                Some("track-1"),
                Some(("a b\nc", "alpha beta gamma")),
                saved,
            ),
            PaneTextStats {
                raw: EditorTextStats {
                    lines: 2,
                    words: 3,
                    chars: 5,
                },
                final_pane: EditorTextStats {
                    lines: 1,
                    words: 3,
                    chars: 16,
                },
            }
        );
    }

    #[test]
    fn split_stat_text_keeps_metric_visible() {
        assert_eq!(split_stat_text("L", 12), "L 12");
        assert_eq!(split_stat_text("W", 0), "W 0");
    }

    #[test]
    fn structure_tool_labels_follow_current_final_structure_counts() {
        let text = "[intro]\nstart\n[verse 1]\na\n[hook]\nh\n[VERSE 2]\nb\n[HOOK 2]\nh";

        assert_eq!(
            structure_tool_label_for_kind(text, StructureKind::Intro),
            "[INTRO]"
        );
        assert_eq!(
            structure_tool_label_for_kind(text, StructureKind::Verse),
            "[VERSE 3]"
        );
        assert_eq!(
            structure_tool_label_for_kind(text, StructureKind::Hook),
            "[HOOK 3]"
        );
        assert_eq!(
            structure_tool_label_for_kind(text, StructureKind::Outro),
            "[OUTRO]"
        );
    }

    #[test]
    fn structure_tool_numbers_decrease_when_tags_are_removed() {
        assert_eq!(
            next_structure_number("[verse 1]\na\n[verse 2]\nb", StructureKind::Verse),
            3
        );
        assert_eq!(
            next_structure_number("[verse 1]\na", StructureKind::Verse),
            2
        );
        assert_eq!(next_structure_number("", StructureKind::Hook), 1);
    }

    #[test]
    fn structure_tool_numbers_follow_highest_existing_number() {
        assert_eq!(
            next_structure_number("[verse 1]\na\n[verse 3]\nb", StructureKind::Verse),
            4
        );
        assert_eq!(
            next_structure_number("[hook]\na\n[hook 7]\nb", StructureKind::Hook),
            8
        );
        assert_eq!(
            next_structure_number("[hook]\na\n[HOOK]\nb", StructureKind::Hook),
            3
        );
        assert_eq!(
            next_structure_number("[hook 100]\nignored", StructureKind::Hook),
            1
        );
    }

    #[test]
    fn easter_egg_easing_has_expected_shape() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!(ease_in_out(0.25) < 0.25);
        assert!(ease_in_out(0.75) > 0.75);
    }

    #[test]
    fn credit_flights_use_requested_font_range_and_translucency() {
        let flights = credit_flights(&["ALPHA", "BETA", "GAMMA", "DELTA"]);
        assert_eq!(flights.len(), 4);
        assert!(flights.iter().all(|flight| {
            (CREDIT_FONT_MIN_PT..=CREDIT_FONT_MAX_PT).contains(&flight.font_size_pt)
        }));
        assert!(
            flights
                .iter()
                .all(|flight| flight.alpha > 0.0 && flight.alpha < 1.0)
        );
        assert!(
            flights
                .iter()
                .all(|flight| (0.0..=0.36).contains(&flight.delay))
        );

        let rounded_sizes = flights
            .iter()
            .map(|flight| flight.font_size_pt.round() as u16)
            .collect::<HashSet<_>>();
        assert!(rounded_sizes.len() > 1);
        assert_eq!(credit_fade(0.0), 0.0);
        assert!(credit_fade(0.5) > 0.99);
        assert!(credit_fade(1.0) < f64::EPSILON);
    }

    #[test]
    fn byte_formatting_is_human_readable() {
        assert_eq!(format_bytes(42), "42 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
    }
}
