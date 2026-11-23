//! Read-only operation listing.
//!
//! [`list`] reports every machining operation of a document in document order
//! with the fields a machinist needs to review a job. The per-operation time is
//! the kinematic motion time of that operation alone, computed by the shared
//! cycle-time engine, so the list never introduces a second timing model.

use serde::Serialize;

use crate::{
    CamError,
    estimate::{self, EstimateRequest},
    model::{CamDocument, Operation, OperationKind, UNITS},
    toolpath::RAPID_FEED,
};

/// One operation row of [`OperationListReport`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationListEntry {
    /// Operation identifier.
    pub id: String,
    /// Operation strategy.
    pub kind: OperationKind,
    /// Tool the operation uses.
    pub tool: String,
    /// Human readable operation name.
    pub name: String,
    /// Total depth below the stock top.
    pub depth: f64,
    /// Maximum depth removed per Z pass.
    pub stepdown: f64,
    /// Raster spacing; `null` selects the kind default.
    pub stepover: Option<f64>,
    /// Effective pocket strategy; `null` for every kind without one.
    pub strategy: Option<String>,
    /// Effective contour or pocket shape; `null` for every other kind.
    pub shape: Option<String>,
    /// Number of drilled holes, including a generated pattern; `0` otherwise.
    pub holes: usize,
    /// Number of rectangular islands a pocket clears around; `0` otherwise.
    pub island_count: usize,
    /// Whether the pocket cuts its wall finish loop; `false` otherwise.
    pub finish: bool,
    /// Kinematic motion time of the operation in seconds, without tool-change
    /// overhead.
    pub estimated_seconds: f64,
}

/// Read-only operation list of one document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationListReport {
    /// Report format identifier, always `open-cam-operation-list`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Number of operations in the document.
    pub operation_count: usize,
    /// Number of distinct tools the operations reference.
    pub used_tools: usize,
    /// One entry per operation, in document order.
    pub operations: Vec<OperationListEntry>,
}

/// Builds the operation list report of a document.
///
/// The document is generated first, so an unmachineable file never produces a
/// partial list and the first validation error is returned unchanged. The
/// per-operation time comes from the cycle-time engine with the default rapid
/// feed and no tool-change overhead, so it is the kinematic motion time of that
/// operation alone.
pub fn list(document: &CamDocument) -> Result<OperationListReport, CamError> {
    let request = EstimateRequest { rapid_feed: RAPID_FEED, tool_change_seconds: 0.0 };
    let estimate = estimate::estimate(document, &request)?;

    let mut used_tools: Vec<&str> = Vec::new();
    let operations = document
        .operations
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            if !used_tools.contains(&operation.tool.as_str()) {
                used_tools.push(operation.tool.as_str());
            }
            entry(operation, estimate.operations[index].total_seconds)
        })
        .collect();

    Ok(OperationListReport {
        format: "open-cam-operation-list".to_string(),
        version: 1,
        units: UNITS.to_string(),
        operation_count: document.operations.len(),
        used_tools: used_tools.len(),
        operations,
    })
}

fn entry(operation: &Operation, estimated_seconds: f64) -> OperationListEntry {
    let strategy =
        (operation.kind == OperationKind::Pocket).then(|| operation.strategy.clone().unwrap_or_else(|| "raster".to_string()));
    let shape = matches!(operation.kind, OperationKind::Contour | OperationKind::Pocket)
        .then(|| operation.shape.clone().unwrap_or_else(|| "rect".to_string()));
    let holes = if operation.kind == OperationKind::Drill { operation.resolved_holes().len() } else { 0 };
    let island_count = if operation.kind == OperationKind::Pocket { operation.islands.len() } else { 0 };
    let finish = operation.kind == OperationKind::Pocket && operation.finish == Some(true);
    OperationListEntry {
        id: operation.id.clone(),
        kind: operation.kind,
        tool: operation.tool.clone(),
        name: operation.name.clone(),
        depth: operation.depth,
        stepdown: operation.stepdown,
        stepover: operation.stepover,
        strategy,
        shape,
        holes,
        island_count,
        finish,
        estimated_seconds,
    }
}
