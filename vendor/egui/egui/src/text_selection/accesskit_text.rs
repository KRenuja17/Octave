use core::ops::Range;

use emath::TSTransform;
use epaint::text::{CharIndex, PlacedRow};

use crate::{Context, Galley, Id};

use super::{CCursorRange, text_cursor_state::is_word_char};

/// AccessKit's word offsets are u8. Each run must also be unidirectional:
/// adapters compute partial selection rectangles from one direction per node.
pub(crate) const MAX_CHARS_PER_TEXT_RUN: usize = 255;

pub(crate) fn text_run_ranges(row: &PlacedRow) -> Vec<Range<usize>> {
    let length = row.glyphs.len() + usize::from(row.ends_with_newline);
    let mut ranges = Vec::new();
    let mut start = 0;
    for i in 1..length {
        let direction_changed =
            i < row.glyphs.len() && row.glyphs[i].bidi_level != row.glyphs[i - 1].bidi_level;
        if i - start == MAX_CHARS_PER_TEXT_RUN || direction_changed {
            ranges.push(start..i);
            start = i;
        }
    }
    ranges.push(start..length);
    ranges
}

fn text_run_position(
    parent_id: Id,
    galley: &Galley,
    row: usize,
    column: CharIndex,
) -> accesskit::TextPosition {
    let ranges = text_run_ranges(&galley.rows[row]);
    // A boundary belongs to the previous run, matching the original chunk rule.
    let chunk = ranges
        .iter()
        .position(|range| column.0 <= range.end)
        .unwrap_or(ranges.len() - 1);
    accesskit::TextPosition {
        node: parent_id.with(row).with(chunk).accesskit_id(),
        character_index: column
            .0
            .saturating_sub(ranges[chunk].start)
            .min(ranges[chunk].len()),
    }
}

/// Expose logical Unicode text with direction-aware, transformed character bounds.
pub fn update_accesskit_for_text_widget(
    ctx: &Context,
    widget_id: Id,
    cursor_range: Option<CCursorRange>,
    role: accesskit::Role,
    global_from_galley: TSTransform,
    galley: &Galley,
) {
    ctx.accesskit_node_builder(widget_id, |builder| {
        if let Some(cursor_range) = &cursor_range {
            let anchor = galley.layout_from_cursor(cursor_range.secondary);
            let focus = galley.layout_from_cursor(cursor_range.primary);
            builder.set_text_selection(accesskit::TextSelection {
                anchor: text_run_position(widget_id, galley, anchor.row, anchor.column),
                focus: text_run_position(widget_id, galley, focus.row, focus.column),
            });
        }
        builder.set_role(role);
    });

    let mut prev_row_ended_with_newline = true;
    for (row_index, row) in galley.rows.iter().enumerate() {
        let ranges = text_run_ranges(row);
        let mut was_at_word_end = !prev_row_ended_with_newline;
        for (chunk, range) in ranges.iter().enumerate() {
            let rtl = row
                .glyphs
                .get(range.start)
                .or_else(|| row.glyphs.last())
                .is_some_and(|g| g.is_rtl());
            let mut value = String::new();
            let mut lengths = Vec::new();
            let mut spans = Vec::new();
            let mut word_starts = Vec::new();
            for i in range.clone() {
                let (chr, x, width) = if let Some(glyph) = row.glyphs.get(i) {
                    (glyph.chr, glyph.pos.x, glyph.advance_width)
                } else {
                    ('\n', row.x_offset(CharIndex(row.glyphs.len())), 0.0)
                };
                let word = is_word_char(chr);
                if word && was_at_word_end {
                    word_starts.push(lengths.len() as u8);
                }
                was_at_word_end = !word;
                value.push(chr);
                lengths.push(chr.len_utf8() as u8);
                spans.push((x, width));
            }
            let left = spans
                .iter()
                .map(|(x, _)| *x)
                .reduce(f32::min)
                .unwrap_or(0.0);
            let right = spans
                .iter()
                .map(|(x, w)| x + w)
                .reduce(f32::max)
                .unwrap_or(left);
            let rect = global_from_galley
                * emath::Rect::from_min_max(
                    row.pos + emath::vec2(left, 0.0),
                    row.pos + emath::vec2(right, row.size.y),
                );
            let scale = global_from_galley.scaling;
            let positions: Vec<f32> = spans
                .iter()
                .map(|(x, w)| (if rtl { right - x - w } else { x - left }) * scale)
                .collect();
            let widths: Vec<f32> = spans.iter().map(|(_, width)| width * scale).collect();
            let run_id = widget_id.with(row_index).with(chunk);
            ctx.register_accesskit_parent(run_id, widget_id);
            ctx.accesskit_node_builder(run_id, |builder| {
                builder.set_role(accesskit::Role::TextRun);
                builder.set_text_direction(if rtl {
                    accesskit::TextDirection::RightToLeft
                } else {
                    accesskit::TextDirection::LeftToRight
                });
                if chunk > 0 {
                    builder.set_previous_on_line(
                        widget_id.with(row_index).with(chunk - 1).accesskit_id(),
                    );
                }
                if chunk + 1 < ranges.len() {
                    builder
                        .set_next_on_line(widget_id.with(row_index).with(chunk + 1).accesskit_id());
                }
                builder.set_bounds(accesskit::Rect {
                    x0: rect.min.x.into(),
                    y0: rect.min.y.into(),
                    x1: rect.max.x.into(),
                    y1: rect.max.y.into(),
                });
                builder.set_value(value);
                builder.set_character_lengths(lengths);
                builder.set_character_positions(positions);
                builder.set_character_widths(widths);
                builder.set_word_starts(word_starts);
            });
        }
        prev_row_ended_with_newline = row.ends_with_newline;
    }
}
