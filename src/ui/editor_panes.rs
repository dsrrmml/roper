use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::services::live_highlights::RepeatWarning;
use crate::ui::raw_gutter::NUMBER_LANE_WIDTH;

const FINAL_WARNING_MARGIN_PX: i32 = 96;
pub const RAW_PANE_WIDTH_FRACTION: f64 = 0.40;
const EDITOR_GUTTER_WIDTH_PX: i32 = 56;
const EDITOR_MINIMAP_WIDTH_PX: i32 = 56;
const EDITOR_MINIMAP_CONTENT_INSET_PX: f64 = 6.0;
const EDITOR_MINIMAP_MIN_VIEWPORT_HEIGHT_PX: f64 = 18.0;
const EDITOR_MINIMAP_STRUCTURE_KINDS: usize = 6;
const EDITOR_MINIMAP_LINE_THRESHOLD: usize = 100;
const EDITOR_MINIMAP_MARGIN_PX: i32 = 12;
const EDITOR_MINIMAP_LABEL_FONT_PX: f64 = 12.0;
const EDITOR_MINIMAP_LABEL_MIN_GAP_PX: f64 = 10.0;
const EDITOR_MINIMAP_LABEL_PADDING_PX: f64 = 3.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditorMinimapStructureKind {
    Intro,
    Verse,
    Hook,
    Bridge,
    Outro,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EditorMinimapStructureTag {
    kind: EditorMinimapStructureKind,
    number: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct EditorMinimapNumberLabel {
    y: f64,
    kind: EditorMinimapStructureKind,
    number: Option<usize>,
}

pub struct EditorPanes {
    pub root: gtk::Paned,
    pub final_view: gtk::TextView,
    pub raw_view: gtk::TextView,
    pub final_buffer: gtk::TextBuffer,
    pub raw_buffer: gtk::TextBuffer,
    pub final_pattern_layer: gtk::DrawingArea,
    pub final_warning_layer: gtk::DrawingArea,
    pub final_gutter: gtk::Box,
    pub raw_gutter: gtk::Box,
    pub final_minimap: gtk::DrawingArea,
    pub raw_minimap: gtk::DrawingArea,
    pub empty_line_pattern_enabled: Rc<Cell<bool>>,
    pub symbols_in_minimap: Rc<Cell<bool>>,
    pub line_numbers_enabled: Rc<Cell<bool>>,
    pub final_warning_markers: Rc<RefCell<Vec<RepeatWarning>>>,
}

impl EditorPanes {
    pub fn new(font_size_pt: u16) -> Self {
        install_font_css(font_size_pt);

        let final_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        let raw_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        final_buffer.set_enable_undo(true);
        raw_buffer.set_enable_undo(true);

        let final_view = build_text_view(&final_buffer);
        final_view.add_css_class("final-editor-view");
        final_view.set_left_margin(0); // was NUMBER_LANE_WIDTH
        final_view.set_right_margin(FINAL_WARNING_MARGIN_PX);
        let empty_line_pattern_enabled = Rc::new(Cell::new(false));
        let raw_view = build_text_view(&raw_buffer);
        raw_view.set_left_margin(EDITOR_GUTTER_WIDTH_PX);
        raw_view.set_right_margin(EDITOR_MINIMAP_MARGIN_PX);

        let final_scrolled = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&final_view)
            .build();
        final_scrolled.add_css_class("final-scrolled");

        let raw_scrolled = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&raw_view)
            .build();

        let final_pattern_layer = gtk::DrawingArea::new();
        final_pattern_layer.add_css_class("final-pattern-layer");
        final_pattern_layer.set_can_target(false);
        final_pattern_layer.set_halign(gtk::Align::Fill);
        final_pattern_layer.set_valign(gtk::Align::Fill);
        final_pattern_layer.set_hexpand(true);
        final_pattern_layer.set_vexpand(true);

        {
            let enabled = empty_line_pattern_enabled.clone();
            final_pattern_layer.set_draw_func(move |_, cr, width, height| {
                if !enabled.get() {
                    return;
                }
                let width = width as f64;
                let height = height as f64;
                let angle = 27_f64.to_radians();
                let slope = angle.tan();
                let spacing = 18.0;
                let run = height / slope;
                let mut x = -run;

                cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
                cr.set_line_width(1.6);
                cr.set_line_cap(gtk::cairo::LineCap::Round);

                while x <= width {
                    cr.move_to(x, 0.0);
                    cr.line_to(x + run, height);
                    x += spacing;
                }
                cr.stroke().ok();
            });
        }

        let final_warning_layer = gtk::DrawingArea::new();
        final_warning_layer.add_css_class("final-warning-layer");
        final_warning_layer.set_can_target(false);
        final_warning_layer.set_halign(gtk::Align::Fill);
        final_warning_layer.set_valign(gtk::Align::Fill);
        final_warning_layer.set_hexpand(true);
        final_warning_layer.set_vexpand(true);

        {
            let final_warning_layer = final_warning_layer.clone();
            let final_pattern_layer = final_pattern_layer.clone();
            final_scrolled
                .vadjustment()
                .connect_value_changed(move |_| {
                    final_warning_layer.queue_draw();
                    final_pattern_layer.queue_draw();
                });
        }
        {
            let final_warning_layer = final_warning_layer.clone();
            let final_pattern_layer = final_pattern_layer.clone();
            final_scrolled
                .hadjustment()
                .connect_value_changed(move |_| {
                    final_warning_layer.queue_draw();
                    final_pattern_layer.queue_draw();
                });
        }

        let final_gutter = gtk::Box::new(gtk::Orientation::Vertical, 0);
        final_gutter.add_css_class("editor-gutter");
        final_gutter.set_size_request(NUMBER_LANE_WIDTH, -1);
        final_gutter.set_halign(gtk::Align::Start);
        final_gutter.set_valign(gtk::Align::Fill);
        // Ensure the gutter sits flush with the pane border and top
        final_gutter.set_margin_start(0);
        final_gutter.set_margin_top(0);
        final_gutter.set_margin_end(0);
        final_gutter.set_hexpand(false);
        final_gutter.set_vexpand(true);
        crate::ui::raw_gutter::install_line_number_gutter(&final_gutter, &final_view);

        let final_minimap = gtk::DrawingArea::new();
        final_minimap.add_css_class("editor-minimap");
        final_minimap.set_size_request(EDITOR_MINIMAP_WIDTH_PX, -1);
        final_minimap.set_halign(gtk::Align::End);
        final_minimap.set_valign(gtk::Align::Fill);
        final_minimap.set_hexpand(false);
        final_minimap.set_vexpand(true);
        final_minimap.set_visible(false);

        let raw_gutter = gtk::Box::new(gtk::Orientation::Vertical, 0);
        raw_gutter.add_css_class("raw-gutter");
        raw_gutter.set_size_request(EDITOR_GUTTER_WIDTH_PX, -1);
        raw_gutter.set_halign(gtk::Align::Start);
        raw_gutter.set_valign(gtk::Align::Fill);
        raw_gutter.set_hexpand(false);
        raw_gutter.set_vexpand(true);

        let symbols_in_minimap = Rc::new(Cell::new(false));
        let line_numbers_enabled = Rc::new(Cell::new(true));
        let final_warning_markers = Rc::new(RefCell::new(Vec::new()));

        let raw_minimap = gtk::DrawingArea::new();
        raw_minimap.add_css_class("editor-minimap");
        raw_minimap.set_size_request(EDITOR_MINIMAP_WIDTH_PX, -1);
        raw_minimap.set_halign(gtk::Align::End);
        raw_minimap.set_valign(gtk::Align::Fill);
        raw_minimap.set_hexpand(false);
        raw_minimap.set_vexpand(true);
        raw_minimap.set_visible(false);

        let final_adjustment = final_scrolled.vadjustment();
        {
            let final_gutter = final_gutter.clone();
            final_adjustment.connect_value_changed(move |_| {
                queue_gutter_redraw(&final_gutter);
            });
        }
        {
            let final_minimap = final_minimap.clone();
            final_adjustment.connect_value_changed(move |_| {
                final_minimap.queue_draw();
            });
        }
        {
            let final_minimap = final_minimap.clone();
            final_adjustment.connect_changed(move |_| {
                final_minimap.queue_draw();
            });
        }
        {
            let final_gutter = final_gutter.clone();
            let final_minimap = final_minimap.clone();
            let final_view = final_view.clone();
            let final_buffer_for_signal = final_buffer.clone();
            let final_buffer_for_visibility = final_buffer.clone();
            final_buffer_for_signal.connect_changed(move |_| {
                queue_gutter_redraw(&final_gutter);
                refresh_editor_minimap_visibility(
                    &final_view,
                    &final_minimap,
                    &final_buffer_for_visibility,
                    FINAL_WARNING_MARGIN_PX,
                );
                final_minimap.queue_draw();
            });
        }
        {
            let final_buffer = final_buffer.clone();
            let final_adjustment = final_adjustment.clone();
            let symbols_in_minimap = symbols_in_minimap.clone();
            let final_warning_markers = final_warning_markers.clone();
            final_minimap.set_draw_func(move |layer, cr, width, height| {
                draw_editor_minimap(
                    layer,
                    cr,
                    width as f64,
                    height as f64,
                    &final_buffer,
                    &final_adjustment,
                    Some(&symbols_in_minimap),
                    Some(&final_warning_markers),
                );
            });
        }
        {
            let final_adjustment = final_adjustment.clone();
            let final_minimap_for_click = final_minimap.clone();
            let click = gtk::GestureClick::new();
            click.set_button(0);
            click.connect_pressed(move |_, _, _, y| {
                set_adjustment_from_minimap_y(
                    &final_adjustment,
                    y,
                    final_minimap_for_click.height() as f64,
                );
            });
            final_minimap.add_controller(click);
        }
        {
            let final_adjustment = final_adjustment.clone();
            let final_minimap_for_drag = final_minimap.clone();
            let drag_origin_y = Rc::new(Cell::new(0.0));
            let drag = gtk::GestureDrag::new();
            {
                let drag_origin_y = drag_origin_y.clone();
                let final_adjustment = final_adjustment.clone();
                let final_minimap_for_begin = final_minimap_for_drag.clone();
                drag.connect_drag_begin(move |_, _, start_y| {
                    drag_origin_y.set(start_y);
                    set_adjustment_from_minimap_y(
                        &final_adjustment,
                        start_y,
                        final_minimap_for_begin.height() as f64,
                    );
                });
            }
            {
                let drag_origin_y = drag_origin_y.clone();
                let final_minimap_for_update = final_minimap_for_drag.clone();
                drag.connect_drag_update(move |_, _, offset_y| {
                    set_adjustment_from_minimap_y(
                        &final_adjustment,
                        drag_origin_y.get() + offset_y,
                        final_minimap_for_update.height() as f64,
                    );
                });
            }
            final_minimap.add_controller(drag);
        }

        let raw_adjustment = raw_scrolled.vadjustment();
        let raw_gutter_for_scroll = raw_gutter.clone();
        raw_adjustment.connect_value_changed(move |adjustment| {
            let _ = adjustment;
            queue_gutter_redraw(&raw_gutter_for_scroll);
        });

        {
            let raw_minimap = raw_minimap.clone();
            raw_adjustment.connect_value_changed(move |_| {
                raw_minimap.queue_draw();
            });
        }

        {
            let raw_minimap = raw_minimap.clone();
            raw_adjustment.connect_changed(move |_| {
                raw_minimap.queue_draw();
            });
        }

        {
            let raw_minimap = raw_minimap.clone();
            let raw_view = raw_view.clone();
            let raw_gutter = raw_gutter.clone();
            let raw_buffer_for_signal = raw_buffer.clone();
            let raw_buffer_for_visibility = raw_buffer.clone();
            raw_buffer_for_signal.connect_changed(move |_| {
                queue_gutter_redraw(&raw_gutter);
                refresh_editor_minimap_visibility(
                    &raw_view,
                    &raw_minimap,
                    &raw_buffer_for_visibility,
                    0,
                );
                raw_minimap.queue_draw();
            });
        }

        {
            let raw_buffer = raw_buffer.clone();
            let raw_adjustment = raw_adjustment.clone();
            raw_minimap.set_draw_func(move |layer, cr, width, height| {
                draw_editor_minimap(
                    layer,
                    cr,
                    width as f64,
                    height as f64,
                    &raw_buffer,
                    &raw_adjustment,
                    None,
                    None,
                );
            });
        }

        {
            let raw_adjustment = raw_adjustment.clone();
            let raw_minimap_for_click = raw_minimap.clone();
            let click = gtk::GestureClick::new();
            click.set_button(0);
            click.connect_pressed(move |_, _, _, y| {
                set_adjustment_from_minimap_y(
                    &raw_adjustment,
                    y,
                    raw_minimap_for_click.height() as f64,
                );
            });
            raw_minimap.add_controller(click);
        }

        {
            let raw_adjustment = raw_adjustment.clone();
            let raw_minimap_for_drag = raw_minimap.clone();
            let drag_origin_y = Rc::new(Cell::new(0.0));
            let drag = gtk::GestureDrag::new();
            {
                let drag_origin_y = drag_origin_y.clone();
                let raw_adjustment = raw_adjustment.clone();
                let raw_minimap_for_begin = raw_minimap_for_drag.clone();
                drag.connect_drag_begin(move |_, _, start_y| {
                    drag_origin_y.set(start_y);
                    set_adjustment_from_minimap_y(
                        &raw_adjustment,
                        start_y,
                        raw_minimap_for_begin.height() as f64,
                    );
                });
            }
            {
                let drag_origin_y = drag_origin_y.clone();
                let raw_minimap_for_update = raw_minimap_for_drag.clone();
                drag.connect_drag_update(move |_, _, offset_y| {
                    set_adjustment_from_minimap_y(
                        &raw_adjustment,
                        drag_origin_y.get() + offset_y,
                        raw_minimap_for_update.height() as f64,
                    );
                });
            }
            raw_minimap.add_controller(drag);
        }

        let final_overlay = gtk::Overlay::new();
        final_overlay.set_hexpand(true);
        final_overlay.set_vexpand(true);
        final_overlay.set_child(Some(&final_pattern_layer));
        final_overlay.add_overlay(&final_scrolled);
        final_overlay.add_overlay(&final_gutter);
        final_overlay.add_overlay(&final_minimap);
        final_overlay.add_overlay(&final_warning_layer);
        let final_pane = pane_shell("final", &final_overlay);
        let raw_overlay = gtk::Overlay::new();
        raw_overlay.set_hexpand(true);
        raw_overlay.set_vexpand(true);
        raw_overlay.set_child(Some(&raw_scrolled));
        raw_overlay.add_overlay(&raw_gutter);
        raw_overlay.add_overlay(&raw_minimap);
        let raw_pane = pane_shell("raw", &raw_overlay);

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&raw_pane));
        root.set_end_child(Some(&final_pane));
        root.set_resize_start_child(true);
        root.set_resize_end_child(true);
        root.set_shrink_start_child(false);
        root.set_shrink_end_child(false);

        let panes = Self {
            root,
            final_view,
            raw_view,
            final_buffer,
            raw_buffer,
            final_pattern_layer,
            final_warning_layer,
            final_gutter,
            raw_gutter,
            final_minimap,
            raw_minimap,
            empty_line_pattern_enabled,
            symbols_in_minimap,
            line_numbers_enabled,
            final_warning_markers,
        };

        refresh_editor_minimap_visibility(
            &panes.final_view,
            &panes.final_minimap,
            &panes.final_buffer,
            FINAL_WARNING_MARGIN_PX,
        );
        refresh_editor_minimap_visibility(
            &panes.raw_view,
            &panes.raw_minimap,
            &panes.raw_buffer,
            0,
        );
        panes
    }

    pub fn set_texts(&self, final_text: &str, raw_text: &str) {
        self.final_buffer.set_text(final_text);
        self.raw_buffer.set_text(raw_text);
        refresh_editor_minimap_visibility(
            &self.final_view,
            &self.final_minimap,
            &self.final_buffer,
            FINAL_WARNING_MARGIN_PX,
        );
        refresh_editor_minimap_visibility(&self.raw_view, &self.raw_minimap, &self.raw_buffer, 0);
        queue_gutter_redraw(&self.final_gutter);
        queue_gutter_redraw(&self.raw_gutter);
    }

    pub fn clear(&self) {
        self.set_texts("", "");
    }

    pub fn set_track_connection(&self, connected: bool) {
        let (root_sensitive, view_sensitive, editable, can_focus, cursor_visible) =
            track_connection_state(connected);
        self.root.set_sensitive(root_sensitive);
        self.final_view.set_sensitive(view_sensitive);
        self.raw_view.set_sensitive(view_sensitive);
        self.final_view.set_editable(editable);
        self.raw_view.set_editable(editable);
        self.final_view.set_can_focus(can_focus);
        self.raw_view.set_can_focus(can_focus);
        self.final_view.set_cursor_visible(cursor_visible);
        self.raw_view.set_cursor_visible(cursor_visible);

        if !connected {
            self.clear();
        }
    }

    pub fn final_text(&self) -> String {
        buffer_text(&self.final_buffer)
    }

    pub fn raw_text(&self) -> String {
        buffer_text(&self.raw_buffer)
    }

    pub fn keep_ratio(&self) {
        let width = self.root.width();
        if width > 0 {
            self.root
                .set_position(((width as f64) * RAW_PANE_WIDTH_FRACTION).round() as i32);
        }
    }

    pub fn set_font_size(&self, font_size_pt: u16) {
        install_font_css(font_size_pt);
    }
}

