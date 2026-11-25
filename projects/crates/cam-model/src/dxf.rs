//! ASCII DXF import and export of a small 2D subset.
//!
//! [`export`] writes the document geometry back as the same subset: the stock
//! rectangle on layer `STOCK`, every operation outline, island and drill hole
//! on a layer derived from the operation id, as `LINE`, `LWPOLYLINE` and
//! `CIRCLE` entities. Coordinates are millimetres rounded to three decimals,
//! so the same document always produces the same bytes.
//!
//! The reader understands `LINE`, `LWPOLYLINE`, `POLYLINE` with `VERTEX`
//! records, `ARC` and `CIRCLE` in the `ENTITIES` section of an ASCII DXF file.
//! Every supported entity becomes one `engrave` operation. Arcs and circles are
//! flattened into chords with a deterministic sagitta tolerance; the DXF layer
//! names the operation. Other entity types and sections never fail the import:
//! they are reported in the loss list. A malformed group code or a supported
//! entity with missing or invalid values fails with a line number.
//!
//! Coordinates are millimetres. `$INSUNITS` is reported but never converted;
//! `0` (unitless) and `4` (millimetres) are accepted, any other value is
//! reported as a loss and read as millimetres. Z coordinates are ignored.

use std::f64::consts::{PI, TAU};

use serde::Serialize;

use crate::{
    CamError,
    model::{CamDocument, Operation, OperationKind, PathPoint, Stock, Tool},
    session::{DEFAULT_STOCK, DocumentSummary},
};

/// Report format identifier of [`crate::OpenCam::import_dxf`].
pub const DXF_IMPORT_FORMAT: &str = "open-cam-dxf-import";
/// Report schema version of [`crate::OpenCam::import_dxf`].
pub const DXF_IMPORT_VERSION: u32 = 1;
/// Cut depth used when the caller supplies none, in millimetres.
pub const DEFAULT_DXF_DEPTH: f64 = 1.0;
/// Chord sagitta tolerance used when the caller supplies none, in millimetres.
pub const DEFAULT_DXF_TOLERANCE: f64 = 0.05;
/// Identifier of the fixed engraving tool every imported operation references.
pub const DEFAULT_ENGRAVE_TOOL_ID: &str = "engrave";
/// Upper bound on the chords one arc or circle may be flattened into.
pub const MAX_ARC_SEGMENTS: usize = 4096;
/// A circle is always emitted as at least this many chords.
pub const MIN_CIRCLE_SEGMENTS: usize = 3;

/// Two consecutive vertices closer than this are merged, in millimetres.
const MERGE_EPSILON: f64 = 1e-6;
/// Air above the cut floor added to the derived stock height, in millimetres.
const STOCK_MARGIN: f64 = 5.0;

/// Request of [`crate::OpenCam::import_dxf`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxfImportRequest {
    /// Cut depth of every imported operation, in millimetres.
    pub depth: f64,
    /// Maximum depth removed per Z pass, in millimetres.
    pub stepdown: f64,
    /// Maximum chord sagitta of a flattened arc or circle, in millimetres.
    pub tolerance: f64,
}

impl Default for DxfImportRequest {
    fn default() -> Self {
        Self { depth: DEFAULT_DXF_DEPTH, stepdown: DEFAULT_DXF_DEPTH, tolerance: DEFAULT_DXF_TOLERANCE }
    }
}

impl DxfImportRequest {
    /// Creates a request from explicit values.
    pub const fn new(depth: f64, stepdown: f64, tolerance: f64) -> Self {
        Self { depth, stepdown, tolerance }
    }

    /// Rejects values that are not finite and greater than zero.
    pub fn checked(self) -> Result<Self, CamError> {
        for (name, value) in [("depth", self.depth), ("stepdown", self.stepdown), ("tolerance", self.tolerance)] {
            if !value.is_finite() || value <= 0.0 {
                return Err(CamError::new("import/invalid", format!("`{name}` must be finite and greater than zero")));
            }
        }
        Ok(self)
    }
}

/// One entity or section that the importer did not convert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DxfLoss {
    /// Stable loss code, for example `import/dxf-unsupported-entity`.
    pub code: String,
    /// DXF entity type, or `SECTION` for a skipped section.
    pub entity: String,
    /// DXF layer name; empty for a skipped section.
    pub layer: String,
    /// 1-based line of the entity or section header.
    pub line: usize,
    /// Human readable explanation.
    pub message: String,
}

