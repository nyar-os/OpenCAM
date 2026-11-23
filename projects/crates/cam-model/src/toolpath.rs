//! Toolpath generation.
//!
//! The generator turns a validated document into explicit machine moves. It is
//! deterministic: the same document always produces the same move list, in
//! document order, with no timestamps or randomised ordering.

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, LeadSettings, Operation, OperationKind, Point, Stock, TabSettings, Tool},
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
    /// Half-circle arc interpolation at the tool feed.
    Arc,
}

/// Arc parameters of one [`MoveKind::Arc`] move.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcParams {
    /// Centre of the arc circle.
    pub centre: Point,
    /// `true` renders `G2`, `false` renders `G3`.
    pub clockwise: bool,
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
    /// Path length: Euclidean for linear moves, arc length for arcs.
    pub length: f64,
    /// Feed rate in millimetres per minute; [`RAPID_FEED`] for rapid moves.
    pub feed: f64,
    /// Arc centre and direction; omitted for every non-arc move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<ArcParams>,
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
    /// Circular outline that generated this path; omitted for rectangles.
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
            OperationKind::Contour if operation.shape.as_deref() == Some("circle") => {
                contour_circle(operation, tool, document.stock.height, safe_z, &mut builder);
            }
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
        match &operation.lead {
            Some(lead) => {
                // The lead-in joins corner 0 tangentially along the bottom edge.
                let (outer, centre) = lead_arc(corners[0], 1.0, 0.0, lead, z);
                builder.rapid_to(Point::new(outer.x, outer.y, safe_z));
                builder.feed_to(Point::new(outer.x, outer.y, z), MoveKind::Plunge, tool.plunge_feed);
                builder.arc_to(Point::new(corners[0].x, corners[0].y, z), centre, lead.is_clockwise(), tool.feed);
            }
            None => {
                builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
                match operation.ramp {
                    Some(ramp) => {
                        let end = profile.point(ramp_run, safe_z);
                        ramp_entry(
                            builder,
                            Point::new(corners[0].x, corners[0].y, safe_z),
                            end,
                            z,
                            ramp,
                            tool.plunge_feed,
                            tool.feed,
                        );
                    }
                    None => {
                        builder.feed_to(Point::new(corners[0].x, corners[0].y, z), MoveKind::Plunge, tool.plunge_feed);
                    }
                }
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
        match &operation.lead {
            Some(lead) => {
                // The lead-out leaves corner 0 tangentially along the closing move.
                let (outer, centre) = lead_arc(corners[0], 0.0, -1.0, lead, z);
                builder.arc_to(Point::new(outer.x, outer.y, z), centre, lead.is_clockwise(), tool.feed);
                builder.rapid_to(Point::new(outer.x, outer.y, safe_z));
            }
            None => {
                builder.rapid_to(Point::new(corners[0].x, corners[0].y, safe_z));
            }
        }
    }
}

/// The outer end and centre of one tangential half-circle lead arc.
///
/// The arc has radius `lead.radius`, touches `tangent_point` exactly along the
/// unit travel direction `(dx, dy)` and spans a half circle: its centre lies
/// `lead.radius` to the right of the travel direction for a clockwise arc and
/// to the left for a counter-clockwise one, and the outer end is the point of
/// the circle diametrically opposite the tangent point. A lead-in runs from the
/// outer end into the profile, a lead-out from the profile to the outer end.
fn lead_arc(tangent_point: Point, dx: f64, dy: f64, lead: &LeadSettings, z: f64) -> (Point, Point) {
    let sign = if lead.is_clockwise() { 1.0 } else { -1.0 };
    let centre = Point::new(tangent_point.x + sign * dy * lead.radius, tangent_point.y - sign * dx * lead.radius, z);
    let outer = Point::new(2.0 * centre.x - tangent_point.x, 2.0 * centre.y - tangent_point.y, z);
    (outer, centre)
}

