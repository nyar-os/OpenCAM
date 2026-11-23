//! Thin Node-API surface for OpenCAM.
//!
//! Every export performs one orthogonal operation and returns a JSON string, so
//! the TypeScript layer owns argument parsing, help text and exit codes. The
//! document editing, validation, toolpath generation and export calls forward to
//! `cam-model`'s [`cam_model::OpenCam`] session, which the TypeScript SDK also
//! calls, so the two entry points cannot disagree.
//!
//! Bad arguments report `session/argument-invalid`; every other failure keeps
//! the stable code of [`cam_model::CamError`].

#![deny(missing_docs)]

use std::path::Path;

use cam_model::{
    CamError, EstimateRequest, ExportFormat, FeedsRequest, Hole, HolePattern, Island, LeadSettings, OpenCam, Operation,
    OperationDuplicate, OperationKind, OperationUpdate, PathPoint, PostOptions, Stock, TabSettings, Tool,
};
use serde::Serialize;
use serde_json::Value;
use wae::{CallContext, Error, JsValue, Result, function, register_module};

fn argument_error(message: &str) -> Error {
    Error::new(format!("session/argument-invalid: {message}"))
}

fn argument(ctx: &CallContext, index: usize) -> Result<JsValue> {
    ctx.args().get(index).copied().ok_or_else(|| argument_error("a required argument is missing"))
}

fn text(ctx: &CallContext, index: usize) -> Result<String> {
    argument(ctx, index)?.as_string()
}

fn optional_text(ctx: &CallContext, index: usize) -> Result<Option<String>> {
    match ctx.args().get(index) {
        Some(value) => value.as_string().map(Some),
        None => Ok(None),
    }
}

fn object(ctx: &CallContext, index: usize) -> Result<Value> {
    let raw = text(ctx, index)?;
    serde_json::from_str(&raw).map_err(|error| argument_error(&format!("expected a JSON object: {error}")))
}

fn number_field(request: &Value, name: &str) -> Result<Option<f64>> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_f64().map(Some).ok_or_else(|| argument_error(&format!("`{name}` must be a number"))),
    }
}

fn required_number_field(request: &Value, name: &str) -> Result<f64> {
    number_field(request, name)?.ok_or_else(|| argument_error(&format!("`{name}` is required")))
}

fn text_field(request: &Value, name: &str) -> Result<String> {
    match request.get(name).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => Err(argument_error(&format!("`{name}` must be a non-empty string"))),
    }
}

fn optional_u32(request: &Value, name: &str) -> Result<Option<u32>> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|number| u32::try_from(number).ok())
            .map(Some)
            .ok_or_else(|| argument_error(&format!("`{name}` must be a non-negative integer"))),
    }
}

fn optional_string_field(request: &Value, name: &str) -> Result<Option<String>> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(argument_error(&format!("`{name}` must be a string"))),
    }
}

fn fail(error: CamError) -> Error {
    Error::new(format!("{}: {}", error.code, error.message))
}

fn respond<T: Serialize>(ctx: &CallContext, value: &T) -> Result<JsValue> {
    let text = serde_json::to_string(value).map_err(|error| Error::new(format!("document/serialize: {error}")))?;
    ctx.env().string(&text)
}

/// Library version of the native binding, as a JSON string.
#[function]
fn version(ctx: CallContext) -> Result<JsValue> {
    respond(&ctx, &env!("CARGO_PKG_VERSION"))
}

/// Capability matrix of the engine, as a JSON string.
#[function]
fn capabilities(ctx: CallContext) -> Result<JsValue> {
    ctx.env().string(OpenCam::new().capabilities())
}