/// Result of parsing a DXF file before it is written as a document.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfImport {
    /// Imported document with revision `0`.
    pub document: CamDocument,
    /// Raw `$INSUNITS` value, when the header declared one.
    pub ins_units: Option<u32>,
    /// Unit note: `millimetres`, `unitless` or `unspecified`.
    pub source_units: String,
    /// Number of supported entities converted into an operation.
    pub entity_count: usize,
    /// Number of entities skipped because their type or geometry produced nothing.
    pub skipped_count: usize,
    /// Every skipped entity, section and unit declaration.
    pub losses: Vec<DxfLoss>,
}

/// Report returned by [`crate::OpenCam::import_dxf`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DxfImportReport {
    /// Report format identifier, always [`DXF_IMPORT_FORMAT`].
    pub format: String,
    /// Report version, always [`DXF_IMPORT_VERSION`].
    pub version: u32,
    /// Declared document unit system, always `mm`.
    pub units: String,
    /// DXF file that was read, exactly as the caller supplied it.
    pub source: String,
    /// Document that was written, exactly as the caller supplied it.
    pub path: String,
    /// Raw `$INSUNITS` value, or `null` when the header declared none.
    pub ins_units: Option<u32>,
    /// Unit note: `millimetres`, `unitless` or `unspecified`.
    pub source_units: String,
    /// Chord sagitta tolerance used to flatten arcs and circles.
    pub tolerance: f64,
    /// Cut depth of every imported operation.
    pub depth: f64,
    /// Stepdown of every imported operation.
    pub stepdown: f64,
    /// Number of entities converted into an operation.
    pub entity_count: usize,
    /// Number of engrave operations in the written document.
    pub operation_count: usize,
    /// Number of entities skipped by type or because they had no usable geometry.
    pub skipped_count: usize,
    /// Skipped entities, sections and units, in file order.
    pub losses: Vec<DxfLoss>,
    /// Summary of the written document.
    pub document: DocumentSummary,
}