/// Circular contour: one closed ring offset outward by the cutter radius.
///
/// `width` is the diameter and `height` equals it, so the part is centred at
/// `(x + width / 2, y + height / 2)` and the tool centre follows the circle of
/// radius `width / 2 + r`. The ring starts at angle 0 (+X) and runs
/// counter-clockwise, closing at the cutting feed: as [`RING_SEGMENTS`] linear
/// cutting segments by default, or as counter-clockwise `G3` arcs of at most
/// half a circle when the operation sets `arcs: "arc"`. Every depth level
/// rapids to the start at the clearance plane, enters with the vertical plunge
/// or the `ramp` entry and retracts to the clearance plane afterwards, exactly
/// like the rectangular contour.
///
/// With `tabs` the ring is split at every tab entry and exit angle so no
/// cutting move crosses a tab, and the cutter lifts over each tab as described
/// in [`circle_with_tabs`].
fn contour_circle(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let path = CirclePath::new(operation, tool, safe_z);
    let arcs = uses_arcs(operation);
    let chord = std::f64::consts::TAU / RING_SEGMENTS as f64;
    // A ramp stays on the first cutting run and never crosses a tab.
    let ramp_end = match operation.tabs {
        None => chord,
        Some(tabs) => circle_first_tab_entry(tabs, path.radius),
    };

    for depth in level_depths(operation.depth, operation.stepdown) {
        let z = stock_height - depth;
        match &operation.lead {
            Some(lead) => {
                // The ring starts at angle 0 running counter-clockwise, so the
                // tangent at the start point is +Y for both the lead-in and the
                // lead-out; the two half circles share the circle but cover
                // opposite halves, so no arc is retraced.
                let (outer, centre) = lead_arc(path.start, 0.0, 1.0, lead, z);
                builder.rapid_to(Point::new(outer.x, outer.y, safe_z));
                builder.feed_to(Point::new(outer.x, outer.y, z), MoveKind::Plunge, tool.plunge_feed);
                builder.arc_to(Point::new(path.start.x, path.start.y, z), centre, lead.is_clockwise(), tool.feed);
            }
            None => {
                builder.rapid_to(path.start);
                match operation.ramp {
                    Some(ramp) => {
                        ramp_entry(builder, path.start, path.point(ramp_end, safe_z), z, ramp, tool.plunge_feed, tool.feed)
                    }
                    None => {
                        builder.feed_to(Point::new(path.start.x, path.start.y, z), MoveKind::Plunge, tool.plunge_feed);
                    }
                }
            }
        }
        match operation.tabs {
            None => {
                Ring { centre: path.centre, radius: path.radius }.cut(builder, 0.0, std::f64::consts::TAU, z, tool.feed, arcs);
                builder.feed_to(Point::new(path.start.x, path.start.y, z), MoveKind::Cut, tool.feed);
            }
            Some(tabs) => circle_with_tabs(&path, tabs, z, safe_z, tool, builder, arcs),
        }
        match &operation.lead {
            Some(lead) => {
                let (outer, centre) = lead_arc(path.start, 0.0, 1.0, lead, z);
                builder.arc_to(Point::new(outer.x, outer.y, z), centre, lead.is_clockwise(), tool.feed);
                builder.rapid_to(Point::new(outer.x, outer.y, safe_z));
            }
            None => {
                builder.rapid_to(path.start);
            }
        }
    }
}

/// Tool-centre path of one circular contour.
struct CirclePath {
    centre: Point,
    radius: f64,
    start: Point,
}

impl CirclePath {
    fn new(operation: &Operation, tool: &Tool, safe_z: f64) -> Self {
        let radius = operation.width / 2.0 + tool.radius();
        let centre = Point::new(operation.x + operation.width / 2.0, operation.y + operation.height / 2.0, 0.0);
        let start = Point::new(centre.x + radius, centre.y, safe_z);
        Self { centre, radius, start }
    }

