use crate::services::live_highlights::{
    LiveHighlights, PaneHighlights, REPEAT_BUCKETS, RepeatWarning, RepeatWarningKind,
    STRUCTURE_BUCKETS, StructureKind,
};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const CHAIN_TAG: &str = "live_chain";
const REPEAT_TAG_PREFIX: &str = "live_repeat_";
const STRUCTURE_TAG_PREFIX: &str = "live_structure_";
const WARNING_ICON_SIZE: f64 = 12.0;
const WARNING_ICON_HALF_WIDTH: f64 = WARNING_ICON_SIZE * 0.72;
const WARNING_ICON_GAP: f64 = 4.0;
const WARNING_ICON_SLOT_GAP: f64 = 3.0;

#[derive(Clone, Copy, Debug)]
struct WarningMarkerAnchor {
    word_end_x: f64,
    next_char_x: Option<f64>,
    line_end_x: f64,
    center_y: f64,
    line_index: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WarningMarkerSymbol {
    Skull,
    Diamond,
    Triangle,
    Orange,
    Violet,
    Gray,
}

pub fn apply(
    raw_buffer: &gtk::TextBuffer,
    final_buffer: &gtk::TextBuffer,
    final_view: &gtk::TextView,
    final_warning_layer: &gtk::DrawingArea,
    show_empty_line_pattern: bool,
    raw_text: &str,
    final_text: &str,
    final_warning_markers: &Rc<RefCell<Vec<RepeatWarning>>>,
) {
    let highlights = crate::services::live_highlights::analyze(raw_text, final_text);
    apply_highlights(
        raw_buffer,
        final_buffer,
        final_view,
        final_warning_layer,
        show_empty_line_pattern,
        &highlights,
        final_warning_markers,
    );
}

pub fn apply_highlights(
    raw_buffer: &gtk::TextBuffer,
    final_buffer: &gtk::TextBuffer,
    final_view: &gtk::TextView,
    final_warning_layer: &gtk::DrawingArea,
    show_empty_line_pattern: bool,
    highlights: &LiveHighlights,
    final_warning_markers: &Rc<RefCell<Vec<RepeatWarning>>>,
) {
    apply_to_buffer(raw_buffer, &highlights.raw);
    apply_to_buffer(final_buffer, &highlights.final_);
    *final_warning_markers.borrow_mut() = highlights.final_.warnings.clone();
    final_warning_layer.set_tooltip_text(warning_summary(&highlights.final_.warnings).as_deref());
    apply_warning_layer(
        final_warning_layer,
        final_view,
        highlights.final_.warnings.clone(),
        show_empty_line_pattern,
    );
}

fn warning_summary(warnings: &[RepeatWarning]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
    for warning in warnings {
        *by_kind.entry(warning_kind_label(warning.kind)).or_default() += 1;
    }
    let mut parts = by_kind
        .into_iter()
        .map(|(label, count)| format!("{label}: {count}"))
        .collect::<Vec<_>>();
    parts.sort();
    Some(parts.join("\n"))
}

fn warning_kind_label(kind: RepeatWarningKind) -> &'static str {
    match kind {
        RepeatWarningKind::ScatteredWeakWord => "scattered weak word",
        RepeatWarningKind::AdjacentRepetition => "adjacent repetition",
        RepeatWarningKind::HookRepetition => "hook repetition",
        RepeatWarningKind::WordFamilyEcho => "word family echo",
        RepeatWarningKind::PhraseEcho => "phrase echo",
        RepeatWarningKind::RepeatedLine => "repeated line",
    }
}

fn apply_to_buffer(buffer: &gtk::TextBuffer, highlights: &PaneHighlights) {
    clear_live_tags(buffer);

    for structure in &highlights.structures {
        let tag = ensure_structure_tag(buffer, structure.kind, structure.bucket);
        let start = buffer.iter_at_offset(structure.range.start as i32);
        let end = buffer.iter_at_offset(structure.range.end as i32);
        buffer.apply_tag(&tag, &start, &end);
    }

    let chain_tag = ensure_chain_tag(buffer);
    for range in &highlights.chains {
        let start = buffer.iter_at_offset(range.start as i32);
        let end = buffer.iter_at_offset(range.end as i32);
        buffer.apply_tag(&chain_tag, &start, &end);
    }

    let repeat_tags = ensure_repeat_tags(buffer);
    for repeat in &highlights.repeats {
        let Some(tag) = repeat_tags.get(repeat.bucket) else {
            continue;
        };
        let start = buffer.iter_at_offset(repeat.range.start as i32);
        let end = buffer.iter_at_offset(repeat.range.end as i32);
        buffer.apply_tag(tag, &start, &end);
    }
}

