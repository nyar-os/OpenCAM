//! Partial operation updates.
//!
//! [`update`] changes a selected subset of one operation's fields and returns
//! the re-validated candidate document; the session validates and saves it
//! atomically. The update deliberately stays narrow: it changes the tool, name,
//! depth, stepdown, stepover, peck and dwell of an existing operation. The kind,
//! geometry coordinates, holes, islands, tabs, lead, ramp and arcs stay
//! untouched, so a partial edit can never quietly redefine the shape of a job.

use crate::{
    CamError,
    model::{CamDocument, Operation, OperationKind},
    validate,
};

/// Partial update request of one operation.
///
/// Every field except `id` is optional; only the supplied fields change. An
/// update that supplies no field reports `operation/invalid`. `peck` and
/// `dwell` are accepted only by a `drill` and cannot be combined, mirroring the
/// add-time rule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationUpdate {
    /// Identifier of the operation to edit.
    pub id: String,
    /// New tool identifier; must name a declared tool.
    pub tool: Option<String>,
    /// New total depth below the stock top, in millimetres.
    pub depth: Option<f64>,
    /// New maximum depth of one Z pass, in millimetres.
    pub stepdown: Option<f64>,
    /// New raster spacing, in millimetres.
    pub stepover: Option<f64>,
    /// New human readable name.
    pub name: Option<String>,
    /// New peck increment of a `drill`, in millimetres.
    pub peck: Option<f64>,
    /// New bottom-of-hole pause of a `drill`, in seconds.
    pub dwell: Option<f64>,
}

impl OperationUpdate {
    /// Creates a request that identifies one operation and changes nothing.
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), ..Self::default() }
    }

    /// Whether at least one editable field is supplied.
    pub fn has_changes(&self) -> bool {
        self.tool.is_some()
            || self.depth.is_some()
            || self.stepdown.is_some()
            || self.stepover.is_some()
            || self.name.is_some()
            || self.peck.is_some()
            || self.dwell.is_some()
    }
}

/// Applies a partial update to one operation and returns the candidate document.
///
/// The supplied fields are checked individually: depth, stepdown, stepover,
/// peck and dwell must be finite and greater than zero, and peck and dwell are
/// accepted only by a `drill`. The whole candidate document is then validated
/// with the same rules as `add` and `save`, so an unknown tool, an oversize
/// stepover, an invalid combination or any other existing error is returned
/// unchanged and the caller must not persist the document. The revision is not
/// changed here; the session bumps it only when it saves the candidate.
pub fn update(document: &CamDocument, request: &OperationUpdate) -> Result<CamDocument, CamError> {
    if !request.has_changes() {
        return Err(CamError::new(
            "operation/invalid",
            format!(
                "operation `{}` update needs at least one of tool, depth, stepdown, stepover, name, peck or dwell",
                request.id
            ),
        ));
    }

    let mut candidate = document.clone();
    let operation = candidate
        .operations
        .iter_mut()
        .find(|operation| operation.id == request.id)
        .ok_or_else(|| CamError::new("operation/unknown-id", format!("operation `{}` does not exist", request.id)))?;

    if let Some(tool) = &request.tool {
        operation.tool = tool.clone();
    }
    if let Some(name) = &request.name {
        operation.name = name.clone();
    }
    if let Some(depth) = request.depth {
        check_positive(request, "depth", depth)?;
        operation.depth = depth;
    }
    if let Some(stepdown) = request.stepdown {
        check_positive(request, "stepdown", stepdown)?;
        operation.stepdown = stepdown;
    }
    if let Some(stepover) = request.stepover {
        check_positive(request, "stepover", stepover)?;
        operation.stepover = Some(stepover);
    }
    if let Some(peck) = request.peck {
        check_drill_only(request, operation, "peck")?;
        check_positive(request, "peck", peck)?;
        operation.peck = Some(peck);
    }
    if let Some(dwell) = request.dwell {
        check_drill_only(request, operation, "dwell")?;
        check_positive(request, "dwell", dwell)?;
        operation.dwell = Some(dwell);
    }
    if operation.kind == OperationKind::Drill && operation.dwell.is_some() && operation.peck.is_some() {
        return Err(CamError::new(
            "operation/invalid",
            format!(
                "operation `{}` combines dwell with peck, but `G83` has no dwell; update one of the two instead",
                request.id
            ),
        ));
    }

    let report = validate::validate(&candidate);
    if let Some(error) = validate::first_error(&report) {
        return Err(error);
    }
    Ok(candidate)
}

fn check_positive(request: &OperationUpdate, field: &str, value: f64) -> Result<(), CamError> {
    if value.is_finite() && value > 0.0 {
        return Ok(());
    }
    Err(CamError::new(
        "operation/invalid",
        format!("operation `{}` update needs a finite `{field}` greater than zero, got `{value}`", request.id),
    ))
}

fn check_drill_only(request: &OperationUpdate, operation: &Operation, field: &str) -> Result<(), CamError> {
    if operation.kind == OperationKind::Drill {
        return Ok(());
    }
    Err(CamError::new(
        "operation/invalid",
        format!("operation `{}` update uses `{field}` but only `drill` supports it", request.id),
    ))
}