#[derive(Debug, Clone)]
struct Group {
    code: i32,
    value: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct RawEntity {
    name: String,
    line: usize,
    groups: Vec<Group>,
}

#[derive(Debug, Clone)]
struct ParsedEntity {
    layer: String,
    points: Vec<(f64, f64)>,
    closed: bool,
}

#[derive(Debug, Default)]
struct UnitsInfo {
    value: Option<u32>,
    line: Option<usize>,
}

/// Parses `text` and builds an imported document with revision `0`.
///
/// The result is deterministic: the same bytes and request always produce the
/// same document and loss list.
pub fn import(text: &str, request: &DxfImportRequest) -> Result<DxfImport, CamError> {
    let request = request.checked()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let groups = read_groups(text)?;
    let mut losses = Vec::new();
    let units = read_units(&groups);
    let source_units = unit_note(&units, &mut losses);
    let entities = scan_sections(&groups, &mut losses)?;

    let mut parsed = Vec::new();
    let mut skipped = 0usize;
    let mut index = 0;
    while index < entities.len() {
        let raw = &entities[index];
        let name = raw.name.as_str();
        if name == "POLYLINE" {
            let (entity, next) = parse_polyline(&entities, index)?;
            index = next;
            match entity {
                Some(entity) => parsed.push(entity),
                None => {
                    skipped += 1;
                    losses.push(empty_loss(raw));
                }
            }
            continue;
        }
        if name == "VERTEX" || name == "SEQEND" {
            index += 1;
            continue;
        }
        let supported = matches!(name, "LINE" | "LWPOLYLINE" | "ARC" | "CIRCLE");
        if !supported {
            losses.push(DxfLoss {
                code: "import/dxf-unsupported-entity".to_string(),
                entity: name.to_string(),
                layer: raw_layer(raw),
                line: raw.line,
                message: format!("entity `{name}` is not supported and was skipped"),
            });
            skipped += 1;
            index += 1;
            continue;
        }
        let entity = match name {
            "LINE" => parse_line(raw)?,
            "LWPOLYLINE" => parse_lwpolyline(raw, &mut losses)?,
            "ARC" => parse_arc(raw, request.tolerance)?,
            _ => parse_circle(raw, request.tolerance)?,
        };
        match entity {
            Some(entity) => parsed.push(entity),
            None => {
                skipped += 1;
                losses.push(empty_loss(raw));
            }
        }
        index += 1;
    }

    let document = build_document(&parsed, &request);
    Ok(DxfImport { document, ins_units: units.value, source_units, entity_count: parsed.len(), skipped_count: skipped, losses })
}

/// Writes the document geometry as an ASCII DXF drawing.
///
/// The `HEADER` declares `$INSUNITS` `4` (millimetres) and the `ENTITIES`
/// section carries, in document order: a closed `LWPOLYLINE` of the stock
/// rectangle on layer `STOCK`; every `engrave` path as a `LINE` (two open
/// points) or a `LWPOLYLINE` whose flag `70` is `1` when closed; every
/// `contour` and `pocket` outline as a closed rectangle `LWPOLYLINE` or, for
/// `shape` `circle`, a `CIRCLE`; every pocket island likewise; and every
/// `drill` hole as a `CIRCLE` of the tool radius. A `face` operation adds no
/// entity because the stock rectangle already spans it. Operation layers are
/// the operation ids with every character outside `[A-Za-z0-9_-]` replaced by
/// `-`; an empty result falls back to `OPERATION`.
pub fn export(document: &CamDocument) -> String {
    let mut text = String::from("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    rectangle(&mut text, "STOCK", 0.0, 0.0, document.stock.width, document.stock.depth);
    for operation in &document.operations {
        let layer = layer_name(&operation.id);
        match operation.kind {
            OperationKind::Engrave => {
                let points: Vec<(f64, f64)> = operation.path.iter().map(|point| (point.x, point.y)).collect();
                if points.len() == 2 && !operation.closed {
                    line(&mut text, &layer, points[0], points[1]);
                }
                else if points.len() >= 2 {
                    polyline(&mut text, &layer, &points, operation.closed);
                }
            }
            OperationKind::Contour | OperationKind::Pocket => {
                outline(&mut text, &layer, operation);
                for island in &operation.islands {
                    if island.shape.as_deref() == Some("circle") {
                        circle(
                            &mut text,
                            &layer,
                            (island.x + island.width / 2.0, island.y + island.height / 2.0),
                            island.width / 2.0,
                        );
                    }
                    else {
                        rectangle(&mut text, &layer, island.x, island.y, island.width, island.height);
                    }
                }
            }
            OperationKind::Drill => {
                if let Some(radius) = document.tools.iter().find(|tool| tool.id == operation.tool).map(Tool::radius) {
                    for hole in operation.resolved_holes() {
                        circle(&mut text, &layer, (hole.x, hole.y), radius);
                    }
                }
            }
            OperationKind::Face => {}
        }
    }
    text.push_str("0\nENDSEC\n0\nEOF\n");
    text
}

fn outline(text: &mut String, layer: &str, operation: &Operation) {
    if operation.shape.as_deref() == Some("circle") {
        if operation.width > 0.0 && operation.height > 0.0 {
            circle(
                text,
                layer,
                (operation.x + operation.width / 2.0, operation.y + operation.height / 2.0),
                operation.width / 2.0,
            );
        }
        return;
    }
    rectangle(text, layer, operation.x, operation.y, operation.width, operation.height);
}

fn rectangle(text: &mut String, layer: &str, x: f64, y: f64, width: f64, height: f64) {
    if width > 0.0 && height > 0.0 {
        polyline(text, layer, &[(x, y), (x + width, y), (x + width, y + height), (x, y + height)], true);
    }
}

fn polyline(text: &mut String, layer: &str, points: &[(f64, f64)], closed: bool) {
    text.push_str(&format!("0\nLWPOLYLINE\n8\n{layer}\n90\n{}\n70\n{}\n", points.len(), u8::from(closed)));
    for &(x, y) in points {
        text.push_str(&format!("10\n{}\n20\n{}\n", number(x), number(y)));
    }
}

fn line(text: &mut String, layer: &str, start: (f64, f64), end: (f64, f64)) {
    text.push_str(&format!(
        "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
        number(start.0),
        number(start.1),
        number(end.0),
        number(end.1)
    ));
}

fn circle(text: &mut String, layer: &str, centre: (f64, f64), radius: f64) {
    text.push_str(&format!(
        "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n40\n{}\n",
        number(centre.0),
        number(centre.1),
        number(radius)
    ));
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}

fn layer_name(id: &str) -> String {
    let mut layer = String::with_capacity(id.len());
    for character in id.chars() {
        layer.push(if character.is_ascii_alphanumeric() || character == '-' || character == '_' { character } else { '-' });
    }
    if layer.is_empty() { "OPERATION".to_string() } else { layer }
}

fn read_groups(text: &str) -> Result<Vec<Group>, CamError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut groups = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index].trim();
        if raw.is_empty() {
            return Err(malformed_group(index + 1, "a group code is missing (blank line)"));
        }
        let code = raw.parse::<i32>().map_err(|_| malformed_group(index + 1, format!("`{raw}` is not a DXF group code")))?;
        let Some(value) = lines.get(index + 1)
        else {
            return Err(malformed_group(index + 1, "the file ends before the group value"));
        };
        groups.push(Group { code, value: value.trim().to_string(), line: index + 1 });
        index += 2;
    }
    Ok(groups)
}

