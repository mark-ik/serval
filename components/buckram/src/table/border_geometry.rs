// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! K4g5's final collapsed-border geometry.
//!
//! Conflict resolution and metric projection happen before this module. It
//! consumes that one resolved atomic winner grid together with the final K4g4
//! table lines, and produces CSS-pixel paint strips without reselecting a
//! winner or rounding to a device scale.

use std::collections::BTreeSet;

use crate::{BoxId, LogicalRect};

use super::{
    GridEdgeOrientation, ResolvedTableBorderGrid, TableBorderStyle, TableFragmentRole,
    TableFragments, TableGridEdge,
};

/// Final logical grid-line positions, relative to the table grid fragment.
///
/// Every line, including the two outer lines, comes from the final K4g4 row
/// and column track fragments. The grid fragment can include collapsed outer
/// metric space and must not move a border away from its track edge.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGridLines {
    pub inline: Vec<f32>,
    pub block: Vec<f32>,
}

impl TableGridLines {
    /// Recover final table lines from the K4d6 fragment model.
    pub fn from_fragments(
        fragments: &TableFragments,
    ) -> Result<Self, CollapsedBorderGeometryError> {
        let grid = fragments
            .grid()
            .ok_or(CollapsedBorderGeometryError::MissingGridFragment)?;
        let rows = fragments
            .fragments()
            .iter()
            .filter(|fragment| fragment.role == TableFragmentRole::Row)
            .collect::<Vec<_>>();
        let columns = fragments
            .fragments()
            .iter()
            .filter(|fragment| fragment.role == TableFragmentRole::Column)
            .collect::<Vec<_>>();

        let inline = match (columns.first(), columns.last()) {
            (Some(first), Some(last)) => {
                let mut lines = Vec::with_capacity(columns.len().saturating_add(1));
                lines.push(first.rect.inline_start);
                lines.extend(
                    columns
                        .iter()
                        .take(columns.len().saturating_sub(1))
                        .map(|column| column.rect.inline_start + column.rect.inline_size),
                );
                lines.push(last.rect.inline_start + last.rect.inline_size);
                lines
            },
            // Empty tracks have no atomic cell edge to paint. Preserve the
            // grid endpoints so validation stays total for an empty table.
            (None, None) => vec![
                grid.rect.inline_start,
                grid.rect.inline_start + grid.rect.inline_size,
            ],
            _ => unreachable!("first and last column presence agree"),
        };

        let block = match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => {
                let mut lines = Vec::with_capacity(rows.len().saturating_add(1));
                lines.push(first.rect.block_start);
                lines.extend(
                    rows.iter()
                        .take(rows.len().saturating_sub(1))
                        .map(|row| row.rect.block_start + row.rect.block_size),
                );
                lines.push(last.rect.block_start + last.rect.block_size);
                lines
            },
            // See the matching inline-axis empty-table case above.
            (None, None) => vec![
                grid.rect.block_start,
                grid.rect.block_start + grid.rect.block_size,
            ],
            _ => unreachable!("first and last row presence agree"),
        };

        let lines = Self { inline, block };
        lines.validate()?;
        Ok(lines)
    }

    fn validate(&self) -> Result<(), CollapsedBorderGeometryError> {
        validate_lines(&self.inline, GridEdgeOrientation::BlockRunning)?;
        validate_lines(&self.block, GridEdgeOrientation::InlineRunning)
    }
}

/// One resolved winner lowered to its final logical CSS-pixel paint strip.
///
/// A table consumer turns this rectangle into one or more neutral commands
/// for the requested style. The winner and table identities remain attached
/// here so a paint consumer never needs to revisit candidate selection.
#[derive(Clone, Debug, PartialEq)]
pub struct CollapsedBorderPaintSegment<Color> {
    pub table: BoxId,
    pub edge: TableGridEdge,
    pub winner: BoxId,
    pub style: TableBorderStyle,
    pub color: Color,
    pub rect: LogicalRect,
}