pub fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

fn track_connection_state(connected: bool) -> (bool, bool, bool, bool, bool) {
    (connected, connected, connected, connected, connected)
}

#[cfg(test)]
mod editor_panes_tests {
    use super::*;

    #[test]
    fn track_connection_disables_editors_when_no_track_is_active() {
        let (root_sensitive, view_sensitive, editable, can_focus, cursor_visible) =
            track_connection_state(false);

        assert!(!root_sensitive);
        assert!(!view_sensitive);
        assert!(!editable);
        assert!(!can_focus);
        assert!(!cursor_visible);
    }

    #[test]
    fn track_connection_enables_editors_when_a_track_is_active() {
        let (root_sensitive, view_sensitive, editable, can_focus, cursor_visible) =
            track_connection_state(true);

        assert!(root_sensitive);
        assert!(view_sensitive);
        assert!(editable);
        assert!(can_focus);
        assert!(cursor_visible);
    }
}

pub fn replace_buffer_text_preserving_cursor(buffer: &gtk::TextBuffer, text: &str) {
    let old_offset = buffer.cursor_position();
    buffer.set_text(text);
    let max_offset = text.chars().count().min(old_offset.max(0) as usize) as i32;
    let iter = buffer.iter_at_offset(max_offset);
    buffer.place_cursor(&iter);
}