fn read_units(groups: &[Group]) -> UnitsInfo {
    for (index, group) in groups.iter().enumerate() {
        if group.code == 9 && group.value == "$INSUNITS" {
            let value = groups.get(index + 1).filter(|next| next.code == 70).and_then(|next| next.value.parse::<u32>().ok());
            return UnitsInfo { value, line: Some(group.line) };
        }
    }
    UnitsInfo::default()
}

fn unit_note(units: &UnitsInfo, losses: &mut Vec<DxfLoss>) -> String {
    match units.value {
        Some(0) => "unitless".to_string(),
        Some(4) => "millimetres".to_string(),
        value => {
            let line = units.line.unwrap_or(0);
            let declared = match value {
                Some(number) => format!("$INSUNITS {number}"),
                None => "$INSUNITS".to_string(),
            };
            losses.push(DxfLoss {
                code: "import/dxf-unsupported-units".to_string(),
                entity: "HEADER".to_string(),
                layer: String::new(),
                line,
                message: format!("{declared} is not supported; coordinates are read as millimetres"),
            });
            "millimetres".to_string()
        }
    }
}

fn scan_sections(groups: &[Group], losses: &mut Vec<DxfLoss>) -> Result<Vec<RawEntity>, CamError> {
    let mut entities = Vec::new();
    let mut index = 0;
    while index < groups.len() {
        let group = &groups[index];
        if group.code == 0 && group.value == "EOF" {
            break;
        }
        if group.code != 0 || group.value != "SECTION" {
            index += 1;
            continue;
        }
        let Some(name_group) = groups.get(index + 1).filter(|next| next.code == 2)
        else {
            return Err(malformed_group(group.line, "a SECTION is not followed by a group 2 name"));
        };
        let name = name_group.value.as_str();
        let end = groups[index + 2..]
            .iter()
            .position(|next| next.code == 0 && next.value == "ENDSEC")
            .map(|offset| index + 2 + offset)
            .ok_or_else(|| malformed_group(group.line, format!("section `{name}` has no ENDSEC")))?;
        match name {
            "HEADER" => {}
            "ENTITIES" => entities.extend(split_entities(&groups[index + 2..end])),
            other => losses.push(DxfLoss {
                code: "import/dxf-unsupported-section".to_string(),
                entity: "SECTION".to_string(),
                layer: String::new(),
                line: group.line,
                message: format!("section `{other}` is ignored"),
            }),
        }
        index = end + 1;
    }
    Ok(entities)
}

fn split_entities(groups: &[Group]) -> Vec<RawEntity> {
    let mut entities = Vec::new();
    let mut current: Option<RawEntity> = None;
    for group in groups {
        if group.code == 0 {
            if let Some(entity) = current.take() {
                entities.push(entity);
            }
            current = Some(RawEntity { name: group.value.clone(), line: group.line, groups: Vec::new() });
        }
        else if let Some(entity) = current.as_mut() {
            entity.groups.push(group.clone());
        }
    }
    if let Some(entity) = current {
        entities.push(entity);
    }
    entities
}

fn parse_line(raw: &RawEntity) -> Result<Option<ParsedEntity>, CamError> {
    let start = (required_number(raw, 10)?, required_number(raw, 20)?);
    let end = (required_number(raw, 11)?, required_number(raw, 21)?);
    Ok(finish(raw, vec![start, end], false))
}

