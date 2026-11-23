//! OpenCAM: a deterministic 2.5-axis CAM engine.
//!
//! This crate implements a deliberately small machining kernel:
//!
//! * one rectangular stock block, measured in millimetres;
//! * a tool table with diameter, flute count, spindle speed and cutting feeds;
//! * five machining strategies — raster facing, outside contouring (rectangle
//!   or circle, with optional half-circle tangential lead-in and lead-out arcs
//!   rendered as `G2`/`G3`), pocket clearing (rectangular raster or spiral,
//!   circular concentric rings, optionally around rectangular islands),
//!   drilling cycles and polyline engraving;
//! * multi-pass Z depth control through `stepdown`, and raster spacing through
//!   `stepover`;
//! * a deterministic toolpath generator with cutting/rapid lengths and cycle
//!   time estimation;
//! * a configurable cycle-time estimate report splitting every operation into
//!   cutting, plunge, retract, traverse and tool-change time;
//! * portable `open-cam-tools` library files plus a tool-table report that ties
//!   every declared tool to the operations that use it;
//! * a generic absolute-millimetre G-code writer (`G21 G90 G17 G94`) including
//!   `G81`/`G83` drilling cycles;
//! * JSON, CSV and Markdown toolpath reports.
//!
//! Three-dimensional surfacing, adaptive clearing, turning, four-axis work,
//! STEP/DXF import and vendor post-processors are **not** implemented. See
//! [`CAPABILITIES_JSON`] for the machine-readable capability matrix.

#![deny(missing_docs)]

mod error;
mod estimate;
mod export;
mod gcode;
mod library;
mod model;
mod session;
mod toolpath;
pub mod validate;

pub use crate::{
    error::{CamError, not_implemented},
    estimate::{
        CycleTimeReport, DEFAULT_TOOL_CHANGE_SECONDS, EstimateRequest, OperationEstimate, estimate as estimate_cycle_time,
    },
    export::{markdown, segments_csv, write_atomic},
    gcode::render as render_gcode,
    library::{LIBRARY_FORMAT, LIBRARY_VERSION, ToolEntry, ToolLibrary, ToolTableReport},
    model::{
        CamDocument, FORMAT, Hole, Island, LeadSettings, Operation, OperationKind, PathPoint, Point, Stock, TabSettings, Tool,
        UNITS, VERSION,
    },
    session::{DEFAULT_STOCK, DocumentSummary, ExportFormat, ExportReport, OpenCam, Report, ToolImportReport},
    toolpath::{CLEARANCE, Move, MoveKind, OperationPath, RAPID_FEED, ToolpathReport, generate as generate_toolpaths},
    validate::{Issue, Severity, ValidationReport},
};

/// Stable machine-readable capability matrix of this build.
///
/// Supported entries are marked `"supported"`; everything else is
/// `"unsupported"` and must not be silently treated as working.
pub const CAPABILITIES_JSON: &str = r#"{
  "operations": {
    "face": "supported",
    "contour": "supported",
    "contourCircle": "supported",
    "contourLead": "supported",
    "circleArcs": "supported",
    "tabs": "supported",
    "pocket": "supported",
    "pocketRect": "supported",
    "pocketCircle": "supported",
    "pocketSpiral": "supported",
    "pocketIslands": "supported",
    "pocketFinish": "supported",
    "drill": "supported",
    "drillDwell": "supported",
    "engrave": "supported",
    "engraveClosed": "supported",
    "ramp": "supported",
    "adaptive": "unsupported",
    "turning": "unsupported",
    "fourAxis": "unsupported"
  },
  "io": {
    "ocam": "supported",
    "gcode": "supported",
    "json": "supported",
    "csv": "supported",
    "md": "supported",
    "toolLibrary": "supported",
    "step": "unsupported",
    "dxf": "unsupported"
  }
}"#;