fn build_text_view(buffer: &gtk::TextBuffer) -> gtk::TextView {
    let view = gtk::TextView::new();
    view.set_buffer(Some(buffer));
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_accepts_tab(false);
    view.add_css_class("editor-view");
    view
}

fn refresh_editor_minimap_visibility(
    view: &gtk::TextView,
    minimap: &gtk::DrawingArea,
    buffer: &gtk::TextBuffer,
    base_right_margin: i32,
) {
    let show_minimap = editor_line_count(&buffer_text(buffer)) > EDITOR_MINIMAP_LINE_THRESHOLD;
    minimap.set_visible(show_minimap);
    view.set_right_margin(if show_minimap {
        base_right_margin + EDITOR_MINIMAP_WIDTH_PX + EDITOR_MINIMAP_MARGIN_PX
    } else {
        base_right_margin + EDITOR_MINIMAP_MARGIN_PX
    });
}

fn editor_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

fn queue_gutter_redraw(gutter: &gtk::Box) {
    gutter.queue_draw();
    if let Some(child) = gutter.first_child() {
        child.queue_draw();
    }
}

fn draw_editor_minimap(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    buffer: &gtk::TextBuffer,
    adjustment: &gtk::Adjustment,
    symbols_in_minimap: Option<&Rc<Cell<bool>>>,
    final_warning_markers: Option<&Rc<RefCell<Vec<RepeatWarning>>>>,
) {
    let _ = layer;
    if width <= 0.0 || height <= 0.0 {
        return;
    }

    cr.set_source_rgba(0.05, 0.07, 0.09, 0.84);
    cr.rectangle(0.0, 0.0, width, height);
    cr.fill().ok();

    cr.set_source_rgba(0.18, 0.21, 0.25, 0.94);
    cr.rectangle(0.0, 0.0, 1.0, height);
    cr.fill().ok();

    let text = buffer_text(buffer);
    let content_width = (width - EDITOR_MINIMAP_CONTENT_INSET_PX * 2.0).max(1.0);
    let bucket_count = height.max(1.0).round() as usize;
    let mut normal_density = vec![0_u16; bucket_count];
    let mut structure_density = vec![[0_u16; EDITOR_MINIMAP_STRUCTURE_KINDS]; bucket_count];
    let mut numbered_labels = Vec::new();
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };
    let total_lines = lines.len().max(1);

    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bucket = ((line_index as f64 / total_lines as f64) * bucket_count as f64)
            .floor()
            .clamp(0.0, (bucket_count.saturating_sub(1)) as f64) as usize;
        if let Some(tag) = parse_editor_minimap_structure_tag(trimmed) {
            let slot = &mut structure_density[bucket][tag.kind.index()];
            *slot = slot.saturating_add(1);
            if tag.kind.supports_badge() {
                numbered_labels.push(EditorMinimapNumberLabel {
                    y: bucket as f64,
                    kind: tag.kind,
                    number: tag.number,
                });
            }
        } else {
            normal_density[bucket] = normal_density[bucket].saturating_add(1);
        }
    }

    for bucket in 0..bucket_count {
        let structure = structure_density[bucket]
            .iter()
            .map(|count| *count as f64)
            .sum::<f64>();
        let normal = normal_density[bucket] as f64;
        if structure == 0.0 && normal == 0.0 {
            continue;
        }

        let density = ((structure * 1.35) + normal).min(4.0) / 4.0;
        let bar_width = (content_width * (0.24 + density * 0.76)).max(2.0);
        let x = width - EDITOR_MINIMAP_CONTENT_INSET_PX - bar_width;
        let y = bucket as f64;

        if normal > 0.0 {
            cr.set_source_rgba(0.62, 0.68, 0.74, 0.54 + density * 0.24);
            cr.rectangle(x, y, bar_width, 1.0);
            cr.fill().ok();
        }
        if structure > 0.0 {
            let dominant_kind = dominant_structure_kind(&structure_density[bucket]);
            let (red, green, blue) = editor_minimap_structure_color(dominant_kind);
            cr.set_source_rgba(red, green, blue, 0.74 + density * 0.16);
            cr.rectangle(x, y, bar_width, 1.0);
            cr.fill().ok();

            let accent_width = (bar_width * 0.34).clamp(2.0, content_width);
            cr.set_source_rgba(red, green, blue, 0.95);
            cr.rectangle(
                width - EDITOR_MINIMAP_CONTENT_INSET_PX - accent_width,
                y,
                accent_width,
                1.0,
            );
            cr.fill().ok();
        }
    }

    draw_editor_minimap_number_labels(cr, width, height, &numbered_labels);

    let (viewport_y, viewport_height) = minimap_viewport_geometry(
        height,
        adjustment.value(),
        adjustment.upper(),
        adjustment.page_size(),
    );
    cr.set_source_rgba(0.90, 0.94, 0.99, 0.14);
    cr.rectangle(1.0, viewport_y, width - 2.0, viewport_height);
    cr.fill().ok();

    cr.set_source_rgba(0.89, 0.93, 0.98, 0.72);
    cr.rectangle(
        0.5,
        viewport_y + 0.5,
        width - 1.0,
        (viewport_height - 1.0).max(1.0),
    );
    cr.stroke().ok();

    if symbols_in_minimap
        .and_then(|enabled| (enabled.get() as bool).then_some(()))
        .is_some()
        && final_warning_markers.is_some()
    {
        draw_minimap_warning_symbols(
            cr,
            width,
            height,
            buffer,
            final_warning_markers.unwrap(),
        );
    }
}