fn clear_live_tags(buffer: &gtk::TextBuffer) {
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    for name in live_tag_names() {
        if buffer.tag_table().lookup(&name).is_some() {
            buffer.remove_tag_by_name(&name, &start, &end);
        }
    }
}

fn live_tag_names() -> Vec<String> {
    let mut names = vec![CHAIN_TAG.to_owned()];
    names.extend((0..REPEAT_BUCKETS).map(repeat_tag_name));
    for kind in structure_kinds() {
        names.extend((0..STRUCTURE_BUCKETS).map(move |bucket| structure_tag_name(kind, bucket)));
    }
    names
}

fn ensure_chain_tag(buffer: &gtk::TextBuffer) -> gtk::TextTag {
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup(CHAIN_TAG) {
        return tag;
    }

    let tag = gtk::TextTag::builder().name(CHAIN_TAG).build();
    tag.set_background_rgba(Some(&gtk::gdk::RGBA::new(1.0, 0.48, 0.0, 0.25)));
    table.add(&tag);
    tag
}

fn ensure_repeat_tags(buffer: &gtk::TextBuffer) -> Vec<gtk::TextTag> {
    (0..REPEAT_BUCKETS)
        .map(|index| ensure_repeat_tag(buffer, index))
        .collect()
}

fn ensure_repeat_tag(buffer: &gtk::TextBuffer, index: usize) -> gtk::TextTag {
    let name = repeat_tag_name(index);
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup(&name) {
        return tag;
    }

    let alpha = match index {
        0 => 0.18,
        1 => 0.25,
        2 => 0.33,
        3 => 0.42,
        4 => 0.52,
        _ => 0.64,
    };
    let tag = gtk::TextTag::builder().name(&name).build();
    tag.set_background_rgba(Some(&gtk::gdk::RGBA::new(0.95, 0.05, 0.08, alpha)));
    table.add(&tag);
    tag
}

fn repeat_tag_name(index: usize) -> String {
    format!("{REPEAT_TAG_PREFIX}{index}")
}

fn ensure_structure_tag(
    buffer: &gtk::TextBuffer,
    kind: StructureKind,
    bucket: usize,
) -> gtk::TextTag {
    let name = structure_tag_name(kind, bucket);
    let table = buffer.tag_table();
    if let Some(tag) = table.lookup(&name) {
        return tag;
    }

    let tag = gtk::TextTag::builder().name(&name).build();
    tag.set_background_rgba(Some(&structure_rgba(kind, bucket)));
    table.add(&tag);
    tag
}

fn structure_kinds() -> [StructureKind; 4] {
    [
        StructureKind::Intro,
        StructureKind::Verse,
        StructureKind::Hook,
        StructureKind::Outro,
    ]
}

fn structure_tag_name(kind: StructureKind, bucket: usize) -> String {
    format!(
        "{STRUCTURE_TAG_PREFIX}{}_{}",
        structure_kind_name(kind),
        bucket.min(STRUCTURE_BUCKETS - 1)
    )
}

pub fn structure_rgba(kind: StructureKind, bucket: usize) -> gtk::gdk::RGBA {
    let level = bucket.min(STRUCTURE_BUCKETS - 1) as f32 / (STRUCTURE_BUCKETS - 1) as f32;
    match kind {
        StructureKind::Intro => gtk::gdk::RGBA::new(0.48, 0.76, 1.0, 0.24),
        StructureKind::Verse => {
            gtk::gdk::RGBA::new(0.36, 0.72 + level * 0.20, 0.44, 0.18 + level * 0.12)
        }
        StructureKind::Hook => {
            gtk::gdk::RGBA::new(1.0, 0.56 + level * 0.20, 0.20, 0.18 + level * 0.14)
        }
        StructureKind::Outro => gtk::gdk::RGBA::new(0.10, 0.22, 0.48, 0.28),
    }
}