    /// One point of the tool-centre circle at `angle` radians.
    fn point(&self, angle: f64, z: f64) -> Point {
        Point::new(self.centre.x + self.radius * angle.cos(), self.centre.y + self.radius * angle.sin(), z)
    }
}

/// Angle where the first tab begins, measured counter-clockwise from +X.
///
/// Tabs are centred at `2·pi·(k + 0.5) / count`, so the first one starts half a
/// share minus half its angular width and the path never starts inside a tab.
fn circle_first_tab_entry(tabs: TabSettings, path_radius: f64) -> f64 {
    let share = std::f64::consts::TAU / f64::from(tabs.count);
    (0.5 * share - tabs.width / (2.0 * path_radius)).max(0.0)
}

/// Angles of the [`RING_SEGMENTS`] approximation strictly between `start` and
/// `end`, followed by `end` itself.
///
/// Splitting the baseline ring at the tab boundaries keeps the linear
/// approximation while making every emitted interval either fully cutting or
/// fully inside a tab.
fn ring_angles_between(start: f64, end: f64) -> Vec<f64> {
    let chord = std::f64::consts::TAU / RING_SEGMENTS as f64;
    let mut angles: Vec<f64> = (1..RING_SEGMENTS)
        .map(|step| chord * step as f64)
        .filter(|angle| *angle > start + TOLERANCE && *angle < end - TOLERANCE)
        .collect();
    angles.push(end);
    angles
}

/// Cuts a circular contour while leaving `tabs` standing.
///
/// Each tab is centred in an equal angular share of the tool-centre circle at
/// angle `2·pi·(k + 0.5) / count` and covers an arc length of `width` along it,
/// so its half angle is `width / (2·path_radius)`. The ring is split at every
/// tab entry and exit angle. Over a tab interval the cutter lifts to
/// `min(z + height, safe_z)`, rapids along the circle to the tab exit and
/// plunges back to `z`; the intervals between tabs are cut at the cutting feed.
/// One level therefore emits exactly `count` lifts and `count` tab-end plunges,
/// every point stays exactly `path_radius` from the centre and the ring closes
/// at the start point. With `arcs` the cutting intervals become `G3` arcs while
/// the rapid crossings over a tab stay straight moves.
fn circle_with_tabs(path: &CirclePath, tabs: TabSettings, z: f64, safe_z: f64, tool: &Tool, builder: &mut Builder, arcs: bool) {
    let raised = (z + tabs.height).min(safe_z);
    let share = std::f64::consts::TAU / f64::from(tabs.count);
    let half = tabs.width / (2.0 * path.radius);

    let mut cursor = 0.0;
    for index in 0..tabs.count {
        let tab_centre = (f64::from(index) + 0.5) * share;
        let entry = tab_centre - half;
        let exit = tab_centre + half;
        Ring { centre: path.centre, radius: path.radius }.cut(builder, cursor, entry, z, tool.feed, arcs);
        builder.rapid_to(path.point(entry, raised));
        circle_rapid_arc(path, builder, entry, exit, raised);
        builder.feed_to(path.point(exit, z), MoveKind::Plunge, tool.plunge_feed);
        cursor = exit;
    }
    Ring { centre: path.centre, radius: path.radius }.cut(builder, cursor, std::f64::consts::TAU, z, tool.feed, arcs);
    builder.feed_to(Point::new(path.start.x, path.start.y, z), MoveKind::Cut, tool.feed);
}

/// Tool-centre ring of a circular contour or pocket.
struct Ring {
    centre: Point,
    radius: f64,
}