fn draw_editor_minimap_number_labels(
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    labels: &[EditorMinimapNumberLabel],
) {
    let mut last_label_y = f64::NEG_INFINITY;
    for label in labels.iter().copied() {
        if label.y - last_label_y < EDITOR_MINIMAP_LABEL_MIN_GAP_PX {
            continue;
        }
        last_label_y = label.y;

        let text = minimap_badge_text(label.kind, label.number);
        let (red, green, blue) = editor_minimap_structure_color(label.kind);
        cr.select_font_face(
            "monospace",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Bold,
        );
        cr.set_font_size(EDITOR_MINIMAP_LABEL_FONT_PX);
        let Ok(extents) = cr.text_extents(&text) else {
            continue;
        };

        let bubble_width = (extents.width() + EDITOR_MINIMAP_LABEL_PADDING_PX * 2.0).max(8.0);
        let bubble_height = (extents.height() + EDITOR_MINIMAP_LABEL_PADDING_PX * 2.0).max(8.0);
        let bubble_x = (width - EDITOR_MINIMAP_CONTENT_INSET_PX - bubble_width).max(1.0);
        let bubble_y = minimap_badge_y(label.y, bubble_height, height);
        let (text_red, text_green, text_blue) = minimap_badge_text_color(red, green, blue);

        cr.set_source_rgba(red, green, blue, 0.92);
        cr.rectangle(bubble_x, bubble_y, bubble_width, bubble_height);
        cr.fill().ok();

        cr.set_source_rgba(text_red, text_green, text_blue, 0.98);
        let text_x = bubble_x + (bubble_width - extents.width()) / 2.0 - extents.x_bearing();
        let text_y = bubble_y + (bubble_height - extents.height()) / 2.0 - extents.y_bearing();
        cr.move_to(text_x, text_y);
        cr.show_text(&text).ok();
    }
}