fn structure_kind_name(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Intro => "intro",
        StructureKind::Verse => "verse",
        StructureKind::Hook => "hook",
        StructureKind::Outro => "outro",
    }
}

fn apply_warning_layer(
    layer: &gtk::DrawingArea,
    final_view: &gtk::TextView,
    warnings: Vec<RepeatWarning>,
    _show_empty_line_pattern: bool,
) {
    let warnings = Rc::new(warnings);
    let final_view = final_view.clone();
    layer.set_draw_func(move |layer, cr, width, height| {
        draw_warning_markers(
            layer,
            cr,
            width as f64,
            height as f64,
            &final_view,
            &warnings,
        );
    });
    layer.queue_draw();
}

pub fn draw_empty_line_pattern(
    layer: &gtk::DrawingArea,
    final_view: &gtk::TextView,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    enabled: bool,
) {
    if !enabled {
        return;
    }

    let view_origin = final_view
        .translate_coordinates(layer, 0.0, 0.0)
        .unwrap_or((0.0, 0.0));
    let buffer = final_view.buffer();
    let mut iter = buffer.start_iter();
    let mut last_text_line_bottom: Option<f64> = None;

    while iter.offset() < buffer.char_count() as i32 {
        let line_start = iter.clone();
        let mut line_end = iter.clone();
        line_end.forward_to_line_end();
        let line_text = buffer.slice(&line_start, &line_end, false);
        let is_empty_line = line_text.trim().is_empty();
        let (line_y, line_height) = final_view.line_yrange(&line_start);

        let line_top = view_origin.1 + line_y as f64;
        let line_bottom = view_origin.1 + (line_y + line_height) as f64;
        last_text_line_bottom = Some(line_bottom.max(last_text_line_bottom.unwrap_or(line_bottom)));

        if is_empty_line {
            let line_span = (line_bottom - line_top).max(1.0);
            let _ = cr.save();
            cr.rectangle(0.0, line_top, width, line_span);
            cr.clip();
            cr.set_source_rgba(0.72, 0.68, 0.90, 0.56);
            cr.set_line_width(1.4);
            let step = 8.0;
            let diagonal = line_span * 2.4;
            let min_x = -width;
            let max_x = width * 2.0;
            let mut x = min_x;
            while x <= max_x {
                cr.move_to(x, line_top);
                cr.line_to(x + diagonal, line_bottom);
                x += step;
            }
            cr.stroke().ok();
            let _ = cr.restore();
        }

        if !iter.forward_line() {
            break;
        }
    }

    if let Some(last_bottom) = last_text_line_bottom {
        let text_end_iter = buffer.end_iter();
        let (_end_y, end_height) = final_view.line_yrange(&text_end_iter);
        let empty_region_top = last_bottom;
        if empty_region_top < height {
            let empty_span = height - empty_region_top;
            let _ = cr.save();
            cr.rectangle(0.0, empty_region_top, width, empty_span);
            cr.clip();
            cr.set_source_rgba(0.72, 0.68, 0.90, 0.56);
            cr.set_line_width(1.4);
            let step = 8.0;
            let diagonal = (end_height.max(18) as f64) * 2.4;
            let min_x = -width;
            let max_x = width * 2.0;
            let mut x = min_x;
            while x <= max_x {
                cr.move_to(x, empty_region_top);
                cr.line_to(x + diagonal, empty_region_top + empty_span);
                x += step;
            }
            cr.stroke().ok();
            let _ = cr.restore();
        }
    }

    let _ = height;
}

