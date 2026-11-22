//! Toolpath generation.
//!
//! The generator turns a validated document into explicit machine moves. It is
//! deterministic: the same document always produces the same move list, in
//! document order, with no timestamps or randomised ordering.

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, Operation, OperationKind, Point, Stock, TabSettings, Tool},
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
            OperationKind::Engrave => engrave(operation, tool, document.stock.height, safe_z, &mut builder),
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
    let profile = Profile::new(
        operation.x - radius,
        operation.y - radius,
        operation.x + operation.width + radius,
        operation.y + operation.height + radius,
    );
    let corners = [
        Point::new(profile.x0, profile.y0, 0.0),
        Point::new(profile.x1, profile.y0, 0.0),
        Point::new(profile.x1, profile.y1, 0.0),
        Point::new(profile.x0, profile.y1, 0.0),
    ];
    // A ramp stays on the first bottom-edge run and never crosses a tab.
    let ramp_run = match operation.tabs {
        None => profile.width(),
        Some(tabs) => first_tab_start(&profile, tabs).min(profile.width()),
    };

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
        match operation.ramp {
            Some(ramp) => {
                let end = profile.point(ramp_run, safe_z);
                ramp_entry(builder, Point::new(corners[0].x, corners[0].y, safe_z), end, z, ramp, tool.plunge_feed, tool.feed);
            }
            None => {
                builder.feed_to(Point::new(corners[0].x, corners[0].y, z), MoveKind::Plunge, tool.plunge_feed);
            }
        }
        match operation.tabs {
            None => {
                for step in 1..=4 {
                    let corner = corners[step % 4];
                    builder.feed_to(Point::new(corner.x, corner.y, z), MoveKind::Cut, tool.feed);
                }
            }
            Some(tabs) => contour_with_tabs(&profile, tabs, z, safe_z, tool, builder),
        }
        builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
    }
}

/// One rectangular contour profile, already offset outward by the cutter radius.
struct Profile {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Profile {
    fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    fn perimeter(&self) -> f64 {
        2.0 * (self.width() + self.height())
    }

    /// Point at distance `s` from the lower-left corner, walking bottom, right,
    /// top and left in that order.
    fn point(&self, s: f64, z: f64) -> Point {
        let width = self.width();
        let height = self.height();
        if s >= self.perimeter() - TOLERANCE {
            return Point::new(self.x0, self.y0, z);
        }
        if s <= width {
            return Point::new(self.x0 + s, self.y0, z);
        }
        if s <= width + height {
            return Point::new(self.x1, self.y0 + (s - width), z);
        }
        if s <= 2.0 * width + height {
            return Point::new(self.x1 - (s - width - height), self.y1, z);
        }
        Point::new(self.x0, self.y1 - (s - 2.0 * width - height), z)
    }