fn parse_lwpolyline(raw: &RawEntity, losses: &mut Vec<DxfLoss>) -> Result<Option<ParsedEntity>, CamError> {
    let flags = optional_number(raw, 70)?.unwrap_or(0.0) as i64;
    let mut vertices: Vec<(Option<f64>, Option<f64>)> = Vec::new();
    let mut bulged = false;
    for group in &raw.groups {
        match group.code {
            10 => vertices.push((Some(parse_number(group, "LWPOLYLINE vertex X")?), None)),
            20 => {
                let Some(vertex) = vertices.last_mut()
                else {
                    return Err(malformed_entity(group.line, "LWPOLYLINE has a Y vertex without an X vertex"));
                };
                vertex.1 = Some(parse_number(group, "LWPOLYLINE vertex Y")?);
            }
            42 if parse_number(group, "LWPOLYLINE bulge")?.abs() > MERGE_EPSILON => bulged = true,
            _ => {}
        }
    }
    if bulged {
        losses.push(DxfLoss {
            code: "import/dxf-bulge-ignored".to_string(),
            entity: "LWPOLYLINE".to_string(),
            layer: raw_layer(raw),
            line: raw.line,
            message: "bulge arcs are ignored; the affected segment is imported as a straight line".to_string(),
        });
    }
    let mut points = Vec::with_capacity(vertices.len());
    for (index, vertex) in vertices.iter().enumerate() {
        let Some(x) = vertex.0
        else {
            return Err(malformed_entity(raw.line, format!("LWPOLYLINE vertex {index} has no X (group 10)")));
        };
        let Some(y) = vertex.1
        else {
            return Err(malformed_entity(raw.line, format!("LWPOLYLINE vertex {index} has no Y (group 20)")));
        };
        points.push((x, y));
    }
    Ok(finish(raw, points, flags & 1 == 1))
}

fn parse_polyline(entities: &[RawEntity], start: usize) -> Result<(Option<ParsedEntity>, usize), CamError> {
    let raw = &entities[start];
    let flags = optional_number(raw, 70)?.unwrap_or(0.0) as i64;
    let mut points = Vec::new();
    let mut index = start + 1;
    while index < entities.len() && entities[index].name == "VERTEX" {
        let vertex = &entities[index];
        points.push((required_number(vertex, 10)?, required_number(vertex, 20)?));
        index += 1;
    }
    if index < entities.len() && entities[index].name == "SEQEND" {
        index += 1;
    }
    Ok((finish(raw, points, flags & 1 == 1), index))
}