fn draw_warning_markers(
    layer: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: f64,
    height: f64,
    final_view: &gtk::TextView,
    warnings: &[RepeatWarning],
) {
    let mut primary_by_line: HashMap<usize, (RepeatWarning, usize)> = HashMap::new();
    for warning in warnings {
        primary_by_line
            .entry(warning.line_index)
            .and_modify(|(current, count)| {
                *count += 1;
                if warning_severity(warning) > warning_severity(current) {
                    *current = warning.clone();
                }
            })
            .or_insert_with(|| (warning.clone(), 1));
    }

    let mut fallback_slots_by_line = HashMap::new();
    for (warning, count) in primary_by_line.values() {
        let Some(anchor) = warning_anchor(layer, final_view, warning) else {
            continue;
        };
        if anchor.center_y < -WARNING_ICON_SIZE || anchor.center_y > height + WARNING_ICON_SIZE {
            continue;
        }

        let fallback_slot = if warning_marker_fits_inline(&anchor) {
            0
        } else {
            let slot = fallback_slots_by_line
                .entry(anchor.line_index)
                .and_modify(|slot| *slot += 1)
                .or_insert(0);
            *slot
        };
        let x = warning_marker_x(width, &anchor, fallback_slot);
        match warning_marker_symbol(warning) {
            WarningMarkerSymbol::Skull => {
                draw_polygon_skull(cr, x, anchor.center_y, WARNING_ICON_SIZE)
            }
            WarningMarkerSymbol::Diamond => {
                draw_polygon_diamond(cr, x, anchor.center_y, WARNING_ICON_SIZE)
            }
            WarningMarkerSymbol::Triangle => {
                draw_polygon_triangle(cr, x, anchor.center_y, WARNING_ICON_SIZE)
            }
            WarningMarkerSymbol::Orange => {
                draw_marker_circle(cr, x, anchor.center_y, WARNING_ICON_SIZE, 1.0, 0.48, 0.0)
            }
            WarningMarkerSymbol::Violet => {
                draw_marker_circle(cr, x, anchor.center_y, WARNING_ICON_SIZE, 0.64, 0.36, 1.0)
            }
            WarningMarkerSymbol::Gray => {
                draw_marker_circle(cr, x, anchor.center_y, WARNING_ICON_SIZE, 0.58, 0.60, 0.64)
            }
        }
        if *count > 1 {
            draw_warning_count_badge(cr, x + WARNING_ICON_SIZE * 0.62, anchor.center_y, *count);
        }
    }
}

fn warning_marker_symbol(warning: &RepeatWarning) -> WarningMarkerSymbol {
    match warning.kind {
        RepeatWarningKind::ScatteredWeakWord => WarningMarkerSymbol::Skull,
        RepeatWarningKind::AdjacentRepetition => WarningMarkerSymbol::Triangle,
        RepeatWarningKind::HookRepetition => WarningMarkerSymbol::Diamond,
        RepeatWarningKind::WordFamilyEcho => WarningMarkerSymbol::Orange,
        RepeatWarningKind::PhraseEcho => WarningMarkerSymbol::Violet,
        RepeatWarningKind::RepeatedLine => WarningMarkerSymbol::Gray,
    }
}

fn warning_severity(warning: &RepeatWarning) -> usize {
    match warning.kind {
        RepeatWarningKind::ScatteredWeakWord => 6,
        RepeatWarningKind::AdjacentRepetition => 5,
        RepeatWarningKind::HookRepetition => 4,
        RepeatWarningKind::WordFamilyEcho => 3,
        RepeatWarningKind::PhraseEcho => 2,
        RepeatWarningKind::RepeatedLine => 1,
    }
}

fn draw_warning_count_badge(cr: &gtk::cairo::Context, x: f64, y: f64, count: usize) {
    let label = count.min(9).to_string();
    cr.set_source_rgba(0.08, 0.09, 0.11, 0.92);
    cr.arc(x, y - 5.0, 5.0, 0.0, std::f64::consts::TAU);
    cr.fill().ok();
    cr.set_source_rgba(0.96, 0.97, 0.99, 0.96);
    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Bold,
    );
    cr.set_font_size(7.0);
    cr.move_to(x - 2.0, y - 2.4);
    cr.show_text(&label).ok();
}

fn warning_anchor(
    layer: &gtk::DrawingArea,
    final_view: &gtk::TextView,
    warning: &RepeatWarning,
) -> Option<WarningMarkerAnchor> {
    let buffer = final_view.buffer();
    let iter = buffer.iter_at_offset(warning.range.end as i32);
    let line_index = iter.line();
    let location = final_view.iter_location(&iter);
    let (line_y, line_height) = final_view.line_yrange(&iter);
    let buffer_x = location.x() + location.width();
    let buffer_y = line_y + line_height / 2;
    let line_end_x = line_end_buffer_x(final_view, &iter).unwrap_or(buffer_x);
    let next_char_x = next_non_whitespace_buffer_x(final_view, &iter, line_index);

    Some(WarningMarkerAnchor {
        word_end_x: buffer_point_to_layer(layer, final_view, buffer_x, buffer_y)?.0,
        next_char_x: next_char_x
            .and_then(|x| buffer_point_to_layer(layer, final_view, x, buffer_y))
            .map(|(x, _)| x),
        line_end_x: buffer_point_to_layer(layer, final_view, line_end_x, buffer_y)?.0,
        center_y: buffer_point_to_layer(layer, final_view, buffer_x, buffer_y)?.1,
        line_index,
    })
}