impl Ring {
    /// Cuts one angular interval of the ring at height `z`.
    ///
    /// Without `arcs` the interval is split at the [`RING_SEGMENTS`] grid and cut
    /// as straight chords. With `arcs` it becomes counter-clockwise `G3` arcs of
    /// at most half a circle each, so no arc is ambiguous for a controller that
    /// picks the minor arc from `I`/`J` and the end point.
    fn cut(&self, builder: &mut Builder, start: f64, end: f64, z: f64, feed: f64, arcs: bool) {
        if !arcs {
            for angle in ring_angles_between(start, end) {
                builder.feed_to(ring_point(self.centre, self.radius, angle, z), MoveKind::Cut, feed);
            }
            return;
        }
        let sweep = end - start;
        if sweep <= TOLERANCE {
            return;
        }
        let pieces = ((sweep / std::f64::consts::PI) - TOLERANCE).ceil().max(1.0) as usize;
        let step = sweep / pieces as f64;
        for index in 1..=pieces {
            let angle = start + step * index as f64;
            builder.arc_sweep_to(ring_point(self.centre, self.radius, angle, z), self.centre, false, step, feed);
        }
    }
}

/// One point of a ring around `centre` at `angle` radians and height `z`.
///
/// An angle that closes on a full turn is normalised to 0, so a closed ring ends
/// exactly on its start point with no rounding residue.
fn ring_point(centre: Point, radius: f64, angle: f64, z: f64) -> Point {
    let angle = if (std::f64::consts::TAU - angle).abs() < TOLERANCE { 0.0 } else { angle };
    Point::new(centre.x + radius * angle.cos(), centre.y + radius * angle.sin(), z)
}

/// True when the operation asks for `G3` rings instead of chord approximation.
fn uses_arcs(operation: &Operation) -> bool {
    operation.arcs.as_deref() == Some("arc")
}