fn parse_arc(raw: &RawEntity, tolerance: f64) -> Result<Option<ParsedEntity>, CamError> {
    let center = (required_number(raw, 10)?, required_number(raw, 20)?);
    let radius = required_number(raw, 40)?;
    if radius <= 0.0 {
        return Err(malformed_entity(raw.line, "ARC radius must be greater than zero"));
    }
    let start = optional_number(raw, 50)?.unwrap_or(0.0);
    let end = optional_number(raw, 51)?.unwrap_or(0.0);
    let sweep = arc_sweep(start, end);
    let segments = arc_segments(radius, sweep, tolerance);
    let step = sweep / segments as f64;
    let points = (0..=segments)
        .map(|index| {
            let angle = start.to_radians() + step * index as f64;
            (center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
        })
        .collect();
    Ok(finish(raw, points, false))
}

fn parse_circle(raw: &RawEntity, tolerance: f64) -> Result<Option<ParsedEntity>, CamError> {
    let center = (required_number(raw, 10)?, required_number(raw, 20)?);
    let radius = required_number(raw, 40)?;
    if radius <= 0.0 {
        return Err(malformed_entity(raw.line, "CIRCLE radius must be greater than zero"));
    }
    let segments = arc_segments(radius, TAU, tolerance).max(MIN_CIRCLE_SEGMENTS);
    let step = TAU / segments as f64;
    let points = (0..segments)
        .map(|index| {
            let angle = step * index as f64;
            (center.0 + radius * angle.cos(), center.1 + radius * angle.sin())
        })
        .collect();
    Ok(finish(raw, points, true))
}

fn finish(raw: &RawEntity, points: Vec<(f64, f64)>, closed: bool) -> Option<ParsedEntity> {
    let mut points = remove_adjacent_duplicates(points);
    if closed && points.len() > 2 && coincident(points[0], *points.last().expect("non-empty")) {
        points.pop();
    }
    if points.len() < 2 {
        return None;
    }
    Some(ParsedEntity { layer: raw_layer(raw), points, closed })
}

fn build_document(parsed: &[ParsedEntity], request: &DxfImportRequest) -> CamDocument {
    if parsed.is_empty() {
        return CamDocument::new(DEFAULT_STOCK);
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for entity in parsed {
        for &(x, y) in &entity.points {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let width = (max_x - min_x).max(1.0);
    let depth = (max_y - min_y).max(1.0);
    let height = depth.max(request.depth) + STOCK_MARGIN;
    let mut document = CamDocument::new(Stock::new(width, depth, height));
    document.tools.push(engrave_tool());
    for (index, entity) in parsed.iter().enumerate() {
        let mut operation = Operation::new(
            format!("{}-{}", slug(&entity.layer), index + 1),
            OperationKind::Engrave,
            DEFAULT_ENGRAVE_TOOL_ID,
            request.depth,
            request.stepdown,
        );
        operation.name = entity.layer.clone();
        operation.closed = entity.closed;
        operation.path = entity.points.iter().map(|&(x, y)| PathPoint::new(clean(x - min_x), clean(y - min_y))).collect();
        document.operations.push(operation);
    }
    document
}

pub(crate) fn engrave_tool() -> Tool {
    Tool {
        id: DEFAULT_ENGRAVE_TOOL_ID.to_string(),
        name: "engrave".to_string(),
        number: 1,
        diameter: 0.2,
        flutes: 1,
        spindle: 12000.0,
        feed: 300.0,
        plunge_feed: 100.0,
    }
}

fn arc_sweep(start: f64, end: f64) -> f64 {
    let mut sweep = (end - start) % 360.0;
    if sweep < 0.0 {
        sweep += 360.0;
    }
    if sweep.abs() <= 1e-12 {
        sweep = 360.0;
    }
    sweep.to_radians()
}

pub(crate) fn arc_segments(radius: f64, sweep: f64, tolerance: f64) -> usize {
    let ratio = (tolerance / radius).clamp(0.0, 1.0);
    let step = if ratio >= 1.0 { PI } else { 2.0 * (1.0 - ratio).acos() };
    let count = (sweep / step.max(1e-9)).ceil();
    (count as usize).clamp(1, MAX_ARC_SEGMENTS)
}

pub(crate) fn remove_adjacent_duplicates(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut result: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for point in points {
        let duplicate = match result.last() {
            Some(last) => coincident(*last, point),
            None => false,
        };
        if !duplicate {
            result.push(point);
        }
    }
    result
}

pub(crate) fn coincident(left: (f64, f64), right: (f64, f64)) -> bool {
    let dx = right.0 - left.0;
    let dy = right.1 - left.1;
    dx * dx + dy * dy <= MERGE_EPSILON * MERGE_EPSILON
}

pub(crate) fn clean(value: f64) -> f64 {
    value + 0.0
}

fn slug(layer: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for character in layer.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            dash = false;
        }
        else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { "layer".to_string() } else { trimmed.to_string() }
}

fn raw_layer(raw: &RawEntity) -> String {
    raw.groups
        .iter()
        .find(|group| group.code == 8)
        .map(|group| group.value.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "0".to_string())
}

fn required_number(raw: &RawEntity, code: i32) -> Result<f64, CamError> {
    let group = raw
        .groups
        .iter()
        .find(|group| group.code == code)
        .ok_or_else(|| malformed_entity(raw.line, format!("{} is missing group {}", raw.name, code)))?;
    parse_number(group, &format!("{} group {}", raw.name, code))
}

fn optional_number(raw: &RawEntity, code: i32) -> Result<Option<f64>, CamError> {
    raw.groups
        .iter()
        .find(|group| group.code == code)
        .map(|group| parse_number(group, &format!("{} group {}", raw.name, code)))
        .transpose()
}

fn parse_number(group: &Group, what: &str) -> Result<f64, CamError> {
    let value = group
        .value
        .parse::<f64>()
        .map_err(|_| malformed_entity(group.line, format!("{what} is not a number: `{}`", group.value)))?;
    if !value.is_finite() {
        return Err(malformed_entity(group.line, format!("{what} is not finite")));
    }
    Ok(value)
}

fn malformed_group(line: usize, message: impl AsRef<str>) -> CamError {
    CamError::new("import/dxf-malformed-group", format!("line {line}: {}", message.as_ref()))
}

fn malformed_entity(line: usize, message: impl AsRef<str>) -> CamError {
    CamError::new("import/dxf-malformed-entity", format!("line {line}: {}", message.as_ref()))
}

fn empty_loss(raw: &RawEntity) -> DxfLoss {
    DxfLoss {
        code: "import/dxf-empty-entity".to_string(),
        entity: raw.name.clone(),
        layer: raw_layer(raw),
        line: raw.line,
        message: format!("{} has no usable 2D geometry and was skipped", raw.name),
    }
}
