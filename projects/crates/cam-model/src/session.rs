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
    estimate::{self, CycleTimeReport, EstimateRequest},
    export::{markdown, segments_csv, write_atomic},
    gcode,
    model::{CamDocument, Operation, Stock, Tool, UNITS},
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
}

impl ExportFormat {
    /// Returns the short identifier used by the CLI and by reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gcode => "gcode",
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Md => "md",
        }
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
            "step" | "stp" | "dxf" => Err(crate::not_implemented(&format!("`{value}` export"))),
            other => Err(CamError::new(
                "cli/unknown-format",
                format!("unknown export format `{other}`; only gcode, json, csv or md is supported"),
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
    pub fn remove_operation(&mut self, id: &str) -> Result<DocumentSummary, CamError> {
        let mut document = self.document()?.clone();
        if !document.operations.iter().any(|operation| operation.id == id) {
            return Err(CamError::new("operation/unknown-id", format!("operation `{id}` does not exist")));
        }
        document.operations.retain(|operation| operation.id != id);
        self.persist(document)
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

    /// Exports the open document and returns the created files.
    pub fn export(&self, format: ExportFormat, target: &Path) -> Result<ExportReport, CamError> {
        let document = self.document()?;
        let report = toolpath::generate(document)?;
        let text = match format {
            ExportFormat::Gcode => gcode::render(document)?,
            ExportFormat::Json => {
                serde_json::to_string_pretty(&report).map_err(|error| CamError::new("document/serialize", error.to_string()))?
            }
            ExportFormat::Csv => segments_csv(&report),
            ExportFormat::Md => markdown(document, &report),
        };
        write_atomic(target, &text)?;
        Ok(ExportReport {
            format: format.as_str().to_string(),
            path: target.display().to_string(),
            files: vec![target.display().to_string()],
            bytes: text.len(),
        })
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
        let id = path.file_stem().and_then(|value| value.to_str()).unwrap_or("document").to_string();
        Ok(DocumentSummary {
            id,
            path: path.display().to_string(),
            units: document.units.clone(),
            revision: document.revision,
            stock: document.stock,
            tools: document.tools.len(),
            operations: document.operations.len(),
        })
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