fn circle_rapid_arc(path: &CirclePath, builder: &mut Builder, start: f64, end: f64, z: f64) {
    for angle in ring_angles_between(start, end) {
        builder.rapid_to(path.point(angle, z));
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
        if operation.finish == Some(true) {
            finish_rectangle(operation, tool, z, builder);
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
        if operation.finish == Some(true) {
            finish_rectangle(operation, tool, z, builder);
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

/// Circular pocket: concentric rings, each inset by `stepover`.
///
/// With pocket radius `R = width / 2` and cutter radius `r`, the first ring has
/// radius `R − r`; every next ring shrinks by `stepover` until the radius is no
/// longer positive. Rings are cut as [`RING_SEGMENTS`] chords by default or as
/// counter-clockwise `G3` arcs of at most half a circle with `arcs: "arc"`, close
/// at the cutting feed, and the cutter advances to the next ring at the cutting
/// feed without lifting. A final cutting move reaches the centre so the core is
/// always cleared. Every ring point is at most `R − r` from the centre, so no
/// cutting point leaves the pocket or comes closer than `r` to its wall.
fn pocket_circle(operation: &Operation, tool: &Tool, stock_height: f64, safe_z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let stepover = operation.stepover.unwrap_or(0.6 * tool.diameter);
    let pocket_radius = operation.width / 2.0;
    let centre = Point::new(operation.x + pocket_radius, operation.y + pocket_radius, 0.0);
    let arcs = uses_arcs(operation);

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
            Ring { centre, radius: *ring }.cut(builder, 0.0, std::f64::consts::TAU, z, tool.feed, arcs);
            builder.feed_to(start, MoveKind::Cut, tool.feed);
            builder.loop_count += 1;
        }
        builder.feed_to(Point::new(centre.x, centre.y, z), MoveKind::Cut, tool.feed);
        if operation.finish == Some(true) {
            finish_circle(&centre, pocket_radius, tool, z, builder, arcs);
        }
        let at = builder.at;
        builder.rapid_to(Point::new(at.x, at.y, safe_z));
    }
}

/// One closed finish loop along the final rectangular pocket wall.
///
/// The loop uses the nominal cutter-radius offset, so every corner is exactly
/// `r` from the wall. It runs at the level depth and the cutting feed and is
/// reached with one cutting move across the already cleared floor. The loop is
/// emitted after the clearing moves of one level and adds no Z move.
fn finish_rectangle(operation: &Operation, tool: &Tool, z: f64, builder: &mut Builder) {
    let radius = tool.radius();
    let x0 = operation.x + radius;
    let y0 = operation.y + radius;
    let x1 = operation.x + operation.width - radius;
    let y1 = operation.y + operation.height - radius;
    builder.feed_to(Point::new(x0, y0, z), MoveKind::Cut, tool.feed);
    builder.feed_to(Point::new(x1, y0, z), MoveKind::Cut, tool.feed);
    builder.feed_to(Point::new(x1, y1, z), MoveKind::Cut, tool.feed);
    builder.feed_to(Point::new(x0, y1, z), MoveKind::Cut, tool.feed);
    builder.feed_to(Point::new(x0, y0, z), MoveKind::Cut, tool.feed);
}

/// One closed finish ring along the final circular pocket wall.
///
/// The ring has radius `R − r` around `centre`, uses the same interpolation as
/// the clearing rings (chords by default, `G3` arcs with `arcs`) and is reached
/// with one cutting move from the centre. It runs at the level depth and the
/// cutting feed and adds no Z move.
fn finish_circle(centre: &Point, radius: f64, tool: &Tool, z: f64, builder: &mut Builder, arcs: bool) {
    let ring = radius - tool.radius();
    let start = Point::new(centre.x + ring, centre.y, z);
    builder.feed_to(start, MoveKind::Cut, tool.feed);
    Ring { centre: *centre, radius: ring }.cut(builder, 0.0, std::f64::consts::TAU, z, tool.feed, arcs);
    builder.feed_to(start, MoveKind::Cut, tool.feed);
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

    /// Emits one half-circle cutting arc from the current point to `to` around
    /// `centre`, interpolating at the cutting feed.
    fn arc_to(&mut self, to: Point, centre: Point, clockwise: bool, feed: f64) {
        self.arc_sweep_to(to, centre, clockwise, std::f64::consts::PI, feed);
    }

    /// Emits one circular cutting arc of `sweep` radians from the current point
    /// to `to` around `centre`, interpolating at the cutting feed.
    fn arc_sweep_to(&mut self, to: Point, centre: Point, clockwise: bool, sweep: f64, feed: f64) {
        let from = self.at;
        let radius = ((from.x - centre.x).powi(2) + (from.y - centre.y).powi(2)).sqrt();
        let length = radius * sweep;
        self.at = to;
        if length <= 1e-12 {
            return;
        }
        self.cut_length += length;
        self.cutting_seconds += length / feed * 60.0;
        self.segments.push(Move { kind: MoveKind::Arc, from, to, length, feed, arc: Some(ArcParams { centre, clockwise }) });
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
            MoveKind::Plunge | MoveKind::Cut | MoveKind::Arc => {
                self.cut_length += length;
                self.cutting_seconds += length / feed * 60.0;
            }
        }
        self.segments.push(Move { kind, from, to, length, feed, arc: None });
    }

    fn finish(mut self, operation: &Operation) -> OperationPath {
        let spiral = operation.kind == OperationKind::Pocket && operation.strategy.as_deref() == Some("spiral");
        let circular = operation.shape.as_deref() == Some("circle")
            && matches!(operation.kind, OperationKind::Pocket | OperationKind::Contour);
        let ringed = operation.kind == OperationKind::Pocket && circular;
        OperationPath {
            operation: operation.id.clone(),
            kind: operation.kind,
            strategy: spiral.then(|| "spiral".to_string()),
            shape: circular.then(|| "circle".to_string()),
            tool: operation.tool.clone(),
            moves: self.segments.len(),
            cut_length: self.cut_length,
            rapid_length: self.rapid_length,
            cutting_seconds: self.cutting_seconds,
            rapid_seconds: self.rapid_seconds,
            island_count: self.island_count,
            skipped_length: self.skipped_length,
            skipped_segments: self.skipped_segments,
            loop_count: (spiral || ringed).then_some(self.loop_count),
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
