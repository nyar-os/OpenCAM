//! Toolpath generation.
//!
//! The generator turns a validated document into explicit machine moves. It is
//! deterministic: the same document always produces the same move list, in
//! document order, with no timestamps or randomised ordering.

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, Operation, OperationKind, Point, Stock, Tool},
    validate,
};

/// Rapid traverse rate used for every non-cutting move, in millimetres per minute.
pub const RAPID_FEED: f64 = 8000.0;

/// Distance the tool keeps above the stock top when traversing.
pub const CLEARANCE: f64 = 5.0;

/// Geometric tolerance used when splitting raster lines around islands, in millimetres.
const TOLERANCE: f64 = 1e-9;

/// Number of linear segments used to approximate one circular ring.
const RING_SEGMENTS: usize = 72;

/// Kind of one machine move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveKind {
    /// Non-cutting rapid traverse.
    Rapid,
    /// Vertical entry into the material at the plunge feed.
    Plunge,
    /// Cutting move at the tool feed.
    Cut,
}

/// One machine move.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    /// Move kind.
    pub kind: MoveKind,
    /// Start point.
    pub from: Point,
    /// End point.
    pub to: Point,
    /// Euclidean length.
    pub length: f64,
    /// Feed rate in millimetres per minute; [`RAPID_FEED`] for rapid moves.
    pub feed: f64,
}

/// Generated path of one operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPath {
    /// Operation identifier.
    pub operation: String,
    /// Operation strategy.
    pub kind: OperationKind,
    /// Pocket strategy that generated this path; omitted for raster pockets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Pocket outline that generated this path; omitted for rectangular pockets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    /// Tool identifier.
    pub tool: String,
    /// Number of generated moves.
    pub moves: usize,
    /// Total cutting length including plunges.
    pub cut_length: f64,
    /// Total rapid length.
    pub rapid_length: f64,
    /// Estimated cutting time in seconds.
    pub cutting_seconds: f64,
    /// Estimated rapid time in seconds.
    pub rapid_seconds: f64,
    /// Number of islands this operation clears around.
    pub island_count: usize,
    /// Raster length skipped because it falls inside an island keep-out zone.
    pub skipped_length: f64,
    /// Number of raster passes skipped because of island keep-out zones.
    pub skipped_segments: usize,
    /// Closed loops emitted by a spiral or circular pocket; omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_count: Option<usize>,
    /// Explicit moves in execution order.
    pub segments: Vec<Move>,
}

/// Complete toolpath report of a document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolpathReport {
    /// Format identifier, always `open-cam-toolpaths`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Z height used for all traverses.
    pub safe_z: f64,
    /// Rapid feed used for every rapid move.
    pub rapid_feed: f64,
    /// Total cutting length over every operation.
    pub cut_length: f64,
    /// Total rapid length over every operation.
    pub rapid_length: f64,
    /// Estimated cycle time in seconds.
    pub estimated_seconds: f64,
    /// One entry per operation, in document order.
    pub operations: Vec<OperationPath>,
}

/// Generates the toolpath report of a document.
///
/// The document is validated first, so an unmachinable file never produces a
/// partial path. The first validation error is returned unchanged.
pub fn generate(document: &CamDocument) -> Result<ToolpathReport, CamError> {
    let report = validate::validate(document);
    if let Some(error) = validate::first_error(&report) {
        return Err(error);
    }

    let safe_z = document.stock.height + CLEARANCE;
    let mut operations = Vec::with_capacity(document.operations.len());
    let mut cut_length = 0.0;
    let mut rapid_length = 0.0;
    let mut estimated_seconds = 0.0;

    for operation in &document.operations {
        let tool = document.tools.iter().find(|tool| tool.id == operation.tool).expect("validation guarantees the tool exists");
        let mut builder = Builder::new(Point::new(0.0, 0.0, safe_z));
        match operation.kind {
            OperationKind::Face => face(operation, tool, &document.stock, safe_z, &mut builder),
            OperationKind::Contour => contour(operation, tool, document.stock.height, safe_z, &mut builder),
            OperationKind::Pocket if operation.shape.as_deref() == Some("circle") => {
                pocket_circle(operation, tool, document.stock.height, safe_z, &mut builder);
            }
            OperationKind::Pocket if operation.strategy.as_deref() == Some("spiral") => {
                pocket_spiral(operation, tool, document.stock.height, safe_z, &mut builder);
            }
            OperationKind::Pocket => pocket(operation, tool, document.stock.height, safe_z, &mut builder),
            OperationKind::Drill => drill(operation, tool, document.stock.height, safe_z, &mut builder),
        }
        let path = builder.finish(operation);
        cut_length += path.cut_length;
        rapid_length += path.rapid_length;
        estimated_seconds += path.cutting_seconds + path.rapid_seconds;
        operations.push(path);
    }

    Ok(ToolpathReport {
        format: "open-cam-toolpaths".to_string(),
        version: 1,
        units: document.units.clone(),
        safe_z,
        rapid_feed: RAPID_FEED,
        cut_length,
        rapid_length,
        estimated_seconds,
        operations,
    })
}