/// Creates an empty document and returns its summary.
#[function]
fn create(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let summary = OpenCam::new().create(Path::new(&path)).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Opens a document and returns its summary.
#[function]
fn open(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let summary = OpenCam::new().open(Path::new(&path)).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Writes the open document to `target`, or in place, and returns its summary.
#[function]
fn save(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let target = optional_text(&ctx, 1)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.save(target.as_deref().map(Path::new)).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Replaces the stock block of a document and saves it.
#[function]
fn stock_set(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let stock = Stock::new(
        required_number_field(&request, "width")?,
        required_number_field(&request, "depth")?,
        required_number_field(&request, "height")?,
    );
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.set_stock(stock).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Adds a tool to a document and saves it.
#[function]
fn tool_add(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let id = text_field(&request, "id")?;
    let feed = required_number_field(&request, "feed")?;
    let tool = Tool {
        name: request.get("name").and_then(Value::as_str).unwrap_or(&id).to_string(),
        id,
        number: optional_u32(&request, "number")?.unwrap_or(1),
        diameter: required_number_field(&request, "diameter")?,
        flutes: optional_u32(&request, "flutes")?.unwrap_or(2),
        spindle: required_number_field(&request, "spindle")?,
        feed,
        plunge_feed: number_field(&request, "plungeFeed")?.unwrap_or(feed / 3.0),
    };
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.add_tool(tool).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Removes an unused tool and saves the document.
#[function]
fn tool_remove(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let id = text_field(&request, "id")?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.remove_tool(&id).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Lists every declared tool with the operations that use it.
#[function]
fn tool_list(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.tool_table().map_err(fail)?;
    respond(&ctx, &report)
}

/// Writes the tool table of a document as a portable `open-cam-tools` library.
#[function]
fn tool_export(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let target = text(&ctx, 1)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.write_tool_library(Path::new(&target)).map_err(fail)?;
    respond(&ctx, &report)
}

/// Merges a portable tool library into a document and saves it.
#[function]
fn tool_import(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let source = text(&ctx, 1)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.import_tool_library(Path::new(&source)).map_err(fail)?;
    respond(&ctx, &report)
}

/// Adds a machining operation and saves the document.
#[function]
fn operation_add(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let id = text_field(&request, "id")?;
    let kind: OperationKind = text_field(&request, "kind")?.parse().map_err(|error: CamError| fail(error))?;
    let tool = text_field(&request, "tool")?;
    let depth = required_number_field(&request, "depth")?;
    let stepdown = number_field(&request, "stepdown")?.unwrap_or(depth);
    let mut operation = Operation::new(id.clone(), kind, tool, depth, stepdown);
    operation.name = request.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
    operation.stepover = number_field(&request, "stepover")?;
    operation.ramp = number_field(&request, "ramp")?;
    operation.x = number_field(&request, "x")?.unwrap_or(0.0);
    operation.y = number_field(&request, "y")?.unwrap_or(0.0);
    operation.width = number_field(&request, "width")?.unwrap_or(0.0);
    operation.height = number_field(&request, "height")?.unwrap_or(0.0);
    operation.peck = number_field(&request, "peck")?;
    operation.dwell = number_field(&request, "dwell")?;
    operation.strategy = match request.get("strategy") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err(argument_error("`strategy` must be a string")),
    };
    operation.shape = match request.get("shape") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err(argument_error("`shape` must be a string")),
    };
    operation.arcs = match request.get("arcs") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => return Err(argument_error("`arcs` must be a string")),
    };
    operation.closed = match request.get("closed") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(argument_error("`closed` must be a boolean")),
    };
    operation.finish = match request.get("finish") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => return Err(argument_error("`finish` must be a boolean")),
    };
    if let Some(Value::Array(holes)) = request.get("holes") {
        let mut parsed = Vec::with_capacity(holes.len());
        for hole in holes {
            parsed.push(Hole::new(required_number_field(hole, "x")?, required_number_field(hole, "y")?));
        }
        operation.holes = parsed;
    }
    match request.get("pattern") {
        None | Some(Value::Null) => {}
        Some(Value::Object(_)) => {
            let pattern = request.get("pattern").expect("checked object");
            let kind = match pattern.get("kind") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(value)) => value.clone(),
                Some(_) => return Err(argument_error("`pattern.kind` must be a string")),
            };
            operation.pattern = Some(HolePattern {
                kind,
                rows: optional_u32(pattern, "rows")?.unwrap_or(0),
                columns: optional_u32(pattern, "columns")?.unwrap_or(0),
                spacing_x: number_field(pattern, "spacingX")?.unwrap_or(0.0),
                spacing_y: number_field(pattern, "spacingY")?.unwrap_or(0.0),
                count: optional_u32(pattern, "count")?.unwrap_or(0),
                radius: number_field(pattern, "radius")?.unwrap_or(0.0),
            });
        }
        Some(_) => return Err(argument_error("`pattern` must be an object")),
    }
    if let Some(Value::Array(points)) = request.get("path") {
        let mut parsed = Vec::with_capacity(points.len());
        for point in points {
            parsed.push(PathPoint::new(required_number_field(point, "x")?, required_number_field(point, "y")?));
        }
        operation.path = parsed;
    }
    if let Some(Value::Array(islands)) = request.get("islands") {
        let mut parsed = Vec::with_capacity(islands.len());
        for island in islands {
            parsed.push(Island::new(
                required_number_field(island, "x")?,
                required_number_field(island, "y")?,
                required_number_field(island, "width")?,
                required_number_field(island, "height")?,
            ));
        }
        operation.islands = parsed;
    }
    match request.get("tabs") {
        None | Some(Value::Null) => {}
        Some(Value::Object(_)) => {
            let tabs = request.get("tabs").expect("checked object");
            operation.tabs = Some(TabSettings::new(
                optional_u32(tabs, "count")?.ok_or_else(|| argument_error("`tabs.count` is required"))?,
                required_number_field(tabs, "width")?,
                required_number_field(tabs, "height")?,
            ));
        }
        Some(_) => return Err(argument_error("`tabs` must be an object")),
    }
    match request.get("lead") {
        None | Some(Value::Null) => {}
        Some(Value::Object(_)) => {
            let lead = request.get("lead").expect("checked object");
            let direction = match lead.get("direction") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(value)) => value.clone(),
                Some(_) => return Err(argument_error("`lead.direction` must be a string")),
            };
            operation.lead = Some(LeadSettings::new(required_number_field(lead, "radius")?, direction));
        }
        Some(_) => return Err(argument_error("`lead` must be an object")),
    }
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.add_operation(operation).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Removes an operation and saves the document.
#[function]
fn operation_remove(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let id = text_field(&request, "id")?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.delete_operation(&id).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Deletes an operation and saves the document.
#[function]
fn operation_delete(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let id = text_field(&request, "id")?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.delete_operation(&id).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Duplicates an operation after itself and saves the document.
#[function]
fn operation_duplicate(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let duplicate = OperationDuplicate { id: text_field(&request, "id")?, new_id: optional_string_field(&request, "newId")? };
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.duplicate_operation(&duplicate).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Lists every operation of a document without changing it.
#[function]
fn operation_list(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.operation_list().map_err(fail)?;
    respond(&ctx, &report)
}

/// Applies a partial update to one operation and saves the document.
#[function]
fn operation_update(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = object(&ctx, 1)?;
    let update = OperationUpdate {
        id: text_field(&request, "id")?,
        tool: optional_string_field(&request, "tool")?,
        depth: number_field(&request, "depth")?,
        stepdown: number_field(&request, "stepdown")?,
        stepover: number_field(&request, "stepover")?,
        name: optional_string_field(&request, "name")?,
        peck: number_field(&request, "peck")?,
        dwell: number_field(&request, "dwell")?,
    };
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let summary = session.update_operation(&update).map_err(fail)?;
    respond(&ctx, &summary)
}

/// Validates a document.
#[function]
fn validate(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.validate().map_err(fail)?;
    respond(&ctx, &report)
}

/// Generates the toolpath report of a document.
#[function]
fn toolpaths(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.toolpaths().map_err(fail)?;
    respond(&ctx, &report)
}

/// Estimates the cycle time of a document from an optional JSON request.
#[function]
fn estimate(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let request = match optional_text(&ctx, 1)? {
        None => EstimateRequest::default(),
        Some(raw) => serde_json::from_str(&raw).map_err(|error| argument_error(&format!("expected a JSON object: {error}")))?,
    };
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.estimate(&request).map_err(fail)?;
    respond(&ctx, &report)
}

/// Renders the G-code program of a document, as a JSON string.
///
/// The optional second argument is a JSON post option set; without it the
/// generic program with no program number, line numbers or coolant is rendered.
#[function]
fn gcode(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let post = match optional_text(&ctx, 1)? {
        None => PostOptions::default(),
        Some(raw) => serde_json::from_str(&raw).map_err(|error| argument_error(&format!("expected a JSON object: {error}")))?,
    };
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let program = session.gcode_with_post(&post).map_err(fail)?;
    respond(&ctx, &program)
}

/// Plans the tool-grouped operation order of a document without changing it.
#[function]
fn sequence(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.sequence().map_err(fail)?;
    respond(&ctx, &report)
}

/// Reorders a document by tool, saves it and returns the applied plan.
#[function]
fn sequence_apply(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.apply_sequence().map_err(fail)?;
    respond(&ctx, &report)
}

/// Suggests spindle and feed rates from a JSON request.
///
/// The request carries `material` and `diameter` and an optional `flutes`; the
/// report is the `open-cam-feeds` lookup of the shared engine. An unknown
/// material reports `feeds/unknown-material` and an invalid request
/// `feeds/invalid`.
#[function]
fn feeds(ctx: CallContext) -> Result<JsValue> {
    let raw = optional_text(&ctx, 0)?.ok_or_else(|| argument_error("a JSON request is required"))?;
    let request: FeedsRequest =
        serde_json::from_str(&raw).map_err(|error| argument_error(&format!("expected a JSON object: {error}")))?;
    let report = OpenCam::new().suggest_feeds(&request).map_err(fail)?;
    respond(&ctx, &report)
}

/// Exports a document as `gcode`, `json`, `csv` or `md`.
#[function]
fn export(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let format = text(&ctx, 1)?.parse::<ExportFormat>().map_err(fail)?;
    let target = text(&ctx, 2)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.export(format, Path::new(&target)).map_err(fail)?;
    respond(&ctx, &report)
}

/// Returns the full report of a document.
#[function]
fn report(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let report = session.report().map_err(fail)?;
    respond(&ctx, &report)
}

register_module![
    version,
    capabilities,
    create,
    open,
    save,
    stock_set,
    tool_add,
    tool_remove,
    tool_list,
    tool_export,
    tool_import,
    operation_add,
    operation_remove,
    operation_delete,
    operation_duplicate,
    operation_list,
    operation_update,
    validate,
    toolpaths,
    estimate,
    gcode,
    sequence,
    sequence_apply,
    feeds,
    export,
    report
];