fn minimap_badge_text(kind: EditorMinimapStructureKind, number: Option<usize>) -> String {
    let prefix = match kind {
        EditorMinimapStructureKind::Intro => "I",
        EditorMinimapStructureKind::Verse => "V",
        EditorMinimapStructureKind::Hook => "H",
        EditorMinimapStructureKind::Bridge => "B",
        EditorMinimapStructureKind::Outro => "O",
        EditorMinimapStructureKind::Other => "",
    };

    match number {
        Some(number) if !prefix.is_empty() => format!("{prefix}{number}"),
        Some(number) => number.to_string(),
        None => prefix.to_owned(),
    }
}

fn minimap_badge_y(center_y: f64, bubble_height: f64, minimap_height: f64) -> f64 {
    let max_y = (minimap_height - bubble_height).max(0.0);
    (center_y - bubble_height / 2.0).clamp(0.0, max_y)
}

fn minimap_badge_text_color(red: f64, green: f64, blue: f64) -> (f64, f64, f64) {
    let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    if luminance > 0.58 {
        (0.07, 0.09, 0.12)
    } else {
        (0.96, 0.97, 0.99)
    }
}

fn draw_minimap_warning_symbols(
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    buffer: &gtk::TextBuffer,
    final_warning_markers: &Rc<RefCell<Vec<RepeatWarning>>>,
) {
    let text = buffer_text(buffer);
    let total_lines = text.lines().count().max(1) as f64;
    let markers = final_warning_markers.borrow();
    for warning in markers.iter() {
        let y = ((warning.line_index as f64 + 0.5) / total_lines) * height;
        let marker_size = 6.0;
        let x = width - EDITOR_MINIMAP_CONTENT_INSET_PX - marker_size * 1.2;
        match warning.kind {
            crate::services::live_highlights::RepeatWarningKind::Skull => {
                draw_minimap_circle(cr, x, y, marker_size, 1.0, 0.05, 0.08);
            }
            crate::services::live_highlights::RepeatWarningKind::Diamond => {
                draw_minimap_circle(cr, x, y, marker_size, 0.12, 0.93, 0.94);
            }
            crate::services::live_highlights::RepeatWarningKind::Triangle => {
                draw_minimap_circle(cr, x, y, marker_size, 0.95, 0.82, 0.26);
            }
        }
    }
}

