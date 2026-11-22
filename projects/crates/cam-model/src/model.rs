//! Document model of a 2.5-axis milling job.
//!
//! A document is one `.ocam` JSON file: a rectangular stock, a tool table and a
//! list of machining operations. The document stores intent only; toolpaths and
//! G-code are derived on demand by [`crate::toolpath`] and [`crate::gcode`], so
//! a file never contains stale machine output.

use serde::{Deserialize, Serialize};

/// Document format identifier written to every file.
pub const FORMAT: &str = "open-cam";

/// Document schema version written to every file.
pub const VERSION: u32 = 1;

/// The only unit system this build accepts. Dimensions are millimetres and
/// feed rates are millimetres per minute.
pub const UNITS: &str = "mm";

/// One machining job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CamDocument {
    /// Format identifier; always [`FORMAT`] for a document this crate wrote.
    pub format: String,
    /// Schema version; always [`VERSION`] for a document this crate wrote.
    pub version: u32,
    /// Monotonically increasing edit counter.
    pub revision: u32,
    /// Declared unit system; must be [`UNITS`] to machine.
    pub units: String,
    /// Rectangular stock block.
    pub stock: Stock,
    /// Tool table referenced by operations.
    pub tools: Vec<Tool>,
    /// Machining operations in execution order.
    pub operations: Vec<Operation>,
}

impl CamDocument {
    /// Creates an empty document with the supplied stock and no tools.
    pub fn new(stock: Stock) -> Self {
        Self {
            format: FORMAT.to_string(),
            version: VERSION,
            revision: 0,
            units: UNITS.to_string(),
            stock,
            tools: Vec::new(),
            operations: Vec::new(),
        }
    }
}

/// Rectangular stock. The top face is `z = height` and the origin is a corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stock {
    /// Size along X.
    pub width: f64,
    /// Size along Y.
    pub depth: f64,
    /// Size along Z; the top face is at `z = height`.
    pub height: f64,
}

impl Stock {
    /// Creates a stock block from its three sizes.
    pub fn new(width: f64, depth: f64, height: f64) -> Self {
        Self { width, depth, height }
    }
}

/// One cutting tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Unique tool identifier used by operations.
    pub id: String,
    /// Human readable tool name.
    pub name: String,
    /// Positive tool number emitted as `T<n> M6` in G-code.
    pub number: u32,
    /// Cutting diameter.
    pub diameter: f64,
    /// Number of flutes; informational and used in the report.
    pub flutes: u32,
    /// Spindle speed in revolutions per minute.
    pub spindle: f64,
    /// Cutting feed in millimetres per minute.
    pub feed: f64,
    /// Plunge feed in millimetres per minute.
    pub plunge_feed: f64,
}

impl Tool {
    /// Cutter radius used to offset paths and to size pockets.
    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }
}

/// Machining strategy of one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    /// Raster facing of the whole stock top.
    Face,
    /// Outside profile around a rectangle, offset by the cutter radius.
    Contour,
    /// Rectangular pocket cleared by a raster inside the cutter radius.
    Pocket,
    /// Drilling cycle at the listed hole centres.
    Drill,
}

impl OperationKind {
    /// Short identifier used by the CLI and by reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Contour => "contour",
            Self::Pocket => "pocket",
            Self::Drill => "drill",
        }
    }
}

impl std::str::FromStr for OperationKind {
    type Err = crate::CamError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "face" => Ok(Self::Face),
            "contour" => Ok(Self::Contour),
            "pocket" => Ok(Self::Pocket),
            "drill" => Ok(Self::Drill),
            other => Err(crate::CamError::new(
                "operation/unknown-kind",
                format!("unknown operation kind `{other}`; only face, contour, pocket or drill is supported"),
            )),
        }
    }
}

/// One hole centre of a drilling operation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hole {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Hole {
    /// Creates a hole centre.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// One rectangular island left standing inside a pocket.
///
/// Coordinates use the same frame as the pocket and are millimetres. An island
/// occupies `x … x + width` along X and `y … y + height` along Y.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Island {
    /// Lower-left corner X.
    pub x: f64,
    /// Lower-left corner Y.
    pub y: f64,
    /// Island size along X.
    pub width: f64,
    /// Island size along Y.
    pub height: f64,
}

impl Island {
    /// Creates an island from its lower-left corner and size.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

/// One machining operation.
///
/// `stepdown` is the maximum depth of one Z pass. `stepover` defaults to 70 % of
/// the tool diameter for facing and 60 % for pockets. Geometry fields are used
/// by `contour` and `pocket`, `holes` by `drill`, `islands` only by `pocket`, and
/// `peck` only by `drill`. `strategy` selects the rectangle clearing pattern:
/// `raster` (the default) or `spiral` (concentric rectangles). `shape` selects
/// the pocket outline: `rect` (the default) or `circle`, which uses `width` as
/// the diameter and needs `height` equal to `width`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Unique operation identifier.
    pub id: String,
    /// Human readable operation name.
    pub name: String,
    /// Machining strategy.
    pub kind: OperationKind,
    /// Identifier of the tool this operation uses.
    pub tool: String,
    /// Total depth below the stock top.
    pub depth: f64,
    /// Maximum depth removed per Z pass.
    pub stepdown: f64,
    /// Distance between raster lines; `null` selects the kind default.
    #[serde(default)]
    pub stepover: Option<f64>,
    /// Lower-left corner X of the rectangle used by `contour` and `pocket`.
    #[serde(default)]
    pub x: f64,
    /// Lower-left corner Y of the rectangle used by `contour` and `pocket`.
    #[serde(default)]
    pub y: f64,
    /// Rectangle size along X.
    #[serde(default)]
    pub width: f64,
    /// Rectangle size along Y.
    #[serde(default)]
    pub height: f64,
    /// Hole centres of a `drill` operation.
    #[serde(default)]
    pub holes: Vec<Hole>,
    /// Rectangular islands left standing by a `pocket` operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub islands: Vec<Island>,
    /// Peck increment of a `drill` operation; `null` uses a single plunge.
    #[serde(default)]
    pub peck: Option<f64>,
    /// Pocket clearing pattern: `raster` (default) or `spiral`.
    ///
    /// Kept as a string so an unknown value is reported as
    /// `operation/invalid` by validation instead of a parse error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Pocket outline: `rect` (default) or `circle`.
    ///
    /// A circle uses `width` as its diameter and moves the cutter in concentric
    /// rings. Kept as a string so an unknown value is reported as
    /// `operation/invalid` by validation instead of a parse error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
}

impl Operation {
    /// Starts a new operation with the shared fields and zero geometry.
    pub fn new(id: impl Into<String>, kind: OperationKind, tool: impl Into<String>, depth: f64, stepdown: f64) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            kind,
            tool: tool.into(),
            depth,
            stepdown,
            stepover: None,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            holes: Vec::new(),
            islands: Vec::new(),
            peck: None,
            strategy: None,
            shape: None,
        }
    }
}

/// A point in machine coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}

impl Point {
    /// Creates a point.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Euclidean distance to another point.
    pub fn distance_to(self, other: Self) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}
