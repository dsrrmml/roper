use crate::models::{CasingMode, UsedMaterial};
use crate::services::material_usage::{
    RawLineIdentity, contains_material, material_from_identity, raw_line_identities,
};
use gtk::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

const GUTTER_WIDTH: i32 = 36;
const MARKER_LANE_WIDTH: f64 = 14.0;
const NUMBER_LEFT_INSET: f64 = 3.0;
const NUMBER_RIGHT_INSET: f64 = 3.0;
const NUMBER_FONT_MIN_PX: f64 = 9.0;
const NUMBER_FONT_MAX_PX: f64 = 13.0;

pub const NUMBER_LANE_WIDTH: i32 = GUTTER_WIDTH - MARKER_LANE_WIDTH as i32;
const FINAL_NUMBER_RIGHT_INSET: f64 = 0.0;

#[derive(Clone)]
struct Marker {
    line_index: usize,
    line: String,
    entry: UsedMaterial,
    used: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn rebuild(
    gutter: &gtk::Box,
    raw_view: &gtk::TextView,
    raw_text: &str,
    casing_mode: CasingMode,
    used_material: &[UsedMaterial],
    _font_size_pt: u16,
    on_transfer: Rc<dyn Fn(String, UsedMaterial)>,
    on_unmark: Rc<dyn Fn(UsedMaterial)>,
) {
    while let Some(child) = gutter.first_child() {
        gutter.remove(&child);
    }

    let markers = Rc::new(markers_for_raw(raw_text, casing_mode, used_material));
    let layer = gtk::DrawingArea::new();
    layer.add_css_class("gutter-marker-layer");
    layer.set_size_request(GUTTER_WIDTH, -1);
    layer.set_hexpand(false);
    layer.set_vexpand(true);

    {
        let markers = markers.clone();
        let raw_view = raw_view.clone();
        layer.set_draw_func(move |layer, cr, width, height| {
            draw_markers(layer, cr, width as f64, height as f64, &raw_view, &markers);
        });
    }

    {
        let markers = markers.clone();
        let raw_view = raw_view.clone();
        let layer_for_popover = layer.clone();
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(marker) = marker_at(&layer_for_popover, &raw_view, &markers, x, y) else {
                return;
            };

            match gesture.current_button() {
                1 => on_transfer(marker.line.clone(), marker.entry.clone()),
                3 => {
                    show_unmark_popover(&layer_for_popover, x, y, marker.entry.clone(), &on_unmark)
                }
                _ => {}
            }
        });
        layer.add_controller(click);
    }

    gutter.append(&layer);
}

pub fn install_line_number_gutter(gutter: &gtk::Box, view: &gtk::TextView) {
    while let Some(child) = gutter.first_child() {
        gutter.remove(&child);
    }

    let layer = gtk::DrawingArea::new();
    layer.add_css_class("gutter-marker-layer");
    layer.set_size_request(NUMBER_LANE_WIDTH, -1);
    layer.set_hexpand(false);
    layer.set_vexpand(true);

    let view = view.clone();
    layer.set_draw_func(move |layer, cr, width, height| {
        draw_line_numbers_in_span(
            layer,
            cr,
            width as f64,
            height as f64,
            &view,
            NUMBER_LEFT_INSET,
            width as f64 - FINAL_NUMBER_RIGHT_INSET,
        );
    });

    gutter.append(&layer);
}

fn markers_for_raw(
    raw_text: &str,
    casing_mode: CasingMode,
    used_material: &[UsedMaterial],
) -> Vec<Marker> {
    let line_text = raw_text
        .lines()
        .enumerate()
        .map(|(index, line)| (index, line.to_owned()))
        .collect::<HashMap<_, _>>();

    raw_line_identities(raw_text, casing_mode)
        .into_iter()
        .filter_map(|identity| {
            let entry = material_from_identity(&identity);
            let line = line_text.get(&identity.line_index)?.clone();
            Some(Marker {
                line_index: identity.line_index,
                used: contains_material(used_material, &entry),
                entry,
                line,
            })
        })
        .collect()
}

