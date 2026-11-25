//! Path-backed editing session.
//!
//! [`OpenCam`] owns at most one document. Every editing call validates the
//! candidate document, writes it atomically and returns a fresh summary, so the
//! CLI, the native addon and the TypeScript SDK cannot drift apart. Argument
//! parsing, help text and exit codes stay outside this module.

use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Serialize;

use crate::{
    CAPABILITIES_JSON, CamError,
    dxf::{self, DXF_IMPORT_FORMAT, DXF_IMPORT_VERSION, DxfImport, DxfImportReport, DxfImportRequest},
    estimate::{self, CycleTimeReport, EstimateRequest},
    export::{markdown, segments_csv, write_atomic},
    feeds::{self, FeedsReport, FeedsRequest},
    gcode::{self, PostOptions},
    library::{self, ToolLibrary, ToolTableReport},
    model::{CamDocument, Operation, Stock, Tool, UNITS},
    operation_delete,
    operation_duplicate::{self, OperationDuplicate},
    operation_list::{self, OperationListReport},
    operation_reorder::{self, OperationReorder, OperationReorderReport},
    operation_update::{self, OperationUpdate},
    sequence::{self, SequenceReport},
    toolpath::{self, ToolpathReport},
    validate::{self, ValidationReport},
};

/// Stock used by [`OpenCam::create`] when the caller sets none.
pub const DEFAULT_STOCK: Stock = Stock { width: 100.0, depth: 100.0, height: 10.0 };

/// Summary of the document a session currently owns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    /// Opaque document identity, derived from the file stem.
    pub id: String,
    /// Path of the document file.
    pub path: String,
    /// Declared unit system.
    pub units: String,
    /// Monotonically increasing edit counter.
    pub revision: u32,
    /// Current stock block.
    pub stock: Stock,
    /// Number of tools.
    pub tools: usize,
    /// Number of operations.
    pub operations: usize,
}

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// A G-code program.
    Gcode,
    /// A JSON toolpath report.
    Json,
    /// One CSV row per move.
    Csv,
    /// A Markdown toolpath report.
    Md,
    /// A vector SVG toolpath drawing.
    Svg,
    /// A one-page PDF toolpath drawing.
    Pdf,
    /// An ASCII DXF drawing of the document geometry.
    Dxf,
}

impl ExportFormat {
    /// Returns the short identifier used by the CLI and by reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gcode => "gcode",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Md => "md",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Dxf => "dxf",
        }
    }

    /// Returns `true` for the vector drawing formats.
    pub const fn is_drawing(self) -> bool {
        matches!(self, Self::Svg | Self::Pdf)
    }
}

impl FromStr for ExportFormat {
    type Err = CamError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "gcode" | "nc" | "tap" => Ok(Self::Gcode),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "md" | "markdown" => Ok(Self::Md),
            "svg" => Ok(Self::Svg),
            "pdf" => Ok(Self::Pdf),
            "dxf" => Ok(Self::Dxf),
            "step" | "stp" => Err(crate::not_implemented(&format!("`{value}` export"))),
            other => Err(CamError::new(
                "cli/unknown-format",
                format!("unknown export format `{other}`; only gcode, json, csv, md, svg, pdf or dxf is supported"),
            )),
        }
    }
}

/// Report returned by [`OpenCam::export`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// Format that was written.
    pub format: String,
    /// Target path exactly as the caller supplied it.
    pub path: String,
    /// Concrete files created by the export.
    pub files: Vec<String>,
    /// Number of bytes written.
    pub bytes: usize,
    /// Non-ASCII characters the format replaced with `?`; `0` for every
    /// format except PDF.
    pub dropped: usize,
}

/// Report returned by [`OpenCam::drawing`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawingReport {
    /// Drawing format, `svg` or `pdf`.
    pub format: String,
    /// Rendered sheet text, ready to write unchanged.
    pub text: String,
    /// Non-ASCII characters replaced by `?`; `0` for SVG.
    pub dropped: usize,
}

/// Report returned by [`OpenCam::import_tool_library`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolImportReport {
    /// Report format identifier, always `open-cam-tool-import`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Library file that was read, exactly as the caller supplied it.
    pub path: String,
    /// Tool ids that were added, in library order.
    pub imported: Vec<String>,
    /// Number of tools the document declares after the import.
    pub tool_count: usize,
}

/// Full report returned by [`OpenCam::report`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Report format identifier.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Document summary.
    pub document: DocumentSummary,
    /// Validation findings.
    pub validation: ValidationReport,
    /// Generated toolpaths.
    pub toolpaths: ToolpathReport,
    /// Number of lines in the rendered G-code program.
    pub gcode_lines: usize,
}

