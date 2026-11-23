//! Portable tool libraries and tool-table reports.
//!
//! The tool table of a document is document-local. A library file is one
//! `open-cam-tools` JSON object that a document can write and another document
//! can merge, so a shop keeps one tool list and reuses it across jobs. The
//! tool-table report lists every declared tool together with the operations that
//! reference it, so an unused or missing tool is visible without opening the
//! document by hand.

use serde::{Deserialize, Serialize};

use crate::{
    CamError,
    model::{CamDocument, Tool, UNITS},
    validate,
};

/// Format identifier written to every tool library file.
pub const LIBRARY_FORMAT: &str = "open-cam-tools";

/// Schema version written to every tool library file.
pub const LIBRARY_VERSION: u32 = 1;

/// One portable tool library file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLibrary {
    /// Format identifier; always [`LIBRARY_FORMAT`] for a library this crate wrote.
    pub format: String,
    /// Schema version; always [`LIBRARY_VERSION`] for a library this crate wrote.
    pub version: u32,
    /// Unit system of the tool feeds; must be [`UNITS`].
    pub units: String,
    /// Declared tools, in file order.
    pub tools: Vec<Tool>,
}

impl ToolLibrary {
    /// Creates a library from a list of tools.
    pub fn new(tools: Vec<Tool>) -> Self {
        Self { format: LIBRARY_FORMAT.to_string(), version: LIBRARY_VERSION, units: UNITS.to_string(), tools }
    }

    /// Parses and validates a library file.
    ///
    /// A file that is not `open-cam-tools` version 1 in millimetres reports
    /// `tool/library-invalid`; every malformed tool reports `tool/invalid` and a
    /// tool id declared twice reports `tool/duplicate-id`.
    pub fn parse(text: &str) -> Result<Self, CamError> {
        let library: Self = serde_json::from_str(text)
            .map_err(|error| CamError::new("tool/library-invalid", format!("cannot parse the tool library: {error}")))?;
        if library.format != LIBRARY_FORMAT {
            return Err(CamError::new(
                "tool/library-invalid",
                format!("format is `{}` but `{LIBRARY_FORMAT}` is required", library.format),
            ));
        }
        if library.version != LIBRARY_VERSION {
            return Err(CamError::new(
                "tool/library-invalid",
                format!("version is {} but {LIBRARY_VERSION} is required", library.version),
            ));
        }
        if library.units != UNITS {
            return Err(CamError::new(
                "tool/library-invalid",
                format!("units are `{}` but only `{UNITS}` is supported", library.units),
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for tool in &library.tools {
            validate::check_tool(tool)?;
            if !ids.insert(tool.id.as_str()) {
                return Err(CamError::new("tool/duplicate-id", format!("tool `{}` is declared twice in the library", tool.id)));
            }
        }
        Ok(library)
    }

    /// Renders the library as deterministic pretty-printed JSON.
    pub fn render(&self) -> Result<String, CamError> {
        serde_json::to_string_pretty(self).map_err(|error| CamError::new("document/serialize", error.to_string()))
    }
}

/// One declared tool with the operations that reference it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    /// The declared tool itself.
    #[serde(flatten)]
    pub tool: Tool,
    /// Identifiers of the operations that use this tool, in document order.
    pub used_by: Vec<String>,
    /// `true` when at least one operation references the tool.
    pub used: bool,
}

/// Tool table of one document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTableReport {
    /// Report format identifier, always `open-cam-tool-table`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system of the document.
    pub units: String,
    /// Number of declared tools.
    pub tool_count: usize,
    /// Number of tools no operation references.
    pub unused_count: usize,
    /// One entry per declared tool, in document order.
    pub tools: Vec<ToolEntry>,
}

/// Builds the tool table report of a document.
pub fn tool_table(document: &CamDocument) -> ToolTableReport {
    let tools: Vec<ToolEntry> = document
        .tools
        .iter()
        .map(|tool| {
            let used_by: Vec<String> = document
                .operations
                .iter()
                .filter(|operation| operation.tool == tool.id)
                .map(|operation| operation.id.clone())
                .collect();
            ToolEntry { tool: tool.clone(), used: !used_by.is_empty(), used_by }
        })
        .collect();
    let unused_count = tools.iter().filter(|entry| !entry.used).count();
    ToolTableReport {
        format: "open-cam-tool-table".to_string(),
        version: 1,
        units: document.units.clone(),
        tool_count: tools.len(),
        unused_count,
        tools,
    }
}
