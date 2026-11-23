//! Cycle time estimation.
//!
//! The estimator reuses the generated toolpath moves and splits them into
//! cutting, plunge, retract and traverse time, then charges one fixed
//! tool-change overhead per distinct tool. It is deterministic: the same
//! document and the same request always produce the same report.

use serde::{Deserialize, Serialize};

use crate::{
    CamError,
    model::{CamDocument, OperationKind},
    toolpath::{self, RAPID_FEED},
};

/// Default fixed tool-change overhead charged once per distinct tool, in seconds.
pub const DEFAULT_TOOL_CHANGE_SECONDS: f64 = 3.0;

/// Geometric tolerance used when classifying a rapid move as a vertical lift.
const TOLERANCE: f64 = 1e-9;

/// Rate and overhead parameters of [`estimate`].
///
/// Omitted fields select the engine defaults, so an empty request estimates at
/// the machine defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EstimateRequest {
    /// Rapid traverse rate in millimetres per minute.
    pub rapid_feed: f64,
    /// Fixed tool-change overhead per distinct tool, in seconds.
    pub tool_change_seconds: f64,
}

impl Default for EstimateRequest {
    fn default() -> Self {
        Self { rapid_feed: RAPID_FEED, tool_change_seconds: DEFAULT_TOOL_CHANGE_SECONDS }
    }
}

/// Cycle time estimate of one operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEstimate {
    /// Operation identifier.
    pub operation: String,
    /// Operation strategy.
    pub kind: OperationKind,
    /// Tool identifier.
    pub tool: String,
    /// Length of cutting and arc moves, in millimetres.
    pub cutting_length: f64,
    /// Length of vertical plunge moves, in millimetres.
    pub plunge_length: f64,
    /// Length of vertical rapid lifts, in millimetres.
    pub retract_length: f64,
    /// Length of horizontal rapid moves, in millimetres.
    pub traverse_length: f64,
    /// Cutting and arc time at the move feeds, in seconds.
    pub cutting_seconds: f64,
    /// Plunge time at the plunge feeds, in seconds.
    pub plunge_seconds: f64,
    /// Rapid lift time at the request rapid feed, in seconds.
    pub retract_seconds: f64,
    /// Rapid traverse time at the request rapid feed, in seconds.
    pub traverse_seconds: f64,
    /// Tool-change overhead; the request amount on the first operation that
    /// uses a tool and `0.0` for every later operation with the same tool.
    pub toolchange_seconds: f64,
    /// Sum of the five time components.
    pub total_seconds: f64,
}

/// Complete cycle time report of a document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleTimeReport {
    /// Format identifier, always `open-cam-cycle-time`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Rapid traverse rate used for every rapid move.
    pub rapid_feed: f64,
    /// Tool-change overhead charged once per distinct tool.
    pub tool_change_seconds: f64,
    /// Number of distinct tools that required a change.
    pub tool_changes: usize,
    /// Total cutting and arc time over every operation.
    pub cutting_seconds: f64,
    /// Total plunge time over every operation.
    pub plunge_seconds: f64,
    /// Total rapid lift time over every operation.
    pub retract_seconds: f64,
    /// Total rapid traverse time over every operation.
    pub traverse_seconds: f64,
    /// Total tool-change overhead over every operation.
    pub toolchange_seconds: f64,
    /// Estimated program cycle time in seconds.
    pub total_seconds: f64,
    /// One entry per operation, in document order.
    pub operations: Vec<OperationEstimate>,
}

/// Estimates the cycle time of a document from its generated toolpaths.
///
/// The document is validated first, so an unmachinable file never produces a
/// partial estimate. The first validation error is returned unchanged.
pub fn estimate(document: &CamDocument, request: &EstimateRequest) -> Result<CycleTimeReport, CamError> {
    if !request.rapid_feed.is_finite() || request.rapid_feed <= 0.0 {
        return Err(CamError::new(
            "estimate/invalid",
            format!("rapid feed must be a finite number greater than zero, got `{}`", request.rapid_feed),
        ));
    }
    if !request.tool_change_seconds.is_finite() || request.tool_change_seconds < 0.0 {
        return Err(CamError::new(
            "estimate/invalid",
            format!("tool-change overhead must be a finite number of at least zero, got `{}`", request.tool_change_seconds),
        ));
    }

    let paths = toolpath::generate(document)?;
    let mut used: Vec<&str> = Vec::new();
    let mut operations = Vec::with_capacity(paths.operations.len());

    for path in &paths.operations {
        let mut estimate = OperationEstimate {
            operation: path.operation.clone(),
            kind: path.kind,
            tool: path.tool.clone(),
            cutting_length: 0.0,
            plunge_length: 0.0,
            retract_length: 0.0,
            traverse_length: 0.0,
            cutting_seconds: 0.0,
            plunge_seconds: 0.0,
            retract_seconds: 0.0,
            traverse_seconds: 0.0,
            toolchange_seconds: 0.0,
            total_seconds: 0.0,
        };
        for segment in &path.segments {
            let seconds = segment.length / segment.feed * 60.0;
            match segment.kind {
                toolpath::MoveKind::Cut | toolpath::MoveKind::Arc => {
                    estimate.cutting_length += segment.length;
                    estimate.cutting_seconds += seconds;
                }
                toolpath::MoveKind::Plunge => {
                    estimate.plunge_length += segment.length;
                    estimate.plunge_seconds += seconds;
                }
                toolpath::MoveKind::Rapid => {
                    let lifted = (segment.from.x - segment.to.x).abs() <= TOLERANCE
                        && (segment.from.y - segment.to.y).abs() <= TOLERANCE
                        && segment.to.z > segment.from.z;
                    let seconds = segment.length / request.rapid_feed * 60.0;
                    if lifted {
                        estimate.retract_length += segment.length;
                        estimate.retract_seconds += seconds;
                    }
                    else {
                        estimate.traverse_length += segment.length;
                        estimate.traverse_seconds += seconds;
                    }
                }
            }
        }
        if !used.contains(&path.tool.as_str()) {
            used.push(path.tool.as_str());
            estimate.toolchange_seconds = request.tool_change_seconds;
        }
        estimate.total_seconds = estimate.cutting_seconds
            + estimate.plunge_seconds
            + estimate.retract_seconds
            + estimate.traverse_seconds
            + estimate.toolchange_seconds;
        operations.push(estimate);
    }

    let mut report = CycleTimeReport {
        format: "open-cam-cycle-time".to_string(),
        version: 1,
        units: document.units.clone(),
        rapid_feed: request.rapid_feed,
        tool_change_seconds: request.tool_change_seconds,
        tool_changes: used.len(),
        cutting_seconds: 0.0,
        plunge_seconds: 0.0,
        retract_seconds: 0.0,
        traverse_seconds: 0.0,
        toolchange_seconds: 0.0,
        total_seconds: 0.0,
        operations,
    };
    for operation in &report.operations {
        report.cutting_seconds += operation.cutting_seconds;
        report.plunge_seconds += operation.plunge_seconds;
        report.retract_seconds += operation.retract_seconds;
        report.traverse_seconds += operation.traverse_seconds;
        report.toolchange_seconds += operation.toolchange_seconds;
        report.total_seconds += operation.total_seconds;
    }
    Ok(report)
}