    /// Corner distances strictly between `start` and `end` along the perimeter.
    fn corners_between(&self, start: f64, end: f64) -> Vec<f64> {
        let width = self.width();
        let height = self.height();
        [width, width + height, 2.0 * width + height]
            .into_iter()
            .filter(|corner| *corner > start + TOLERANCE && *corner < end - TOLERANCE)
            .collect()
    }
}

/// Perimeter distance where the first tab begins; never negative.
fn first_tab_start(profile: &Profile, tabs: TabSettings) -> f64 {
    let spacing = profile.perimeter() / f64::from(tabs.count);
    (0.5 * spacing - tabs.width / 2.0).max(0.0)
}

/// Cuts a contour while leaving `tabs` standing.
///
/// Each tab is centred in an equal share of the profile perimeter. Over a tab
/// interval the cutter lifts to `z + height` (capped at the clearance plane),
/// rapids along the perimeter to the far edge of the tab and plunges back to
/// `z`; the intervals between tabs are cut at the cutting feed. One level
/// therefore emits exactly `count` lifts and `count` tab-end plunges.
fn contour_with_tabs(profile: &Profile, tabs: TabSettings, z: f64, safe_z: f64, tool: &Tool, builder: &mut Builder) {
    let raised = (z + tabs.height).min(safe_z);
    let spacing = profile.perimeter() / f64::from(tabs.count);
    let spans = (0..tabs.count).map(|index| {
        let centre = (f64::from(index) + 0.5) * spacing;
        (centre - tabs.width / 2.0, centre + tabs.width / 2.0)
    });

    let mut cursor = 0.0;
    for (start, end) in spans {
        cut_along(profile, builder, cursor, start, z, tool.feed);
        builder.rapid_to(profile.point(start, raised));
        rapid_along(profile, builder, start, end, raised);
        builder.feed_to(profile.point(end, z), MoveKind::Plunge, tool.plunge_feed);
        cursor = end;
    }
    cut_along(profile, builder, cursor, profile.perimeter(), z, tool.feed);
}

fn cut_along(profile: &Profile, builder: &mut Builder, start: f64, end: f64, z: f64, feed: f64) {
    for corner in profile.corners_between(start, end) {
        builder.feed_to(profile.point(corner, z), MoveKind::Cut, feed);
    }
    builder.feed_to(profile.point(end, z), MoveKind::Cut, feed);
}

fn rapid_along(profile: &Profile, builder: &mut Builder, start: f64, end: f64, z: f64) {
    for corner in profile.corners_between(start, end) {
        builder.rapid_to(profile.point(corner, z));
    }
    builder.rapid_to(profile.point(end, z));
}

/// Engraves the operation polyline at every depth level.
///
/// The tool rapids to the first point at the clearance plane, plunges to the
/// level, cuts each segment in order at the cutting feed and finally rapids back
/// to the clearance plane above the last point. A closed engraving adds one
/// cutting move from the last point back to the first at every level. The
/// cutter follows the polyline itself, so no cutter-radius compensation is
/// applied.
fn engrave(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let first = operation.path[0];
    let second = operation.path[1];

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(first.x, first.y, safe_z));
        match operation.ramp {
            Some(ramp) => ramp_entry(
                builder,
                Point::new(first.x, first.y, safe_z),
                Point::new(second.x, second.y, safe_z),
                z,
                ramp,
                tool.plunge_feed,
                tool.feed,
            ),
            None => {
                builder.feed_to(Point::new(first.x, first.y, z), MoveKind::Plunge, tool.plunge_feed);
            }
        }
        for point in &operation.path[1..] {
            builder.feed_to(Point::new(point.x, point.y, z), MoveKind::Cut, tool.feed);
        }
        if operation.closed {
            builder.feed_to(Point::new(first.x, first.y, z), MoveKind::Cut, tool.feed);
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
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
        let mut entered = false;
        for y in &lines {
            let blocked = island_keep_out(operation, radius, *y, x0, x1);
            builder.skipped_segments += blocked.len();
            builder.skipped_length += blocked.iter().map(|(low, high)| high - low).sum::<f64>();
            let spans = cut_spans(x0, x1, &blocked);
            let ordered: Vec<(f64, f64)> =
                if forward { spans } else { spans.into_iter().rev().map(|(low, high)| (high, low)).collect() };
            for (start, end) in ordered {
                builder.rapid_to(Point::new(start, *y, safe_z));
                match operation.ramp {
                    Some(ramp) if !entered => {
                        ramp_entry(
                            builder,
                            Point::new(start, *y, safe_z),
                            Point::new(end, *y, safe_z),
                            z,
                            ramp,
                            tool.plunge_feed,
                            tool.feed,
                        );
                        entered = true;
                    }
                    _ => builder.feed_to(Point::new(start, *y, z), MoveKind::Plunge, tool.plunge_feed),
                }
                builder.feed_to(Point::new(end, *y, z), MoveKind::Cut, tool.feed);
                let at = builder.at;
                builder.rapid_to(Point::new(at.x, at.y, safe_z));
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
        match operation.ramp {
            Some(ramp) => ramp_entry(
                builder,
                Point::new(x0, y0, safe_z),
                Point::new(x1, y0, safe_z),
                z,
                ramp,
                tool.plunge_feed,
                tool.feed,
            ),
            None => {
                builder.feed_to(Point::new(x0, y0, z), MoveKind::Plunge, tool.plunge_feed);
            }
        }
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
        match operation.ramp {
            Some(ramp) => ramp_entry(
                builder,
                Point::new(operation.x + first, operation.y + first, safe_z),
                Point::new(operation.x + operation.width - first, operation.y + first, safe_z),
                z,
                ramp,
                tool.plunge_feed,
                tool.feed,
            ),
            None => {
                builder.feed_to(Point::new(operation.x + first, operation.y + first, z), MoveKind::Plunge, tool.plunge_feed);
            }
        }
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

    let chord = std::f64::consts::TAU / RING_SEGMENTS as f64;

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        builder.rapid_to(Point::new(centre.x + first, centre.y, safe_z));
        match operation.ramp {
            Some(ramp) => ramp_entry(
                builder,
                Point::new(centre.x + first, centre.y, safe_z),
                Point::new(centre.x + first * chord.cos(), centre.y + first * chord.sin(), safe_z),
                z,
                ramp,
                tool.plunge_feed,
                tool.feed,
            ),
            None => {
                builder.feed_to(Point::new(centre.x + first, centre.y, z), MoveKind::Plunge, tool.plunge_feed);
            }
        }
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

/// Emits the ramp entry of one Z level and ends exactly at `target_z`.
///
/// `start` is the clearance-plane point above the first cutting move of the
/// level and `end` is the far end of that move. The cutter travels exactly
/// `ramp` millimetres horizontally along that straight line at the cutting
/// feed, folding back at both ends when `ramp` is longer than the segment, and
/// its Z descends linearly from `start.z` to `target_z`. The last descending
/// move lands on `target_z`, and a final cutting move returns to `start` at
/// `target_z`, so the caller can run the normal level path unchanged and no
/// move goes below the level depth. A degenerate segment falls back to a
/// vertical plunge at the plunge feed.
fn ramp_entry(builder: &mut Builder, start: Point, end: Point, target_z: f64, ramp: f64, plunge_feed: f64, feed: f64) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let span = (dx * dx + dy * dy).sqrt();
    let drop = start.z - target_z;
    if span <= TOLERANCE || drop <= TOLERANCE || ramp <= TOLERANCE {
        builder.feed_to(Point::new(start.x, start.y, target_z), MoveKind::Plunge, plunge_feed);
        return;
    }
    let ux = dx / span;
    let uy = dy / span;
    let mut position = 0.0;
    let mut travelled = 0.0;
    let mut forward = true;
    while travelled < ramp - TOLERANCE {
        let available = if forward { span - position } else { position };
        if available <= TOLERANCE {
            forward = !forward;
            continue;
        }
        let step = (ramp - travelled).min(available);
        position = if forward { position + step } else { position - step };
        travelled += step;
        let z = if travelled >= ramp - TOLERANCE { target_z } else { start.z - drop * travelled / ramp };
        builder.feed_to(Point::new(start.x + ux * position, start.y + uy * position, z), MoveKind::Cut, feed);
        if position <= TOLERANCE || position >= span - TOLERANCE {
            forward = !forward;
        }
    }
    if position > TOLERANCE {
        builder.feed_to(Point::new(start.x, start.y, target_z), MoveKind::Cut, feed);
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
