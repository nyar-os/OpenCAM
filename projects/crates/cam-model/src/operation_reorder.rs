//! Operation reordering.
//!
//! [`reorder`] moves one operation to a final 0-based index and returns the
//! re-validated candidate document; the session validates and saves it
//! atomically. An operation is self-contained, so the move only changes the
//! position in the operation array: every field of every operation and every
//! other document structure keep their value. No document structure stores an
//! operation position, so the tool table, sequence plan and operation list are
//! simply derived from the new order when they are read. Ids never change.
//!
//! `to` is the position the operation occupies after the move: the operation is
//! removed from its current index and reinserted at `to`, so `0` puts it first
//! and `operationCount - 1` puts it last. A move to the current index is a
//! no-op: it succeeds without writing and without raising the revision. A
//! document needs at least one operation; with none, every id is unknown.

use serde::Serialize;

use crate::{CamError, model::CamDocument, validate};

/// Reorder request of one operation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationReorder {
    /// Identifier of the operation to move.
    pub id: String,
    /// Final 0-based position in the operation array.
    pub to: usize,
}

impl OperationReorder {
    /// Creates a request that moves `id` to the final index `to`.
    pub fn new(id: impl Into<String>, to: usize) -> Self {
        Self { id: id.into(), to }
    }
}

/// Report returned by a reorder.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationReorderReport {
    /// Report format identifier, always `open-cam-operation-reorder`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Number of operations in the document.
    pub operation_count: usize,
    /// Identifier of the moved operation.
    pub id: String,
    /// 0-based position before the move.
    pub from: usize,
    /// Requested final 0-based position.
    pub to: usize,
    /// `false` when the operation already sat at `to` and nothing was written.
    pub changed: bool,
    /// Document revision the report describes; the saved revision after an
    /// applied move and the current one otherwise.
    pub revision: u32,
}

/// Moves one operation to a final 0-based index and returns the candidate
/// document with its report.
///
/// An unknown id reports `operation/unknown-id` and an index at or beyond the
/// operation count reports `operation/invalid`. A move to the current index
/// returns the document unchanged with `changed: false`, so the caller must not
/// write it. A real move removes the operation and reinserts it at `to`, keeping
/// every field; the whole candidate document is then validated with the same
/// rules as `add`, `update` and `delete`, so a rejected move is returned
/// unchanged as its first validation error. The revision is not changed here;
/// the session bumps it only when it saves the candidate.
pub fn reorder(document: &CamDocument, request: &OperationReorder) -> Result<(CamDocument, OperationReorderReport), CamError> {
    let index = document
        .operations
        .iter()
        .position(|operation| operation.id == request.id)
        .ok_or_else(|| CamError::new("operation/unknown-id", format!("operation `{}` does not exist", request.id)))?;
    let operation_count = document.operations.len();
    if request.to >= operation_count {
        return Err(CamError::new(
            "operation/invalid",
            format!(
                "operation `{}` cannot move to index {}, which is outside 0 … {}",
                request.id,
                request.to,
                operation_count.saturating_sub(1)
            ),
        ));
    }

    let mut report = OperationReorderReport {
        format: "open-cam-operation-reorder".to_string(),
        version: 1,
        units: document.units.clone(),
        operation_count,
        id: request.id.clone(),
        from: index,
        to: request.to,
        changed: false,
        revision: document.revision,
    };
    if request.to == index {
        return Ok((document.clone(), report));
    }

    let mut candidate = document.clone();
    let operation = candidate.operations.remove(index);
    candidate.operations.insert(request.to, operation);

    let validation = validate::validate(&candidate);
    if let Some(error) = validate::first_error(&validation) {
        return Err(error);
    }
    report.changed = true;
    Ok((candidate, report))
}