/// The final, one-winner-per-atomic-edge collapsed-border paint model.
#[derive(Clone, Debug, PartialEq)]
pub struct CollapsedBorderGeometry<Color> {
    pub table: BoxId,
    pub segments: Vec<CollapsedBorderPaintSegment<Color>>,
}

/// The resolved grid cannot be lowered against the final table lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollapsedBorderGeometryError {
    MissingGridFragment,
    InvalidGridLine {
        orientation: GridEdgeOrientation,
        index: usize,
    },
    DecreasingGridLine {
        orientation: GridEdgeOrientation,
        index: usize,
    },
    InvalidEdge {
        edge: TableGridEdge,
    },
    DuplicateEdge {
        edge: TableGridEdge,
    },
    InvalidWinnerWidth {
        edge: TableGridEdge,
    },
}

/// Lower resolved atomic winners to CSS-pixel strips.
///
/// Segments remain in deterministic edge order. At a multi-way join each
/// winner owns its full centered strip and later command lowering follows this
/// order, so overlap is deliberate rather than a device-rounded accident.
pub fn resolve_collapsed_border_geometry<Color: Clone>(
    table: BoxId,
    lines: &TableGridLines,
    resolved: &ResolvedTableBorderGrid<Color>,
) -> Result<CollapsedBorderGeometry<Color>, CollapsedBorderGeometryError> {
    lines.validate()?;
    let mut seen = BTreeSet::new();
    let mut segments = Vec::with_capacity(resolved.segments.len());

    for resolved in &resolved.segments {
        if !seen.insert(resolved.edge) {
            return Err(CollapsedBorderGeometryError::DuplicateEdge {
                edge: resolved.edge,
            });
        }
        let Some(winner) = resolved.winner.as_ref() else {
            continue;
        };
        if resolved.suppressed_by_hidden || winner.style.draws_nothing() {
            continue;
        }
        if !winner.width.is_finite() || winner.width <= 0.0 {
            return Err(CollapsedBorderGeometryError::InvalidWinnerWidth {
                edge: resolved.edge,
            });
        }
        let rect = segment_rect(resolved.edge, winner.width, lines)?;
        segments.push(CollapsedBorderPaintSegment {
            table,
            edge: resolved.edge,
            winner: winner.source,
            style: winner.style.collapsed_paint_style(),
            color: winner.color.clone(),
            rect,
        });
    }

    segments.sort_by_key(|segment| segment.edge);
    Ok(CollapsedBorderGeometry { table, segments })
}

fn validate_lines(
    lines: &[f32],
    orientation: GridEdgeOrientation,
) -> Result<(), CollapsedBorderGeometryError> {
    for (index, line) in lines.iter().copied().enumerate() {
        if !line.is_finite() {
            return Err(CollapsedBorderGeometryError::InvalidGridLine { orientation, index });
        }
        if index > 0 && line < lines[index - 1] {
            return Err(CollapsedBorderGeometryError::DecreasingGridLine { orientation, index });
        }
    }
    Ok(())
}

fn segment_rect(
    edge: TableGridEdge,
    width: f32,
    lines: &TableGridLines,
) -> Result<LogicalRect, CollapsedBorderGeometryError> {
    let half_width = width * 0.5;
    let rect = match edge.orientation {
        GridEdgeOrientation::InlineRunning => {
            let block = *lines
                .block
                .get(edge.line)
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            let inline_start = *lines
                .inline
                .get(edge.segment)
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            let inline_end = *lines
                .inline
                .get(edge.segment.saturating_add(1))
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            LogicalRect {
                inline_start,
                block_start: block - half_width,
                inline_size: inline_end - inline_start,
                block_size: width,
            }
        },
        GridEdgeOrientation::BlockRunning => {
            let inline = *lines
                .inline
                .get(edge.line)
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            let block_start = *lines
                .block
                .get(edge.segment)
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            let block_end = *lines
                .block
                .get(edge.segment.saturating_add(1))
                .ok_or(CollapsedBorderGeometryError::InvalidEdge { edge })?;
            LogicalRect {
                inline_start: inline - half_width,
                block_start,
                inline_size: width,
                block_size: block_end - block_start,
            }
        },
    };
    Ok(rect)
}
