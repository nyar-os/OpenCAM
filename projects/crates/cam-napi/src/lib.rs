//! Thin Node-API surface for OpenCAM.
//!
//! Every export performs one orthogonal operation and returns a JSON string, so
//! the TypeScript layer owns argument parsing, help text and exit codes. The
//! document editing, validation, toolpath generation and export calls forward to
//! `cam-model`'s [`cam_model::OpenCam`] session, which the `ocam` binary also
//! calls, so the two entry points cannot disagree.
//!
//! Bad arguments report `session/argument-invalid`; every other failure keeps
//! the stable code of [`cam_model::CamError`].

#![deny(missing_docs)]

use std::path::Path;

use cam_model::{CamError, ExportFormat, Hole, Island, OpenCam, Operation, OperationKind, Stock, Tool};
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
    operation.x = number_field(&request, "x")?.unwrap_or(0.0);
    operation.y = number_field(&request, "y")?.unwrap_or(0.0);
    operation.width = number_field(&request, "width")?.unwrap_or(0.0);
    operation.height = number_field(&request, "height")?.unwrap_or(0.0);
    operation.peck = number_field(&request, "peck")?;
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
    if let Some(Value::Array(holes)) = request.get("holes") {
        let mut parsed = Vec::with_capacity(holes.len());
        for hole in holes {
            parsed.push(Hole::new(required_number_field(hole, "x")?, required_number_field(hole, "y")?));
        }
        operation.holes = parsed;
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
    let summary = session.remove_operation(&id).map_err(fail)?;
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

/// Renders the G-code program of a document, as a JSON string.
#[function]
fn gcode(ctx: CallContext) -> Result<JsValue> {
    let path = text(&ctx, 0)?;
    let mut session = OpenCam::new();
    session.open(Path::new(&path)).map_err(fail)?;
    let program = session.gcode().map_err(fail)?;
    respond(&ctx, &program)
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
    operation_add,
    operation_remove,
    validate,
    toolpaths,
    gcode,
    export,
    report
];