fn buffer_point_to_layer(
    layer: &gtk::DrawingArea,
    final_view: &gtk::TextView,
    buffer_x: i32,
    buffer_y: i32,
) -> Option<(f64, f64)> {
    let (window_x, window_y) =
        final_view.buffer_to_window_coords(gtk::TextWindowType::Widget, buffer_x, buffer_y);
    final_view.translate_coordinates(layer, window_x as f64, window_y as f64)
}

fn line_end_buffer_x(final_view: &gtk::TextView, iter: &gtk::TextIter) -> Option<i32> {
    let mut line_end = *iter;
    line_end.forward_to_line_end();
    let location = final_view.iter_location(&line_end);
    Some(location.x() + location.width())
}

fn next_non_whitespace_buffer_x(
    final_view: &gtk::TextView,
    iter: &gtk::TextIter,
    line_index: i32,
) -> Option<i32> {
    let mut next = *iter;
    loop {
        if next.line() != line_index {
            return None;
        }

        let ch = next.char();
        if ch == '\0' || ch == '\n' {
            return None;
        }

        if !ch.is_whitespace() {
            return Some(final_view.iter_location(&next).x());
        }

        if !next.forward_char() {
            return None;
        }
    }
}

fn warning_marker_fits_inline(anchor: &WarningMarkerAnchor) -> bool {
    anchor.next_char_x.is_none_or(|next_x| {
        anchor.word_end_x + WARNING_ICON_GAP + WARNING_ICON_HALF_WIDTH * 2.0 + WARNING_ICON_GAP
            <= next_x
    })
}

fn warning_marker_x(width: f64, anchor: &WarningMarkerAnchor, fallback_slot: usize) -> f64 {
    let x = if warning_marker_fits_inline(anchor) {
        anchor.word_end_x + WARNING_ICON_GAP + WARNING_ICON_HALF_WIDTH
    } else {
        anchor.line_end_x
            + WARNING_ICON_GAP
            + WARNING_ICON_HALF_WIDTH
            + fallback_slot as f64 * (WARNING_ICON_HALF_WIDTH * 2.0 + WARNING_ICON_SLOT_GAP)
    };

    if width > WARNING_ICON_HALF_WIDTH * 2.0 {
        x.clamp(WARNING_ICON_HALF_WIDTH, width - WARNING_ICON_HALF_WIDTH)
    } else {
        width / 2.0
    }
}

fn draw_polygon_skull(cr: &gtk::cairo::Context, x: f64, y: f64, size: f64) {
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_source_rgba(1.0, 0.05, 0.08, 0.88);
    cr.move_to(x - size * 0.48, y - size * 0.48);
    cr.line_to(x, y - size * 0.68);
    cr.line_to(x + size * 0.48, y - size * 0.48);
    cr.line_to(x + size * 0.62, y - size * 0.05);
    cr.line_to(x + size * 0.34, y + size * 0.38);
    cr.line_to(x + size * 0.20, y + size * 0.62);
    cr.line_to(x - size * 0.20, y + size * 0.62);
    cr.line_to(x - size * 0.34, y + size * 0.38);
    cr.line_to(x - size * 0.62, y - size * 0.05);
    cr.close_path();
    cr.fill().ok();

    cr.set_source_rgba(0.10, 0.0, 0.02, 0.86);
    cr.move_to(x - size * 0.30, y - size * 0.14);
    cr.line_to(x - size * 0.09, y - size * 0.24);
    cr.line_to(x - size * 0.12, y + size * 0.02);
    cr.close_path();
    cr.fill().ok();

    cr.move_to(x + size * 0.30, y - size * 0.14);
    cr.line_to(x + size * 0.09, y - size * 0.24);
    cr.line_to(x + size * 0.12, y + size * 0.02);
    cr.close_path();
    cr.fill().ok();

    cr.move_to(x, y + size * 0.04);
    cr.line_to(x + size * 0.09, y + size * 0.22);
    cr.line_to(x - size * 0.09, y + size * 0.22);
    cr.close_path();
    cr.fill().ok();

    cr.set_source_rgba(0.10, 0.0, 0.02, 0.55);
    cr.set_line_width(1.0);
    for offset in [-0.16, 0.0, 0.16] {
        cr.move_to(x + size * offset, y + size * 0.40);
        cr.line_to(x + size * offset, y + size * 0.58);
    }
    cr.stroke().ok();
}