fn draw_markers(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    raw_view: &gtk::TextView,
    markers: &[Marker],
) {
    draw_line_numbers(layer, cr, width, height, raw_view);

    for marker in markers {
        let Some((line_y, line_height)) = marker_layer_range(layer, raw_view, marker.line_index)
        else {
            continue;
        };
        if line_y + line_height < 0.0 || line_y > height {
            continue;
        }

        let center_y = line_y + line_height / 2.0;
        let x = width - MARKER_LANE_WIDTH / 2.0 - 1.0;
        if marker.used {
            cr.set_source_rgba(1.0, 0.24, 0.28, 0.92);
        } else {
            cr.set_source_rgba(0.62, 0.68, 0.74, 0.86);
        }
        cr.set_line_width(2.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        cr.move_to(x - 4.0, center_y - 5.0);
        cr.line_to(x + 2.0, center_y);
        cr.line_to(x - 4.0, center_y + 5.0);
        cr.stroke().ok();
    }
}

fn draw_line_numbers(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    raw_view: &gtk::TextView,
) {
    draw_line_numbers_in_span(
        layer,
        cr,
        width,
        height,
        raw_view,
        NUMBER_LEFT_INSET,
        width - MARKER_LANE_WIDTH - NUMBER_RIGHT_INSET,
    );
}

fn draw_line_numbers_in_span(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    view: &gtk::TextView,
    left_inset: f64,
    right_edge: f64,
) {
    let _ = width;
    let buffer = view.buffer();
    let line_count = buffer.line_count().max(1);
    for line_index in 0..line_count {
        let Some((line_y, line_height)) = marker_layer_range(layer, view, line_index as usize)
        else {
            continue;
        };
        if line_y + line_height < 0.0 || line_y > height {
            continue;
        }

        let label = (line_index + 1).to_string();
        let font_size = (line_height * 0.56).clamp(NUMBER_FONT_MIN_PX, NUMBER_FONT_MAX_PX);
        cr.select_font_face(
            "monospace",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        cr.set_font_size(font_size);
        cr.set_source_rgba(0.56, 0.61, 0.68, 0.92);
        let Ok(extents) = cr.text_extents(&label) else {
            continue;
        };
        let x = (right_edge - extents.width() - extents.x_bearing())
            .max(left_inset - extents.x_bearing());
        let y = line_y + ((line_height + font_size) / 2.0) - 2.0;
        cr.move_to(x, y);
        cr.show_text(&label).ok();
    }
}

fn marker_at(
    layer: &gtk::DrawingArea,
    raw_view: &gtk::TextView,
    markers: &[Marker],
    x: f64,
    y: f64,
) -> Option<Marker> {
    if x < layer.width() as f64 - MARKER_LANE_WIDTH - 4.0 {
        return None;
    }
    markers.iter().find_map(|marker| {
        let (line_y, line_height) = marker_layer_range(layer, raw_view, marker.line_index)?;
        if y >= line_y && y < line_y + line_height {
            Some(marker.clone())
        } else {
            None
        }
    })
}

fn marker_layer_range(
    layer: &gtk::DrawingArea,
    raw_view: &gtk::TextView,
    line_index: usize,
) -> Option<(f64, f64)> {
    let buffer = raw_view.buffer();
    let line_start = buffer.iter_at_line(line_index as i32)?;
    let (buffer_y, line_height) = raw_view.line_yrange(&line_start);
    let (_, window_y) = raw_view.buffer_to_window_coords(gtk::TextWindowType::Widget, 0, buffer_y);
    let (_, layer_y) = raw_view.translate_coordinates(layer, 0.0, window_y as f64)?;
    Some((layer_y, line_height.max(1) as f64))
}

fn show_unmark_popover(
    parent: &gtk::DrawingArea,
    x: f64,
    y: f64,
    entry: UsedMaterial,
    on_unmark: &Rc<dyn Fn(UsedMaterial)>,
) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let mark_unused = gtk::Button::with_label("Als unbenutzt markieren");
    popover.set_child(Some(&mark_unused));
    let unmark = on_unmark.clone();
    let popover_for_action = popover.clone();
    mark_unused.connect_clicked(move |_| {
        unmark(entry.clone());
        popover_for_action.popdown();
    });
    popover.popup();
}

pub fn apply_used_highlights(
    buffer: &gtk::TextBuffer,
    raw_text: &str,
    casing_mode: CasingMode,
    used_material: &[UsedMaterial],
) {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    if buffer.tag_table().lookup("used_material").is_some() {
        buffer.remove_tag_by_name("used_material", &start, &end);
    }

    let tag = ensure_used_tag(buffer);
    let identities = raw_line_identities(raw_text, casing_mode);
    let mut line_offsets = Vec::new();
    let mut offset = 0usize;
    for line in raw_text.lines() {
        let start = offset;
        let end = start + line.chars().count();
        line_offsets.push((start, end));
        offset = end + 1;
    }

    for identity in identities {
        let entry = material_from_identity(&identity);
        if contains_material(used_material, &entry) {
            if let Some((start, end)) = line_offsets.get(identity.line_index).copied() {
                let start_iter = buffer.iter_at_offset(start as i32);
                let end_iter = buffer.iter_at_offset(end as i32);
                buffer.apply_tag(&tag, &start_iter, &end_iter);
            }
        }
    }
}

pub fn current_raw_line(buffer: &gtk::TextBuffer) -> Option<String> {
    current_raw_line_identity(buffer, CasingMode::Preserve).map(|(line, _)| line)
}

pub fn current_raw_line_identity(
    buffer: &gtk::TextBuffer,
    casing_mode: CasingMode,
) -> Option<(String, UsedMaterial)> {
    let cursor = buffer.cursor_position();
    let text = buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string();
    let identities = identities_by_line(&text, casing_mode);
    let mut offset = 0usize;
    for (line_index, line) in text.lines().enumerate() {
        let end = offset + line.chars().count();
        if (offset..=end).contains(&(cursor.max(0) as usize)) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let identity = identities.get(&line_index)?;
            return Some((line.to_owned(), material_from_identity(identity)));
        }
        offset = end + 1;
    }
    None
}

fn identities_by_line(raw_text: &str, casing_mode: CasingMode) -> HashMap<usize, RawLineIdentity> {
    raw_line_identities(raw_text, casing_mode)
        .into_iter()
        .map(|identity| (identity.line_index, identity))
        .collect()
}

fn ensure_used_tag(buffer: &gtk::TextBuffer) -> gtk::TextTag {
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup("used_material") {
        return tag;
    }

    let tag = gtk::TextTag::builder().name("used_material").build();
    tag.set_background_rgba(Some(&gtk::gdk::RGBA::new(0.76, 0.08, 0.11, 0.27)));
    table.add(&tag);
    tag
}
