//! Operation deletion.
//!
//! [`delete`] removes one operation and returns the re-validated candidate
//! document; the session validates and saves it atomically. An operation is
//! self-contained: its geometry, holes, islands, tabs, lead, ramp and arcs live
//! inside the [`Operation`] itself, and the tool table, sequencing plan and
//! operation list are derived from the operation array on demand. No structure
//! stores an operation id, so a delete never leaves a dangling reference and no
//! cross-reference guard is required. Removing the last operation is allowed:
//! it only produces the whole-document warning `operation/none`.

use crate::{CamError, model::CamDocument, validate};

/// Removes one operation and returns the candidate document.
///
/// An unknown id reports `operation/unknown-id`. The remaining operations keep
/// their document order and every other field. The whole candidate document is
/// validated with the same rules as `add`, `update` and `save`, so an already
/// invalid document is returned unchanged as its first validation error. The
/// revision is not changed here; the session bumps it only when it saves the
/// candidate.
pub fn delete(document: &CamDocument, id: &str) -> Result<CamDocument, CamError> {
    let index = document
        .operations
        .iter()
        .position(|operation| operation.id == id)
        .ok_or_else(|| CamError::new("operation/unknown-id", format!("operation `{id}` does not exist")))?;

    let mut candidate = document.clone();
    candidate.operations.remove(index);

    let report = validate::validate(&candidate);
    if let Some(error) = validate::first_error(&report) {
        return Err(error);
    }
    Ok(candidate)
}