fn draw_marker_circle(
    cr: &gtk::cairo::Context,
    x: f64,
    y: f64,
    size: f64,
    red: f64,
    green: f64,
    blue: f64,
) {
    cr.set_source_rgba(red, green, blue, 0.92);
    cr.arc(x, y, size * 0.44, 0.0, std::f64::consts::TAU);
    cr.fill().ok();
}

fn draw_polygon_diamond(cr: &gtk::cairo::Context, x: f64, y: f64, size: f64) {
    cr.set_source_rgba(0.12, 0.93, 0.94, 0.90);
    cr.move_to(x, y - size * 0.45);
    cr.line_to(x + size * 0.35, y);
    cr.line_to(x, y + size * 0.45);
    cr.line_to(x - size * 0.35, y);
    cr.close_path();
    cr.fill().ok();
}

fn draw_polygon_triangle(cr: &gtk::cairo::Context, x: f64, y: f64, size: f64) {
    cr.set_source_rgba(0.95, 0.82, 0.26, 0.90);
    cr.move_to(x, y - size * 0.48);
    cr.line_to(x + size * 0.42, y + size * 0.34);
    cr.line_to(x - size * 0.42, y + size * 0.34);
    cr.close_path();
    cr.fill().ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::live_highlights::{HighlightRange, RepeatWarningKind};

    #[test]
    fn warning_marker_uses_inline_gap_when_it_cannot_touch_next_character() {
        let anchor = WarningMarkerAnchor {
            word_end_x: 20.0,
            next_char_x: Some(80.0),
            line_end_x: 120.0,
            center_y: 0.0,
            line_index: 0,
        };

        let x = warning_marker_x(200.0, &anchor, 0);

        assert!(x + WARNING_ICON_HALF_WIDTH + WARNING_ICON_GAP <= 80.0);
    }

    #[test]
    fn warning_marker_moves_to_line_end_when_next_character_is_too_close() {
        let anchor = WarningMarkerAnchor {
            word_end_x: 20.0,
            next_char_x: Some(28.0),
            line_end_x: 70.0,
            center_y: 0.0,
            line_index: 0,
        };

        let x = warning_marker_x(200.0, &anchor, 0);

        assert!(x - WARNING_ICON_HALF_WIDTH >= 70.0 + WARNING_ICON_GAP);
        assert!(x - WARNING_ICON_HALF_WIDTH > 28.0);
    }

    #[test]
    fn warning_marker_line_end_fallback_uses_slots_for_multiple_markers() {
        let anchor = WarningMarkerAnchor {
            word_end_x: 20.0,
            next_char_x: Some(28.0),
            line_end_x: 70.0,
            center_y: 0.0,
            line_index: 0,
        };

        let first = warning_marker_x(200.0, &anchor, 0);
        let second = warning_marker_x(200.0, &anchor, 1);

        assert!(second > first);
    }

    #[test]
    fn final_redundancy_warnings_render_matching_symbols() {
        for (kind, symbol) in [
            (
                RepeatWarningKind::ScatteredWeakWord,
                WarningMarkerSymbol::Skull,
            ),
            (
                RepeatWarningKind::AdjacentRepetition,
                WarningMarkerSymbol::Triangle,
            ),
            (
                RepeatWarningKind::HookRepetition,
                WarningMarkerSymbol::Diamond,
            ),
            (
                RepeatWarningKind::WordFamilyEcho,
                WarningMarkerSymbol::Orange,
            ),
            (RepeatWarningKind::PhraseEcho, WarningMarkerSymbol::Violet),
            (RepeatWarningKind::RepeatedLine, WarningMarkerSymbol::Gray),
        ] {
            let warning = RepeatWarning {
                range: HighlightRange { start: 0, end: 5 },
                kind,
                line_index: 0,
            };

            assert_eq!(warning_marker_symbol(&warning), symbol);
        }
    }
}