fn draw_minimap_circle(
    cr: &gtk::cairo::Context,
    x: f64,
    y: f64,
    size: f64,
    red: f64,
    green: f64,
    blue: f64,
) {
    cr.set_source_rgba(red, green, blue, 0.9);
    cr.arc(x, y, size * 0.45, 0.0, std::f64::consts::TAU);
    cr.fill().ok();
}

fn set_adjustment_from_minimap_y(adjustment: &gtk::Adjustment, y: f64, height: f64) {
    let upper = adjustment.upper();
    let page_size = adjustment.page_size();
    let value = minimap_scroll_value_for_y(y, height, upper, page_size);
    adjustment.set_value(value);
}

fn minimap_scroll_value_for_y(y: f64, height: f64, upper: f64, page_size: f64) -> f64 {
    if height <= 0.0 {
        return 0.0;
    }
    let scrollable = (upper - page_size).max(0.0);
    if scrollable <= 0.0 {
        return 0.0;
    }
    let fraction = (y / height).clamp(0.0, 1.0);
    let centered = upper * fraction - page_size / 2.0;
    centered.clamp(0.0, scrollable)
}

fn minimap_viewport_geometry(height: f64, value: f64, upper: f64, page_size: f64) -> (f64, f64) {
    if height <= 0.0 || upper <= 0.0 {
        return (0.0, height.max(0.0));
    }

    let visible_fraction = (page_size / upper).clamp(0.0, 1.0);
    let viewport_height = if visible_fraction >= 1.0 {
        height
    } else {
        (height * visible_fraction)
            .max(EDITOR_MINIMAP_MIN_VIEWPORT_HEIGHT_PX)
            .min(height)
    };

    let scrollable = (upper - page_size).max(0.0);
    let travel = (height - viewport_height).max(0.0);
    let viewport_y = if scrollable <= 0.0 || travel <= 0.0 {
        0.0
    } else {
        (value.clamp(0.0, scrollable) / scrollable) * travel
    };

    (viewport_y, viewport_height)
}

