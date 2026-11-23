//! Operation duplication.
//!
//! [`duplicate`] copies one operation of a document and returns the
//! re-validated candidate; the session validates and saves it atomically. The
//! copy keeps every field of its source — kind, tool, geometry, holes, islands,
//! tabs, lead, ramp, arcs and name — and is inserted immediately after the
//! source in document order. Only the identifier changes.

use crate::{CamError, model::CamDocument, validate};

/// Duplication request of one operation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationDuplicate {
    /// Identifier of the operation to copy.
    pub id: String,
    /// Identifier of the copy; `None` generates the next free `op-N`.
    pub new_id: Option<String>,
}

impl OperationDuplicate {
    /// Creates a request that copies `id` under a generated identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), new_id: None }
    }

    /// Creates a request that copies `id` under the supplied identifier.
    pub fn with_new_id(id: impl Into<String>, new_id: impl Into<String>) -> Self {
        Self { id: id.into(), new_id: Some(new_id.into()) }
    }
}

/// Copies one operation and returns the candidate document.
///
/// An unknown source reports `operation/unknown-id`. A supplied `new_id` must be
/// a non-empty identifier that no operation declares yet, otherwise the request
/// reports `operation/invalid` or `operation/duplicate-id`; a missing `new_id`
/// takes the smallest free `op-<n>` identifier, starting at `op-1`. The copy
/// keeps the source name exactly and is inserted directly after the source. The
/// whole candidate document is validated with the same rules as `add`, `update`
/// and `save`, so a rejected duplicate is returned unchanged as its first
/// validation error. The revision is not changed here; the session bumps it only
/// when it saves the candidate.
pub fn duplicate(document: &CamDocument, request: &OperationDuplicate) -> Result<CamDocument, CamError> {
    let index = document
        .operations
        .iter()
        .position(|operation| operation.id == request.id)
        .ok_or_else(|| CamError::new("operation/unknown-id", format!("operation `{}` does not exist", request.id)))?;

    let new_id = match &request.new_id {
        Some(new_id) => check_new_id(document, new_id)?,
        None => next_free_id(document),
    };

    let mut copy = document.operations[index].clone();
    copy.id = new_id;

    let mut candidate = document.clone();
    candidate.operations.insert(index + 1, copy);

    let report = validate::validate(&candidate);
    if let Some(error) = validate::first_error(&report) {
        return Err(error);
    }
    Ok(candidate)
}

/// Checks a supplied identifier and returns it.
fn check_new_id(document: &CamDocument, new_id: &str) -> Result<String, CamError> {
    if new_id.trim().is_empty() {
        return Err(CamError::new("operation/invalid", "a duplicated operation needs a non-empty id"));
    }
    if document.operations.iter().any(|operation| operation.id == new_id) {
        return Err(CamError::new("operation/duplicate-id", format!("operation `{new_id}` already exists")));
    }
    Ok(new_id.to_string())
}

/// Returns the smallest positive `op-<n>` identifier the document does not use.
fn next_free_id(document: &CamDocument) -> String {
    let mut number = 1_u64;
    loop {
        let candidate = format!("op-{number}");
        if !document.operations.iter().any(|operation| operation.id == candidate) {
            return candidate;
        }
        number += 1;
    }
}