/// One document session.
#[derive(Debug, Default)]
pub struct OpenCam {
    path: Option<PathBuf>,
    document: Option<CamDocument>,
}

impl OpenCam {
    /// Creates an empty session. Constructing a session opens no file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the stable capability matrix of this build.
    pub fn capabilities(&self) -> &'static str {
        CAPABILITIES_JSON
    }

    /// Suggests spindle and feed rates for one material and tool diameter.
    ///
    /// The recommendation comes from a fixed per-material surface-speed and
    /// chip-load table and does not read the open document, so it is valid
    /// before a document exists. An unknown material reports
    /// `feeds/unknown-material`; an invalid diameter or flute count reports
    /// `feeds/invalid`.
    pub fn suggest_feeds(&self, request: &FeedsRequest) -> Result<FeedsReport, CamError> {
        feeds::suggest(request)
    }

    /// Creates a document at `path` with the default stock and no tools.
    pub fn create(&mut self, path: &Path) -> Result<DocumentSummary, CamError> {
        if path.exists() {
            return Err(CamError::new("file/exists", format!("`{}` already exists", path.display())));
        }
        let document = CamDocument::new(DEFAULT_STOCK);
        let text =
            serde_json::to_string_pretty(&document).map_err(|error| CamError::new("document/serialize", error.to_string()))?;
        write_atomic(path, &text)?;
        self.path = Some(path.to_path_buf());
        self.document = Some(document);
        self.summary()
    }

    /// Imports an ASCII DXF drawing into a new document at `target`.
    ///
    /// `source` must be an ASCII DXF file. Every supported 2D entity becomes one
    /// `engrave` operation that references a fixed `engrave` tool; arcs and
    /// circles are flattened with the chord tolerance of `request`. The stock
    /// spans the translated drawing bounding box (the minimum X and Y move to
    /// the origin) and is at least `1 × 1` mm, with
    /// `max(bbox depth, request depth) + 5` mm of height. The new document keeps
    /// revision `0`, exactly like [`Self::create`]. An existing `target` reports
    /// `file/exists`; a malformed group or entity reports
    /// `import/dxf-malformed-group` or `import/dxf-malformed-entity` with a line
    /// number, and a rejected candidate is never written.
    pub fn import_dxf(
        &mut self,
        source: &Path,
        target: &Path,
        request: &DxfImportRequest,
    ) -> Result<DxfImportReport, CamError> {
        if target.exists() {
            return Err(CamError::new("file/exists", format!("`{}` already exists", target.display())));
        }
        let text = fs::read_to_string(source)
            .map_err(|error| CamError::new("io/read", format!("cannot read `{}`: {error}", source.display())))?;
        let DxfImport { document, ins_units, source_units, entity_count, skipped_count, losses } = dxf::import(&text, request)?;
        let validation = validate::validate(&document);
        if let Some(error) = validate::first_error(&validation) {
            return Err(error);
        }
        let operation_count = document.operations.len();
        let serialized =
            serde_json::to_string_pretty(&document).map_err(|error| CamError::new("document/serialize", error.to_string()))?;
        write_atomic(target, &serialized)?;
        self.path = Some(target.to_path_buf());
        self.document = Some(document);
        Ok(DxfImportReport {
            format: DXF_IMPORT_FORMAT.to_string(),
            version: DXF_IMPORT_VERSION,
            units: UNITS.to_string(),
            source: source.display().to_string(),
            path: target.display().to_string(),
            ins_units,
            source_units,
            tolerance: request.tolerance,
            depth: request.depth,
            stepdown: request.stepdown,
            entity_count,
            operation_count,
            skipped_count,
            losses,
            document: self.summary()?,
        })
    }

    /// Opens the document at `path`.
    pub fn open(&mut self, path: &Path) -> Result<DocumentSummary, CamError> {
        let text = fs::read_to_string(path)
            .map_err(|error| CamError::new("io/read", format!("cannot read `{}`: {error}", path.display())))?;
        let document: CamDocument = serde_json::from_str(&text)
            .map_err(|error| CamError::new("document/parse", format!("cannot parse `{}`: {error}", path.display())))?;
        self.path = Some(path.to_path_buf());
        self.document = Some(document);
        self.summary()
    }

    /// Writes the open document in place, or to `target` when supplied.
    pub fn save(&mut self, target: Option<&Path>) -> Result<DocumentSummary, CamError> {
        let document = self.document()?.clone();
        let target = target.map(Path::to_path_buf).or_else(|| self.path.clone()).ok_or_else(no_document)?;
        let text =
            serde_json::to_string_pretty(&document).map_err(|error| CamError::new("document/serialize", error.to_string()))?;
        write_atomic(&target, &text)?;
        self.path = Some(target);
        self.summary()
    }

    /// Replaces the stock block and saves the document.
    pub fn set_stock(&mut self, stock: Stock) -> Result<DocumentSummary, CamError> {
        let mut document = self.document()?.clone();
        document.stock = stock;
        self.persist(document)
    }

    /// Adds a tool and saves the document.
    pub fn add_tool(&mut self, tool: Tool) -> Result<DocumentSummary, CamError> {
        let mut document = self.document()?.clone();
        if document.tools.iter().any(|entry| entry.id == tool.id) {
            return Err(CamError::new("tool/duplicate-id", format!("tool `{}` already exists", tool.id)));
        }
        validate::check_tool(&tool)?;
        document.tools.push(tool);
        self.persist(document)
    }

    /// Removes an unused tool and saves the document.
    pub fn remove_tool(&mut self, id: &str) -> Result<DocumentSummary, CamError> {
        let mut document = self.document()?.clone();
        if !document.tools.iter().any(|tool| tool.id == id) {
            return Err(CamError::new("tool/unknown-id", format!("tool `{id}` does not exist")));
        }
        if let Some(operation) = document.operations.iter().find(|operation| operation.tool == id) {
            return Err(CamError::new("tool/in-use", format!("tool `{id}` is used by operation `{}`", operation.id)));
        }
        document.tools.retain(|tool| tool.id != id);
        self.persist(document)
    }

    /// Returns every declared tool with the operations that use it.
    pub fn tool_table(&self) -> Result<ToolTableReport, CamError> {
        Ok(library::tool_table(self.document()?))
    }

    /// Writes the document tool table as a portable `open-cam-tools` library.
    pub fn write_tool_library(&self, target: &Path) -> Result<ExportReport, CamError> {
        let text = ToolLibrary::new(self.document()?.tools.clone()).render()?;
        write_atomic(target, &text)?;
        Ok(ExportReport {
            format: "tools".to_string(),
            path: target.display().to_string(),
            files: vec![target.display().to_string()],
            bytes: text.len(),
            dropped: 0,
        })
    }

    /// Merges a portable tool library into the document and saves it.
    ///
    /// The library is parsed and validated first, so a malformed file or a tool
    /// id that is already declared leaves the document untouched. The first
    /// error is returned unchanged.
    pub fn import_tool_library(&mut self, source: &Path) -> Result<ToolImportReport, CamError> {
        let text = fs::read_to_string(source)
            .map_err(|error| CamError::new("io/read", format!("cannot read `{}`: {error}", source.display())))?;
        let library = ToolLibrary::parse(&text)?;
        let mut document = self.document()?.clone();
        if let Some(tool) = library.tools.iter().find(|tool| document.tools.iter().any(|entry| entry.id == tool.id)) {
            return Err(CamError::new(
                "tool/duplicate-id",
                format!("tool `{}` from the library is already declared in this document", tool.id),
            ));
        }
        let imported = library.tools.iter().map(|tool| tool.id.clone()).collect();
        document.tools.extend(library.tools);
        let summary = self.persist(document)?;
        Ok(ToolImportReport {
            format: "open-cam-tool-import".to_string(),
            version: 1,
            path: source.display().to_string(),
            imported,
            tool_count: summary.tools,
        })
    }

    /// Adds an operation and saves the document.
    pub fn add_operation(&mut self, operation: Operation) -> Result<DocumentSummary, CamError> {
        let mut document = self.document()?.clone();
        if document.operations.iter().any(|entry| entry.id == operation.id) {
            return Err(CamError::new("operation/duplicate-id", format!("operation `{}` already exists", operation.id)));
        }
        validate::check_operation(&operation, &document)?;
        document.operations.push(operation);
        self.persist(document)
    }

    /// Removes an operation and saves the document.
    ///
    /// The alias kept for callers of the older `operation remove` name; it is
    /// the same delete path as [`Self::delete_operation`].
    pub fn remove_operation(&mut self, id: &str) -> Result<DocumentSummary, CamError> {
        self.delete_operation(id)
    }

    /// Removes one operation and saves the document.
    ///
    /// An operation is self-contained, so deleting one never leaves a dangling
    /// reference in the tool table, tabs, islands or the derived sequence plan.
    /// The candidate document is validated with the same rules as `add`, `update`
    /// and `save` before it is written, so a rejected delete leaves the document
    /// and its revision untouched. Removing the last operation is allowed.
    pub fn delete_operation(&mut self, id: &str) -> Result<DocumentSummary, CamError> {
        let document = self.document()?.clone();
        let candidate = operation_delete::delete(&document, id)?;
        self.persist(candidate)
    }

    /// Copies one operation, inserts the copy after it and saves the document.
    ///
    /// The copy keeps every field of its source and only the identifier changes.
    /// A missing id takes the smallest free `op-<n>` identifier; a rejected
    /// duplicate leaves the document and its revision untouched.
    pub fn duplicate_operation(&mut self, request: &OperationDuplicate) -> Result<DocumentSummary, CamError> {
        let document = self.document()?.clone();
        let candidate = operation_duplicate::duplicate(&document, request)?;
        self.persist(candidate)
    }

    /// Moves one operation to a final 0-based position and saves the document.
    ///
    /// `to` is the position the operation occupies after the move. An unknown id
    /// reports `operation/unknown-id` and an index at or beyond the operation
    /// count reports `operation/invalid`. Moving an operation to its current
    /// index is a no-op: the file is not rewritten, the revision stays the same
    /// and the report carries `changed: false`. A real move keeps every field of
    /// every operation, validates the whole candidate document with the same
    /// rules as `add`, `update` and `delete`, and saves it atomically; on any
    /// failure nothing is written and the revision stays the same.
    pub fn reorder_operation(&mut self, request: &OperationReorder) -> Result<OperationReorderReport, CamError> {
        let document = self.document()?.clone();
        let (candidate, mut report) = operation_reorder::reorder(&document, request)?;
        if !report.changed {
            return Ok(report);
        }
        let summary = self.persist(candidate)?;
        report.revision = summary.revision;
        Ok(report)
    }

    /// Lists every operation of the open document without changing it.
    ///
    /// The report is read-only and deterministic; it carries the operation
    /// fields in document order and the kinematic motion time of each operation.
    pub fn operation_list(&self) -> Result<OperationListReport, CamError> {
        operation_list::list(self.document()?)
    }

    /// Applies a partial update to one operation and saves the document.
    ///
    /// Only the supplied fields change. The whole candidate document is
    /// validated with the same rules as `add` and `save` before it is written,
    /// so a rejected update leaves the document and its revision untouched.
    pub fn update_operation(&mut self, request: &OperationUpdate) -> Result<DocumentSummary, CamError> {
        let document = self.document()?.clone();
        let candidate = operation_update::update(&document, request)?;
        self.persist(candidate)
    }

    /// Validates the open document.
    pub fn validate(&self) -> Result<ValidationReport, CamError> {
        Ok(validate::validate(self.document()?))
    }

    /// Generates the toolpath report of the open document.
    pub fn toolpaths(&self) -> Result<ToolpathReport, CamError> {
        toolpath::generate(self.document()?)
    }

    /// Estimates the cycle time of the open document.
    pub fn estimate(&self, request: &EstimateRequest) -> Result<CycleTimeReport, CamError> {
        estimate::estimate(self.document()?, request)
    }

    /// Renders the G-code program of the open document.
    pub fn gcode(&self) -> Result<String, CamError> {
        gcode::render(self.document()?)
    }

    /// Renders the G-code program of the open document with one post option set.
    ///
    /// The options change only the program text (program number, line numbers,
    /// coolant); the moves, feeds and cycles stay exactly the same as
    /// [`Self::gcode`]. Invalid options report `post/invalid`.
    pub fn gcode_with_post(&self, post: &PostOptions) -> Result<String, CamError> {
        gcode::render_with_post(self.document()?, post)
    }

    /// Returns the tool-grouped plan of the open document without changing it.
    ///
    /// The plan keeps every operation of one tool together, in first-appearance
    /// tool order, and keeps the operations inside a tool in document order. It
    /// reports the tool changes of the current order and of the proposed one.
    pub fn sequence(&self) -> Result<SequenceReport, CamError> {
        let document = self.document()?;
        Ok(sequence::plan(document, document.revision))
    }

    /// Reorders the open document by tool and saves it.
    ///
    /// An already grouped document is not rewritten and reports
    /// `applied: false`. Otherwise the grouped document is validated and saved
    /// atomically, and the report carries the saved revision.
    pub fn apply_sequence(&mut self) -> Result<SequenceReport, CamError> {
        let document = self.document()?.clone();
        let mut report = sequence::plan(&document, document.revision);
        if report.unchanged {
            return Ok(report);
        }
        let summary = self.persist(sequence::reorder(&document))?;
        report.applied = true;
        report.revision = summary.revision;
        Ok(report)
    }

    /// Exports the open document and returns the created files.
    pub fn export(&self, format: ExportFormat, target: &Path) -> Result<ExportReport, CamError> {
        let document = self.document()?;
        let report = toolpath::generate(document)?;
        let id = self.id();
        let (text, dropped) = match format {
            ExportFormat::Gcode => (gcode::render(document)?, 0),
            ExportFormat::Json => (
                serde_json::to_string_pretty(&report)
                    .map_err(|error| CamError::new("document/serialize", error.to_string()))?,
                0,
            ),
            ExportFormat::Csv => (segments_csv(&report), 0),
            ExportFormat::Md => (markdown(document, &report), 0),
            ExportFormat::Svg => {
                let drawing = crate::drawing::svg(document, &report, &id);
                (drawing.text, drawing.dropped)
            }
            ExportFormat::Pdf => {
                let drawing = crate::drawing::pdf(document, &report, &id);
                (drawing.text, drawing.dropped)
            }
            ExportFormat::Dxf => (dxf::export(document), 0),
        };
        write_atomic(target, &text)?;
        Ok(ExportReport {
            format: format.as_str().to_string(),
            path: target.display().to_string(),
            files: vec![target.display().to_string()],
            bytes: text.len(),
            dropped,
        })
    }

    /// Renders the open document as a vector drawing without writing it.
    ///
    /// Only the drawing formats [`ExportFormat::Svg`] and [`ExportFormat::Pdf`]
    /// are accepted; the text is returned unchanged so the CLI can send it to
    /// `--out` or stdout.
    pub fn drawing(&self, format: ExportFormat) -> Result<DrawingReport, CamError> {
        if !format.is_drawing() {
            return Err(CamError::new(
                "export/not-drawing",
                format!("`{}` is not a drawing format; use svg or pdf", format.as_str()),
            ));
        }
        let document = self.document()?;
        let report = toolpath::generate(document)?;
        let id = self.id();
        let drawing = match format {
            ExportFormat::Pdf => crate::drawing::pdf(document, &report, &id),
            _ => crate::drawing::svg(document, &report, &id),
        };
        Ok(DrawingReport { format: format.as_str().to_string(), text: drawing.text, dropped: drawing.dropped })
    }

    /// Returns the summary, validation, toolpaths and G-code size of the document.
    pub fn report(&self) -> Result<Report, CamError> {
        let document = self.document()?;
        let summary = self.summary()?;
        let validation = validate::validate(document);
        let toolpaths = toolpath::generate(document)?;
        let gcode_lines = gcode::render(document)?.lines().count();
        Ok(Report {
            format: "open-cam-report".to_string(),
            version: 1,
            units: document.units.clone(),
            document: summary,
            validation,
            toolpaths,
            gcode_lines,
        })
    }

    /// Returns the summary of the open document.
    pub fn summary(&self) -> Result<DocumentSummary, CamError> {
        let document = self.document()?;
        let path = self.path.as_ref().ok_or_else(no_document)?;
        Ok(DocumentSummary {
            id: self.id(),
            path: path.display().to_string(),
            units: document.units.clone(),
            revision: document.revision,
            stock: document.stock,
            tools: document.tools.len(),
            operations: document.operations.len(),
        })
    }

    /// File stem of the open document, used as the drawing id.
    fn id(&self) -> String {
        self.path.as_ref().and_then(|path| path.file_stem()).and_then(|value| value.to_str()).unwrap_or("document").to_string()
    }

    fn document(&self) -> Result<&CamDocument, CamError> {
        self.document.as_ref().ok_or_else(no_document)
    }

    fn persist(&mut self, mut document: CamDocument) -> Result<DocumentSummary, CamError> {
        let report = validate::validate(&document);
        if let Some(error) = validate::first_error(&report) {
            return Err(error);
        }
        let path = self.path.as_ref().ok_or_else(no_document)?.clone();
        document.revision = document.revision.saturating_add(1);
        document.units = UNITS.to_string();
        let text =
            serde_json::to_string_pretty(&document).map_err(|error| CamError::new("document/serialize", error.to_string()))?;
        write_atomic(&path, &text)?;
        self.document = Some(document);
        self.summary()
    }
}

fn no_document() -> CamError {
    CamError::new("session/no-document", "no document is open; call `create` or `open` first")
}