fn parse_editor_minimap_structure_tag(line: &str) -> Option<EditorMinimapStructureTag> {
    let trimmed = line.trim();
    let bracketed = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = bracketed.split_whitespace();
    let normalized = parts.next().unwrap_or_default().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "intro" => EditorMinimapStructureKind::Intro,
        "verse" => EditorMinimapStructureKind::Verse,
        "hook" | "chorus" => EditorMinimapStructureKind::Hook,
        "bridge" => EditorMinimapStructureKind::Bridge,
        "outro" => EditorMinimapStructureKind::Outro,
        _ => EditorMinimapStructureKind::Other,
    };
    let number = parts.next().and_then(|value| value.parse::<usize>().ok());
    Some(EditorMinimapStructureTag { kind, number })
}

fn dominant_structure_kind(
    counts: &[u16; EDITOR_MINIMAP_STRUCTURE_KINDS],
) -> EditorMinimapStructureKind {
    let mut dominant_kind = EditorMinimapStructureKind::Other;
    let mut dominant_count = 0_u16;
    for kind in [
        EditorMinimapStructureKind::Intro,
        EditorMinimapStructureKind::Verse,
        EditorMinimapStructureKind::Hook,
        EditorMinimapStructureKind::Bridge,
        EditorMinimapStructureKind::Outro,
        EditorMinimapStructureKind::Other,
    ] {
        let count = counts[kind.index()];
        if count > dominant_count {
            dominant_count = count;
            dominant_kind = kind;
        }
    }
    dominant_kind
}

fn editor_minimap_structure_color(kind: EditorMinimapStructureKind) -> (f64, f64, f64) {
    match kind {
        EditorMinimapStructureKind::Intro => (0.56, 0.78, 1.0),
        EditorMinimapStructureKind::Verse => (0.34, 0.78, 0.42),
        EditorMinimapStructureKind::Hook => (1.0, 0.61, 0.27),
        EditorMinimapStructureKind::Bridge => (0.62, 0.35, 1.0),
        EditorMinimapStructureKind::Outro => (0.16, 0.32, 0.66),
        EditorMinimapStructureKind::Other => (0.78, 0.81, 0.86),
    }
}