fn face(operation: &Operation, tool: &Tool, stock: &Stock, safe_z: f64, builder: &mut Builder) {
    let stepover = operation.stepover.unwrap_or(0.7 * tool.diameter);
    let levels = level_depths(operation.depth, operation.stepdown);
    let lines = line_positions(stock.depth, stepover);

    for depth in levels {
        let z = stock.height - depth;
        builder.rapid_to(Point::new(0.0, lines[0], safe_z));
        builder.feed_to(Point::new(0.0, lines[0], z), MoveKind::Plunge, tool.plunge_feed);
        let mut forward = true;
        for (index, y) in lines.iter().enumerate() {
            if index > 0 {
                let x = builder.at.x;
                builder.feed_to(Point::new(x, *y, z), MoveKind::Cut, tool.feed);
            }
            let x = if forward { stock.width } else { 0.0 };
            builder.feed_to(Point::new(x, *y, z), MoveKind::Cut, tool.feed);
            forward = !forward;
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

fn contour(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let x0 = operation.x - radius;
    let y0 = operation.y - radius;
    let x1 = operation.x + operation.width + radius;
    let y1 = operation.y + operation.height + radius;
    let corners = [Point::new(x0, y0, 0.0), Point::new(x1, y0, 0.0), Point::new(x1, y1, 0.0), Point::new(x0, y1, 0.0)];

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
        builder.feed_to(Point::new(corners[0].x, corners[0].y, z), MoveKind::Plunge, tool.plunge_feed);
        for step in 1..=4 {
            let corner = corners[step % 4];
            builder.feed_to(Point::new(corner.x, corner.y, z), MoveKind::Cut, tool.feed);
        }
        builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
    }
}

fn pocket(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    if operation.islands.is_empty() {
        pocket_plain(operation, tool, stock_height, safe_z, builder);
        return;
    }

    let radius = tool.radius();
    let stepover = operation.stepover.unwrap_or(0.6 * tool.diameter);
    let x0 = operation.x + radius;
    let x1 = operation.x + operation.width - radius;
    let y0 = operation.y + radius;
    let lines: Vec<f64> =
        line_positions(operation.height - 2.0 * radius, stepover).into_iter().map(|offset| y0 + offset).collect();
    builder.island_count = operation.islands.len();

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        let mut forward = true;
        for y in &lines {
            let blocked = island_keep_out(operation, radius, *y, x0, x1);
            builder.skipped_segments += blocked.len();
            builder.skipped_length += blocked.iter().map(|(low, high)| high - low).sum::<f64>();
            let spans = cut_spans(x0, x1, &blocked);
            if !forward {
                for (start, end) in spans.into_iter().rev().map(|(low, high)| (high, low)) {
                    builder.rapid_to(Point::new(start, *y, safe_z));
                    builder.feed_to(Point::new(start, *y, z), MoveKind::Plunge, tool.plunge_feed);
                    builder.feed_to(Point::new(end, *y, z), MoveKind::Cut, tool.feed);
                    let at = builder.at;
                    builder.rapid_to(Point::new(at.x, at.y, safe_z));
                }
            }
            else {
                for (start, end) in spans {
                    builder.rapid_to(Point::new(start, *y, safe_z));
                    builder.feed_to(Point::new(start, *y, z), MoveKind::Plunge, tool.plunge_feed);
                    builder.feed_to(Point::new(end, *y, z), MoveKind::Cut, tool.feed);
                    let at = builder.at;
                    builder.rapid_to(Point::new(at.x, at.y, safe_z));
                }
            }
            forward = !forward;
        }
    }
}

fn pocket_plain(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let stepover = operation.stepover.unwrap_or(0.6 * tool.diameter);
    let x0 = operation.x + radius;
    let x1 = operation.x + operation.width - radius;
    let y0 = operation.y + radius;
    let lines: Vec<f64> =
        line_positions(operation.height - 2.0 * radius, stepover).into_iter().map(|offset| y0 + offset).collect();

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(x0, y0, safe_z));
        builder.feed_to(Point::new(x0, y0, z), MoveKind::Plunge, tool.plunge_feed);
        let mut forward = true;
        for (index, y) in lines.iter().enumerate() {
            if index > 0 {
                let x = builder.at.x;
                builder.feed_to(Point::new(x, *y, z), MoveKind::Cut, tool.feed);
            }
            let x = if forward { x1 } else { x0 };
            builder.feed_to(Point::new(x, *y, z), MoveKind::Cut, tool.feed);
            forward = !forward;
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

/// Spiral pocket: concentric closed rectangles, each inset by `stepover`.
///
/// The first ring is the pocket shrunk by the cutter radius; every further ring
/// is inset one more `stepover` on all four sides. Rings are emitted until the
/// uncut core left inside a ring is no wider or taller than the tool diameter;
/// at that point opposite passes overlap and the core is cleared, so no further
/// closed ring is needed. Rings advance at the cutting feed without retracting.
fn pocket_spiral(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let diameter = tool.diameter;
    let stepover = operation.stepover.unwrap_or(0.6 * tool.diameter);

    let mut insets = vec![radius];
    let mut inset = radius;
    loop {
        let width = operation.width - 2.0 * inset;
        let height = operation.height - 2.0 * inset;
        if width - diameter <= diameter + TOLERANCE || height - diameter <= diameter + TOLERANCE {
            break;
        }
        inset += stepover;
        insets.push(inset);
    }

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        let first = insets[0];
        builder.rapid_to(Point::new(operation.x + first, operation.y + first, safe_z));
        builder.feed_to(Point::new(operation.x + first, operation.y + first, z), MoveKind::Plunge, tool.plunge_feed);
        for (index, inset) in insets.iter().enumerate() {
            let x0 = operation.x + inset;
            let y0 = operation.y + inset;
            let x1 = operation.x + operation.width - inset;
            let y1 = operation.y + operation.height - inset;
            if index > 0 {
                builder.feed_to(Point::new(x0, y0, z), MoveKind::Cut, tool.feed);
            }
            builder.feed_to(Point::new(x1, y0, z), MoveKind::Cut, tool.feed);
            builder.feed_to(Point::new(x1, y1, z), MoveKind::Cut, tool.feed);
            builder.feed_to(Point::new(x0, y1, z), MoveKind::Cut, tool.feed);
            builder.feed_to(Point::new(x0, y0, z), MoveKind::Cut, tool.feed);
            builder.loop_count += 1;
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

/// Circular pocket: concentric rings, each inset by `stepover`.
///
/// With pocket radius `R = width / 2` and cutter radius `r`, the first ring has
/// radius `R − r`; every next ring shrinks by `stepover` until the radius is no
/// longer positive. Rings are approximated by [`RING_SEGMENTS`] cutting segments
/// and close at the cutting feed, and the cutter advances to the next ring at
/// the cutting feed without lifting. A final cutting move reaches the centre so
/// the core is always cleared. Every ring point is at most `R − r` from the
/// centre, so no cutting point leaves the pocket or comes closer than `r` to
/// its wall.
fn pocket_circle(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let stepover = operation.stepover.unwrap_or(0.6 * tool.diameter);
    let pocket_radius = operation.width / 2.0;
    let centre = Point::new(operation.x + pocket_radius, operation.y + pocket_radius, 0.0);

    let mut rings = Vec::new();
    let mut ring = pocket_radius - radius;
    while ring > TOLERANCE {
        rings.push(ring);
        ring -= stepover;
    }
    let first = rings.first().copied().unwrap_or(0.0);

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(centre.x + first, centre.y, safe_z));
        builder.feed_to(Point::new(centre.x + first, centre.y, z), MoveKind::Plunge, tool.plunge_feed);
        for (index, ring) in rings.iter().enumerate() {
            let start = Point::new(centre.x + ring, centre.y, z);
            if index > 0 {
                builder.feed_to(start, MoveKind::Cut, tool.feed);
            }
            for step in 1..=RING_SEGMENTS {
                let angle = std::f64::consts::TAU * step as f64 / RING_SEGMENTS as f64;
                let point = Point::new(centre.x + ring * angle.cos(), centre.y + ring * angle.sin(), z);
                builder.feed_to(point, MoveKind::Cut, tool.feed);
            }
            builder.feed_to(start, MoveKind::Cut, tool.feed);
            builder.loop_count += 1;
        }
        builder.feed_to(Point::new(centre.x, centre.y, z), MoveKind::Cut, tool.feed);
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

/// X intervals of one raster line that stay outside every island keep-out zone.
fn island_keep_out(operation: &Operation, radius: f64, y: f64, x0: f64, x1: f64) -> Vec<(f64, f64)> {
    let mut blocked = Vec::new();
    for island in &operation.islands {
        if y < island.y - radius - TOLERANCE || y > island.y + island.height + radius + TOLERANCE {
            continue;
        }
        let low = (island.x - radius).max(x0);
        let high = (island.x + island.width + radius).min(x1);
        if high - low > TOLERANCE {
            blocked.push((low, high));
        }
    }
    blocked.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(blocked.len());
    for (low, high) in blocked {
        if let Some(last) = merged.last_mut() {
            if low <= last.1 + TOLERANCE {
                last.1 = last.1.max(high);
                continue;
            }
        }
        merged.push((low, high));
    }
    merged
}

/// Complement of the blocked intervals inside `x0 … x1`, in ascending order.
fn cut_spans(x0: f64, x1: f64, blocked: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut spans = Vec::new();
    let mut cursor = x0;
    for (low, high) in blocked {
        if *low > cursor + TOLERANCE {
            spans.push((cursor, *low));
        }
        cursor = cursor.max(*high);
    }
    if x1 > cursor + TOLERANCE {
        spans.push((cursor, x1));
    }
    spans
}

fn drill(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let z = stock_height - operation.depth;
    for hole in &operation.holes {
        builder.rapid_to(Point::new(hole.x, hole.y, safe_z));
        builder.feed_to(Point::new(hole.x, hole.y, z), MoveKind::Plunge, tool.plunge_feed);
        builder.rapid_to(Point::new(hole.x, hole.y, safe_z));
    }
}

struct Builder {
    at: Point,
    segments: Vec<Move>,
    cut_length: f64,
    rapid_length: f64,
    cutting_seconds: f64,
    rapid_seconds: f64,
    island_count: usize,
    skipped_length: f64,
    skipped_segments: usize,
    loop_count: usize,
}

impl Builder {
    fn new(at: Point) -> Self {
        Self {
            at,
            segments: Vec::new(),
            cut_length: 0.0,
            rapid_length: 0.0,
            cutting_seconds: 0.0,
            rapid_seconds: 0.0,
            island_count: 0,
            skipped_length: 0.0,
            skipped_segments: 0,
            loop_count: 0,
        }
    }

    fn rapid_to(&mut self, to: Point) {
        self.add(MoveKind::Rapid, to, RAPID_FEED);
    }

    fn feed_to(&mut self, to: Point, kind: MoveKind, feed: f64) {
        self.add(kind, to, feed);
    }

    fn add(&mut self, kind: MoveKind, to: Point, feed: f64) {
        let from = self.at;
        let length = from.distance_to(to);
        self.at = to;
        if length <= 1e-12 {
            return;
        }
        match kind {
            MoveKind::Rapid => {
                self.rapid_length += length;
                self.rapid_seconds += length / RAPID_FEED * 60.0;
            }
            MoveKind::Plunge | MoveKind::Cut => {
                self.cut_length += length;
                self.cutting_seconds += length / feed * 60.0;
            }
        }
        self.segments.push(Move { kind, from, to, length, feed });
    }

    fn finish(mut self, operation: &Operation) -> OperationPath {
        let spiral = operation.kind == OperationKind::Pocket && operation.strategy.as_deref() == Some("spiral");
        let circle = operation.kind == OperationKind::Pocket && operation.shape.as_deref() == Some("circle");
        OperationPath {
            operation: operation.id.clone(),
            kind: operation.kind,
            strategy: spiral.then(|| "spiral".to_string()),
            shape: circle.then(|| "circle".to_string()),
            tool: operation.tool.clone(),
            moves: self.segments.len(),
            cut_length: self.cut_length,
            rapid_length: self.rapid_length,
            cutting_seconds: self.cutting_seconds,
            rapid_seconds: self.rapid_seconds,
            island_count: self.island_count,
            skipped_length: self.skipped_length,
            skipped_segments: self.skipped_segments,
            loop_count: (spiral || circle).then_some(self.loop_count),
            segments: std::mem::take(&mut self.segments),
        }
    }
}

fn level_depths(depth: f64, stepdown: f64) -> Vec<f64> {
    let mut levels = Vec::new();
    let mut current = stepdown;
    while current < depth - 1e-9 {
        levels.push(current);
        current += stepdown;
    }
    levels.push(depth);
    levels
}

fn line_positions(span: f64, step: f64) -> Vec<f64> {
    let mut positions = vec![0.0];
    let mut current = step;
    while current < span - 1e-9 {
        positions.push(current);
        current += step;
    }
    if (positions[positions.len() - 1] - span).abs() > 1e-9 {
        positions.push(span);
    }
    positions
}