impl EditorMinimapStructureKind {
    const fn index(self) -> usize {
        match self {
            Self::Intro => 0,
            Self::Verse => 1,
            Self::Hook => 2,
            Self::Bridge => 3,
            Self::Outro => 4,
            Self::Other => 5,
        }
    }

    const fn supports_badge(self) -> bool {
        matches!(
            self,
            Self::Intro | Self::Verse | Self::Hook | Self::Bridge | Self::Outro
        )
    }
}

fn pane_shell(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("pane-shell");
    let label = gtk::Label::new(Some(title));
    label.add_css_class("pane-title");
    label.set_xalign(0.0);
    shell.append(&label);
    shell.append(child);
    shell
}

fn install_font_css(font_size_pt: u16) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&format!(
        ".editor-view {{ font-size: {}pt; }}",
        font_size_pt
    ));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod editor_panes_unit_tests {
    use super::*;

    #[test]
    fn raw_pane_starts_on_left_at_forty_percent() {
        assert_eq!(((1000.0_f64) * RAW_PANE_WIDTH_FRACTION).round() as i32, 400);
    }

    #[test]
    fn raw_minimap_threshold_activates_after_hundred_lines() {
        assert_eq!(editor_line_count(""), 0);
        assert_eq!(editor_line_count("one line"), 1);
        assert_eq!(editor_line_count(&vec!["x"; 100].join("\n")), 100);
        assert_eq!(editor_line_count(&vec!["x"; 101].join("\n")), 101);
    }

    #[test]
    fn minimap_scroll_value_centers_target_position() {
        assert_eq!(minimap_scroll_value_for_y(0.0, 200.0, 1000.0, 200.0), 0.0);
        assert_eq!(
            minimap_scroll_value_for_y(200.0, 200.0, 1000.0, 200.0),
            800.0
        );
        assert_eq!(
            minimap_scroll_value_for_y(100.0, 200.0, 1000.0, 200.0),
            400.0
        );
    }

    #[test]
    fn minimap_viewport_geometry_clamps_to_visible_extent() {
        let (start, span) = minimap_viewport_geometry(300.0, 400.0, 1000.0, 200.0);
        assert!((start - 120.0).abs() < f64::EPSILON);
        assert!((span - 60.0).abs() < f64::EPSILON);

        let (full_start, full_span) = minimap_viewport_geometry(300.0, 0.0, 100.0, 140.0);
        assert_eq!(full_start, 0.0);
        assert_eq!(full_span, 300.0);
    }

    #[test]
    fn minimap_badges_use_structure_initials() {
        assert_eq!(
            minimap_badge_text(EditorMinimapStructureKind::Intro, None),
            "I"
        );
        assert_eq!(
            minimap_badge_text(EditorMinimapStructureKind::Verse, Some(12)),
            "V12"
        );
        assert_eq!(
            minimap_badge_text(EditorMinimapStructureKind::Hook, Some(2)),
            "H2"
        );
        assert_eq!(
            minimap_badge_text(EditorMinimapStructureKind::Bridge, Some(3)),
            "B3"
        );
        assert_eq!(
            minimap_badge_text(EditorMinimapStructureKind::Outro, None),
            "O"
        );
    }

    #[test]
    fn minimap_badge_y_keeps_bottom_badge_inside_viewport() {
        assert_eq!(minimap_badge_y(99.0, 14.0, 100.0), 86.0);
        assert_eq!(minimap_badge_y(1.0, 14.0, 100.0), 0.0);
        assert_eq!(minimap_badge_y(5.0, 120.0, 100.0), 0.0);
    }

    #[test]
    fn minimap_parses_structure_tag_kinds() {
        assert_eq!(
            parse_editor_minimap_structure_tag("[INTRO]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Intro,
                number: None,
            })
        );
        assert_eq!(
            parse_editor_minimap_structure_tag("[VERSE 12]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Verse,
                number: Some(12),
            })
        );
        assert_eq!(
            parse_editor_minimap_structure_tag("[HOOK 2]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Hook,
                number: Some(2),
            })
        );
        assert_eq!(
            parse_editor_minimap_structure_tag("[BRIDGE]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Bridge,
                number: None,
            })
        );
        assert_eq!(
            parse_editor_minimap_structure_tag("[OUTRO]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Outro,
                number: None,
            })
        );
        assert_eq!(
            parse_editor_minimap_structure_tag("[PRE CHORUS]"),
            Some(EditorMinimapStructureTag {
                kind: EditorMinimapStructureKind::Other,
                number: None,
            })
        );
        assert_eq!(parse_editor_minimap_structure_tag("plain text"), None);
    }
}
