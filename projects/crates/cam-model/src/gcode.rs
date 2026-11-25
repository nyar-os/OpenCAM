//! G-code writer and importer.
//!
//! The writer emits a single deterministic program per post. Every move comes
//! from [`crate::toolpath`], so the program and the toolpath report cannot
//! disagree. [`PostOptions`] selects a machine dialect and changes only the text
//! around those moves — the header, the program number, line numbers, coolant
//! and the way arcs and canned cycles are spelled — never a coordinate, a feed
//! or a toolpath.
//!
//! The importer reads back the G0/G1/G2/G3 subset those programs use. Cutting
//! moves between two rapids form one maximal run, every run becomes one
//! `engrave` operation and arcs are flattened into chords with the same
//! deterministic sagitta tolerance the DXF reader uses. Lines that could carry
//! geometry the reader cannot honour — canned cycles, radius-form arcs,
//! unknown words — never fail the import: they are reported in the loss list
//! with their 1-based line number. A modal setting that would change how
//! coordinates are interpreted (`G20`, `G91`, `G18`, `G19`, `G92`, `G93`,
//! `G95`, `G96`, `G97` or an unwinding variant) fails the whole import with
//! `import/gcode-unsupported-mode` instead of guessing.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::{
    CamError,
    dxf::{self, DEFAULT_ENGRAVE_TOOL_ID, MIN_CIRCLE_SEGMENTS},
    model::{CamDocument, Operation, OperationKind, PathPoint, Stock, Tool},
    session::{DEFAULT_STOCK, DocumentSummary},
    toolpath::{self, MoveKind, ToolpathReport},
};

/// Post options that change only the text of the program, never its moves.
///
/// Omitted fields keep the defaults, so an empty request renders exactly the
/// generic absolute-millimetre program. `post` selects the dialect: `generic`
/// (default), `grbl`, `mach3` or `fanuc`. `programNumber` emits `O<n>` right
/// after the leading `%`, `lineNumbers` prefixes every interior line with
/// `N<n>` and `coolant` adds `M8` (`flood`) or `M7` (`mist`) after the spindle
/// start of every operation and closes it with `M9`. Units `G21` and absolute
/// mode `G90` stay fixed in every dialect; the writer never emits `G20` or
/// `G91`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostOptions {
    /// Machine dialect: `generic` (default), `grbl`, `mach3` or `fanuc`.
    pub post: String,
    /// Program number emitted as `O<n>` right after the leading `%`; `None`
    /// emits no program-number line. Required by `fanuc`, rejected by `grbl`
    /// and `mach3`.
    pub program_number: Option<u32>,
    /// Prefixes every line between the two `%` lines with `N<n>`.
    pub line_numbers: bool,
    /// Distance between two line numbers; at least one, ten by default.
    pub line_increment: u32,
    /// Coolant mode: `off` (default), `flood` (`M8`) or `mist` (`M7`).
    pub coolant: String,
}

impl Default for PostOptions {
    fn default() -> Self {
        Self {
            post: "generic".to_string(),
            program_number: None,
            line_numbers: false,
            line_increment: 10,
            coolant: "off".to_string(),
        }
    }
}

/// Renders a complete G-code program for a document with the default post.
///
/// The document is validated and its toolpaths generated first; the first
/// validation error is returned unchanged.
pub fn render(document: &CamDocument) -> Result<String, CamError> {
    render_with_post(document, &PostOptions::default())
}

/// Renders a complete G-code program for a document and one set of post options.
///
/// The post options are checked first, so an unknown coolant or a line
/// increment of zero reports `post/invalid`; the document is validated and its
/// toolpaths generated afterwards, so the first document error is returned
/// unchanged. Without options the program is byte-identical to [`render`].
pub fn render_with_post(document: &CamDocument, post: &PostOptions) -> Result<String, CamError> {
    check_post(post)?;
    let report = toolpath::generate(document)?;
    Ok(render_with(document, &report, post))
}

fn check_post(post: &PostOptions) -> Result<(), CamError> {
    if post.line_increment == 0 {
        return Err(CamError::new("post/invalid", "line increment must be at least 1"));
    }
    if post.program_number == Some(0) {
        return Err(CamError::new("post/invalid", "program number must be greater than zero"));
    }
    if !matches!(post.post.as_str(), "generic" | "grbl" | "mach3" | "fanuc") {
        return Err(CamError::new(
            "post/invalid",
            format!("post `{}` is unknown; only `generic`, `grbl`, `mach3` or `fanuc` is supported", post.post),
        ));
    }
    if matches!(post.post.as_str(), "grbl" | "mach3") && post.program_number.is_some() {
        return Err(CamError::new("post/invalid", format!("post `{}` does not support a program number", post.post)));
    }
    if post.post == "grbl" && post.line_numbers {
        return Err(CamError::new("post/invalid", "post `grbl` does not support line numbers"));
    }
    match (post.post.as_str(), post.program_number) {
        ("fanuc", None) => return Err(CamError::new("post/invalid", "post `fanuc` requires a program number")),
        ("fanuc", Some(number)) if number > 9999 => {
            return Err(CamError::new("post/invalid", "a `fanuc` program number must be at most 9999"));
        }
        _ => {}
    }
    if !matches!(post.coolant.as_str(), "off" | "flood" | "mist") {
        return Err(CamError::new(
            "post/invalid",
            format!("coolant `{}` is unknown; only `off`, `flood` or `mist` is supported", post.coolant),
        ));
    }
    Ok(())
}

fn render_with(document: &CamDocument, report: &ToolpathReport, post: &PostOptions) -> String {
    let safe_z = report.safe_z;
    let coolant = coolant_code(&post.coolant);
    let dialect = post.post.as_str();
    let grbl = dialect == "grbl";
    let fanuc = dialect == "fanuc";
    let mut lines = Vec::new();
    if !grbl {
        lines.push("%".to_string());
    }
    if let Some(number) = post.program_number {
        lines.push(if fanuc { format!("O{number:04}") } else { format!("O{number}") });
    }
    lines.push(format!("(OpenCAM {})", env!("CARGO_PKG_VERSION")));
    lines.push(format!("(Units: {})", document.units));
    lines.push(format!("(Post: {})", post_label(dialect)));
    lines.push(header(dialect).to_string());
    lines.push(format!("G0 Z{}", number(safe_z)));

    for (index, operation) in document.operations.iter().enumerate() {
        let Some(path) = report.operations.get(index)
        else {
            continue;
        };
        let tool = document.tools.iter().find(|tool| tool.id == operation.tool);
        lines.push(format!("(operation {} - {})", operation.id, operation.kind.as_str()));
        if operation.kind == OperationKind::Contour {
            if let Some(tabs) = operation.tabs {
                lines.push(format!("(tabs {} x {} mm wide, {} mm high)", tabs.count, number(tabs.width), number(tabs.height)));
            }
        }
        if let Some(tool) = tool {
            if grbl {
                lines.push(format!("(tool {} T{} diameter {})", tool.id, tool.number, number(tool.diameter)));
            }
            else {
                lines.push(format!("T{} M6", tool.number));
            }
            lines.push(format!("S{} M3", number(tool.spindle)));
            if let Some(code) = coolant {
                lines.push(code.to_string());
            }
            if !grbl {
                lines.push(format!("(tool {} diameter {})", tool.id, number(tool.diameter)));
            }
        }
        lines.push(format!("G0 Z{}", number(safe_z)));

        if operation.kind == OperationKind::Drill {
            if grbl {
                grbl_drill(&mut lines, operation, document.stock.height, safe_z, tool);
            }
            else {
                let depth = number(document.stock.height - operation.depth);
                let retract = number(safe_z);
                let feed = number(tool.map_or(0.0, |tool| tool.plunge_feed));
                let prefix = if fanuc { "G98 " } else { "" };
                for hole in operation.resolved_holes() {
                    let x = number(hole.x);
                    let y = number(hole.y);
                    match (operation.peck, operation.dwell) {
                        (Some(peck), _) => {
                            lines.push(format!("{prefix}G83 X{x} Y{y} Z{depth} R{retract} Q{} F{feed}", number(peck)));
                        }
                        (None, Some(dwell)) => {
                            lines.push(format!("{prefix}G82 X{x} Y{y} Z{depth} R{retract} P{} F{feed}", number(dwell)));
                        }
                        (None, None) => lines.push(format!("{prefix}G81 X{x} Y{y} Z{depth} R{retract} F{feed}")),
                    }
                }
                lines.push("G80".to_string());
            }
        }
        else {
            for segment in &path.segments {
                let feed = if segment.kind == MoveKind::Rapid { String::new() } else { format!(" F{}", number(segment.feed)) };
                let (command, arc) = match &segment.arc {
                    Some(arc) => {
                        let centre =
                            format!(" I{} J{}", number(arc.centre.x - segment.from.x), number(arc.centre.y - segment.from.y));
                        (if arc.clockwise { "G2" } else { "G3" }, centre)
                    }
                    None => (if segment.kind == MoveKind::Rapid { "G0" } else { "G1" }, String::new()),
                };
                lines.push(format!(
                    "{command} X{} Y{} Z{}{arc}{feed}",
                    number(segment.to.x),
                    number(segment.to.y),
                    number(segment.to.z)
                ));
            }
        }

        lines.push(format!("G0 Z{}", number(safe_z)));
        if coolant.is_some() {
            lines.push("M9".to_string());
        }
        lines.push("M5".to_string());
    }

    lines.push(format!("G0 Z{}", number(safe_z)));
    lines.push("M30".to_string());
    if !grbl {
        lines.push("%".to_string());
    }
    if post.line_numbers {
        number_lines(&mut lines, post.line_increment);
    }
    lines.join("\n") + "\n"
}

/// Header line of one dialect; units `G21` and absolute mode `G90` never change.
fn header(dialect: &str) -> &'static str {
    match dialect {
        "grbl" => "G21 G90 G17",
        "mach3" | "fanuc" => "G21 G90 G17 G94 G40 G49 G80",
        _ => "G21 G90 G17 G94",
    }
}

/// Human-readable dialect name written in the header comment.
fn post_label(dialect: &str) -> &'static str {
    match dialect {
        "grbl" => "GRBL, absolute, XY plane",
        "mach3" => "Mach3, absolute, XY plane",
        "fanuc" => "Fanuc, absolute, XY plane",
        _ => "generic mill, absolute, XY plane",
    }
}

/// Expands one drill operation into explicit moves for a GRBL controller.
///
/// GRBL has no canned cycles, so every hole becomes a rapid to the hole centre,
/// the plunge (split into peck steps for `G83`), a dwell for `G82` and a rapid
/// back to the clearance plane. The motion is the same the cycle performs.
fn grbl_drill(lines: &mut Vec<String>, operation: &Operation, stock_height: f64, safe_z: f64, tool: Option<&Tool>) {
    let depth = stock_height - operation.depth;
    let feed = number(tool.map_or(0.0, |tool| tool.plunge_feed));
    for hole in operation.resolved_holes() {
        lines.push(format!("G0 X{} Y{} Z{}", number(hole.x), number(hole.y), number(safe_z)));
        match (operation.peck, operation.dwell) {
            (Some(peck), _) => {
                let mut top = safe_z;
                loop {
                    let next = (top - peck).max(depth);
                    lines.push(format!("G1 Z{} F{feed}", number(next)));
                    if next <= depth {
                        break;
                    }
                    lines.push(format!("G0 Z{}", number(safe_z)));
                    lines.push(format!("G0 Z{}", number(next)));
                    top = next;
                }
            }
            (None, Some(dwell)) => {
                lines.push(format!("G1 Z{} F{feed}", number(depth)));
                lines.push(format!("G4 P{}", number(dwell)));
            }
            (None, None) => lines.push(format!("G1 Z{} F{feed}", number(depth))),
        }
        lines.push(format!("G0 Z{}", number(safe_z)));
    }
}

/// Coolant command of one post option set; `None` emits no coolant line.
fn coolant_code(coolant: &str) -> Option<&'static str> {
    match coolant {
        "flood" => Some("M8"),
        "mist" => Some("M7"),
        _ => None,
    }
}

/// Prefixes every line between the two `%` markers with a sequential `N` word.
///
/// The opening and closing `%` stay unnumbered because a controller reads them
/// as the program delimiter, not as a block. Numbering starts at `increment`
/// and rises by `increment`, so the default ten yields `N10`, `N20`, ….
fn number_lines(lines: &mut [String], increment: u32) {
    let last = lines.len().saturating_sub(1);
    for (index, line) in lines.iter_mut().enumerate().take(last).skip(1) {
        let number = (index as u64) * u64::from(increment);
        let original = std::mem::take(line);
        *line = format!("N{number} {original}");
    }
}

fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 1e-9 { "0".to_string() } else { format!("{rounded}") }
}

/// Report format identifier of [`crate::OpenCam::import_gcode`].
pub const GCODE_IMPORT_FORMAT: &str = "open-cam-gcode-import";
/// Report schema version of [`crate::OpenCam::import_gcode`].
pub const GCODE_IMPORT_VERSION: u32 = 1;
/// Cut depth used when the caller supplies none, in millimetres.
pub const DEFAULT_GCODE_DEPTH: f64 = 1.0;
/// Chord sagitta tolerance used when the caller supplies none, in millimetres.
pub const DEFAULT_GCODE_TOLERANCE: f64 = 0.05;

/// Air above the cut floor added to the derived stock height, in millimetres.
const IMPORT_STOCK_MARGIN: f64 = 5.0;

/// Request of [`crate::OpenCam::import_gcode`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GcodeImportRequest {
    /// Cut depth of every imported operation, in millimetres.
    pub depth: f64,
    /// Stepdown per cut, in millimetres.
    pub stepdown: f64,
    /// Maximum chord sagitta when arcs are flattened, in millimetres.
    pub tolerance: f64,
}

impl Default for GcodeImportRequest {
    fn default() -> Self {
        Self { depth: DEFAULT_GCODE_DEPTH, stepdown: DEFAULT_GCODE_DEPTH, tolerance: DEFAULT_GCODE_TOLERANCE }
    }
}

impl GcodeImportRequest {
    /// Builds a request from caller-supplied values.
    #[must_use]
    pub const fn new(depth: f64, stepdown: f64, tolerance: f64) -> Self {
        Self { depth, stepdown, tolerance }
    }

    fn checked(&self) -> Result<Self, CamError> {
        for (name, value) in [("depth", self.depth), ("stepdown", self.stepdown), ("tolerance", self.tolerance)] {
            if !value.is_finite() || value <= 0.0 {
                return Err(CamError::new("import/invalid", format!("{name} must be greater than zero")));
            }
        }
        Ok(*self)
    }
}

/// One piece of imported G-code geometry or semantics that was dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcodeLoss {
    /// Stable machine-readable code, for example `import/gcode-unsupported-gcode`.
    pub code: String,
    /// Word that caused the loss, for example `G81` or `M98`.
    pub word: String,
    /// 1-based line number of the program the word appeared on.
    pub line: usize,
    /// Human-readable description of what was dropped.
    pub message: String,
}

impl GcodeLoss {
    fn new(code: &str, word: String, line: usize, message: String) -> Self {
        Self { code: code.to_string(), word, line, message }
    }
}

/// Result of [`import`]: the rebuilt document and the loss accounting.
#[derive(Debug, Clone, PartialEq)]
pub struct GcodeImport {
    /// Document rebuilt from the cutting runs; always passes [`crate::validate`].
    pub document: CamDocument,
    /// Total number of lines read, including blank and comment lines.
    pub line_count: usize,
    /// Cutting moves converted into path points.
    pub move_count: usize,
    /// Subset of [`Self::move_count`] that were `G2`/`G3` arcs.
    pub arc_count: usize,
    /// Number of reported losses.
    pub skipped_count: usize,
    /// Everything the reader dropped, in program order.
    pub losses: Vec<GcodeLoss>,
}

/// Report returned by [`crate::OpenCam::import_gcode`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcodeImportReport {
    /// Report format identifier, always [`GCODE_IMPORT_FORMAT`].
    pub format: String,
    /// Report schema version, always [`GCODE_IMPORT_VERSION`].
    pub version: u32,
    /// Coordinates are millimetres; `G20` fails the import instead of converting.
    pub units: String,
    /// Source program path as provided by the caller.
    pub source: String,
    /// Target document path as provided by the caller.
    pub path: String,
    /// Chord tolerance the arcs were flattened with, in millimetres.
    pub tolerance: f64,
    /// Cut depth given to every imported operation, in millimetres.
    pub depth: f64,
    /// Stepdown given to every imported operation, in millimetres.
    pub stepdown: f64,
    /// Total number of lines read.
    pub line_count: usize,
    /// Cutting moves converted into path points.
    pub move_count: usize,
    /// Subset of `moveCount` that were `G2`/`G3` arcs.
    pub arc_count: usize,
    /// Number of `engrave` operations written.
    pub operation_count: usize,
    /// Number of reported losses.
    pub skipped_count: usize,
    /// Everything the reader dropped, in program order.
    pub losses: Vec<GcodeLoss>,
    /// Summary of the rebuilt document.
    pub document: DocumentSummary,
}

/// Motion modes of the supported subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveMode {
    Rapid,
    Linear,
    Clockwise,
    CounterClockwise,
}

/// How a G word affects the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GAction {
    /// Accepted modal setting that carries no motion.
    Modal,
    /// One of the four supported motion codes.
    Motion(MoveMode),
    /// Setting that would reinterpret coordinates or feeds: fail the import.
    Mode,
    /// Anything else: skip the line and report a loss.
    Loss,
}

fn gcode_action(value: f64) -> GAction {
    match value {
        0.0 => GAction::Motion(MoveMode::Rapid),
        1.0 => GAction::Motion(MoveMode::Linear),
        2.0 => GAction::Motion(MoveMode::Clockwise),
        3.0 => GAction::Motion(MoveMode::CounterClockwise),
        17.0 | 21.0 | 40.0 | 49.0 | 80.0 | 90.0 | 94.0 => GAction::Modal,
        18.0 | 19.0 | 20.0 | 90.1 | 91.0 | 91.1 | 92.0 | 93.0 | 95.0 | 96.0 | 97.0 => GAction::Mode,
        _ => GAction::Loss,
    }
}

/// A recognised program-structure `M` code that carries no geometry.
fn structural_m(value: f64) -> bool {
    matches!(value, 0.0 | 1.0 | 2.0 | 3.0 | 4.0 | 5.0 | 6.0 | 7.0 | 8.0 | 9.0 | 30.0)
}

/// One word of a block, e.g. `G1`, `X10.5` or `F300`.
#[derive(Debug)]
struct Word {
    letter: char,
    value: f64,
    text: String,
}

/// The cutting run currently being collected between two rapids.
#[derive(Debug, Default)]
struct Run {
    points: Vec<(f64, f64)>,
    word: String,
    line: usize,
}

impl Run {
    fn push(&mut self, point: (f64, f64), word: &str, line: usize) {
        if self.points.is_empty() {
            self.word = word.to_string();
            self.line = line;
        }
        self.points.push(point);
    }

    fn finish(&mut self, paths: &mut Vec<Vec<(f64, f64)>>, losses: &mut Vec<GcodeLoss>, skipped: &mut usize) {
        if self.points.is_empty() {
            return;
        }
        let points = dxf::remove_adjacent_duplicates(std::mem::take(&mut self.points));
        let (word, line) = (std::mem::take(&mut self.word), self.line);
        self.line = 0;
        if points.len() >= 2 {
            paths.push(points);
        }
        else {
            *skipped += 1;
            losses.push(GcodeLoss::new(
                "import/gcode-degenerate-run",
                word,
                line,
                "a cutting run produced fewer than two distinct points and was skipped".to_string(),
            ));
        }
    }
}

/// Imports the supported G0/G1/G2/G3 subset of a program into a new document.
///
/// Every maximal cutting run becomes one `engrave` operation whose path is the
/// run flattened to XY chords, translated so the bounding box minimum sits at
/// the origin, exactly like [`crate::dxf`] import. Z heights and feeds are
/// ignored; the fixed engraving tool and the request depth and stepdown give
/// the operations their cutting values. The result is not validated or written
/// here; [`crate::OpenCam::import_gcode`] does both.
pub fn import(text: &str, request: &GcodeImportRequest) -> Result<GcodeImport, CamError> {
    let request = request.checked()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut losses = Vec::new();
    let mut paths: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut position: Option<(f64, f64)> = None;
    let mut mode: Option<MoveMode> = None;
    let mut run = Run::default();
    let mut move_count = 0usize;
    let mut arc_count = 0usize;
    let mut skipped = 0usize;
    let mut line_count = 0usize;

    for raw in text.lines() {
        line_count += 1;
        let line = line_count;
        let body = strip_comments(raw, line)?;
        let body = body.trim();
        if body.is_empty() || body == "%" {
            continue;
        }
        let words = tokenize(body, line)?;
        let mut motion: Option<MoveMode> = None;
        let mut loss: Option<GcodeLoss> = None;
        let mut structural = false;
        let mut x = None;
        let mut y = None;
        let mut z = false;
        let mut i = None;
        let mut j = None;
        let mut radius = false;

        for word in &words {
            match word.letter {
                'G' => match gcode_action(word.value) {
                    GAction::Modal => {}
                    GAction::Motion(kind) => {
                        if motion.is_some() {
                            return Err(malformed_line(line, "a block carries more than one motion code"));
                        }
                        motion = Some(kind);
                        mode = Some(kind);
                    }
                    GAction::Mode => {
                        return Err(CamError::new(
                            "import/gcode-unsupported-mode",
                            format!(
                                "line {line}: `{}` is not supported; only absolute millimetre XY programming is read",
                                word.text
                            ),
                        ));
                    }
                    GAction::Loss if loss.is_none() => {
                        loss = Some(GcodeLoss::new(
                            "import/gcode-unsupported-gcode",
                            word.text.clone(),
                            line,
                            format!("`{}` is not supported and its geometry was skipped", word.text),
                        ));
                    }
                    GAction::Loss => {}
                },
                'M' => {
                    if structural_m(word.value) {
                        structural = true;
                    }
                    else if loss.is_none() {
                        loss = Some(GcodeLoss::new(
                            "import/gcode-unsupported-mcode",
                            word.text.clone(),
                            line,
                            format!("`{}` is not supported and was skipped", word.text),
                        ));
                    }
                }
                'X' => x = Some(word.value),
                'Y' => y = Some(word.value),
                'Z' => z = true,
                'I' => i = Some(word.value),
                'J' => j = Some(word.value),
                'R' => radius = true,
                'T' => structural = true,
                'F' | 'S' | 'N' | 'O' => {}
                _ => {
                    if loss.is_none() {
                        loss = Some(GcodeLoss::new(
                            "import/gcode-unknown-word",
                            word.text.clone(),
                            line,
                            format!("word `{}` is not part of the supported subset and was skipped", word.text),
                        ));
                    }
                }
            }
        }

        if let Some(loss) = loss {
            skipped += 1;
            losses.push(loss);
            mode = None;
            run.finish(&mut paths, &mut losses, &mut skipped);
            continue;
        }
        if radius {
            skipped += 1;
            losses.push(GcodeLoss::new(
                "import/gcode-unsupported-arc",
                "R".to_string(),
                line,
                "radius-form arcs are not supported; the move was skipped".to_string(),
            ));
            run.finish(&mut paths, &mut losses, &mut skipped);
            continue;
        }

        let has_axes = x.is_some() || y.is_some() || z || i.is_some() || j.is_some();
        if structural && !has_axes {
            run.finish(&mut paths, &mut losses, &mut skipped);
            continue;
        }
        if !has_axes {
            continue;
        }
        let Some(effective) = motion.or(mode)
        else {
            return Err(malformed_line(line, "coordinates appear before any motion code"));
        };

        match effective {
            MoveMode::Rapid => {
                if x.is_some() || y.is_some() {
                    position = match position {
                        Some(current) => Some((x.unwrap_or(current.0), y.unwrap_or(current.1))),
                        None => match (x, y) {
                            (Some(a), Some(b)) => Some((a, b)),
                            _ => None,
                        },
                    };
                }
                run.finish(&mut paths, &mut losses, &mut skipped);
            }
            MoveMode::Linear => {
                let word = "G1";
                if x.is_none() && y.is_none() {
                    if z {
                        if let Some(current) = position {
                            run.push(current, word, line);
                        }
                        move_count += 1;
                    }
                }
                else {
                    if let Some(current) = position {
                        run.push(current, word, line);
                    }
                    let end = match position {
                        Some(current) => (x.unwrap_or(current.0), y.unwrap_or(current.1)),
                        None => match (x, y) {
                            (Some(a), Some(b)) => (a, b),
                            _ => {
                                return Err(malformed_line(line, "a move without a known start needs both `X` and `Y`"));
                            }
                        },
                    };
                    run.push(end, word, line);
                    position = Some(end);
                    move_count += 1;
                }
            }
            arc @ (MoveMode::Clockwise | MoveMode::CounterClockwise) => {
                let word = if arc == MoveMode::Clockwise { "G2" } else { "G3" };
                let Some(start) = position
                else {
                    return Err(malformed_line(line, "an arc needs a known start point"));
                };
                let (Some(i), Some(j)) = (i, j)
                else {
                    return Err(malformed_line(line, "an arc needs both `I` and `J` centre offsets"));
                };
                let centre = (start.0 + i, start.1 + j);
                let radius = (i * i + j * j).sqrt();
                if radius <= 0.0 {
                    return Err(malformed_line(line, "an arc centre offset must not be zero"));
                }
                let end = match (x, y) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, start.1),
                    (None, Some(b)) => (start.0, b),
                    (None, None) => start,
                };
                let full = dxf::coincident(start, end);
                let start_angle = (start.1 - centre.1).atan2(start.0 - centre.0);
                let end_angle = (end.1 - centre.1).atan2(end.0 - centre.0);
                let sweep = if full {
                    TAU
                }
                else if arc == MoveMode::Clockwise {
                    (start_angle - end_angle).rem_euclid(TAU)
                }
                else {
                    (end_angle - start_angle).rem_euclid(TAU)
                };
                let mut segments = dxf::arc_segments(radius, sweep, request.tolerance);
                if full {
                    segments = segments.max(MIN_CIRCLE_SEGMENTS);
                }
                let step = sweep / segments as f64;
                run.push(start, word, line);
                for chord in 1..segments {
                    let angle = if arc == MoveMode::Clockwise {
                        start_angle - step * chord as f64
                    }
                    else {
                        start_angle + step * chord as f64
                    };
                    run.push(
                        (dxf::clean(centre.0 + radius * angle.cos()), dxf::clean(centre.1 + radius * angle.sin())),
                        word,
                        line,
                    );
                }
                run.push(end, word, line);
                position = Some(end);
                move_count += 1;
                arc_count += 1;
            }
        }
    }
    run.finish(&mut paths, &mut losses, &mut skipped);

    let document = build_document(&paths, &request);
    Ok(GcodeImport { document, line_count, move_count, arc_count, skipped_count: skipped, losses })
}

/// Removes `( … )` and `; …` comments from one line.
///
/// An unterminated `(` is a malformed block because the rest of the line
/// cannot be trusted.
fn strip_comments(line: &str, number: usize) -> Result<String, CamError> {
    let mut result = String::with_capacity(line.len());
    let mut in_comment = false;
    for character in line.chars() {
        if in_comment {
            if character == ')' {
                in_comment = false;
            }
        }
        else if character == '(' {
            in_comment = true;
        }
        else if character == ';' {
            break;
        }
        else {
            result.push(character);
        }
    }
    if in_comment {
        return Err(malformed_line(number, "a comment is missing its closing `)`"));
    }
    Ok(result)
}

/// Splits a block into its `letter value` words.
///
/// Whitespace between words is optional, so `G1X10Y20` and `G1 X10 Y20` read
/// the same. A word without a value or with a non-numeric value is malformed.
fn tokenize(body: &str, number: usize) -> Result<Vec<Word>, CamError> {
    let characters: Vec<char> = body.chars().collect();
    let mut words = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let letter = characters[index];
        if !letter.is_ascii_alphabetic() {
            return Err(malformed_line(number, format!("`{letter}` does not start a word")));
        }
        let mut value = String::new();
        index += 1;
        while index < characters.len() && !characters[index].is_ascii_alphabetic() {
            value.push(characters[index]);
            index += 1;
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(malformed_line(number, format!("word `{letter}` has no value")));
        }
        let Ok(parsed) = value.parse::<f64>()
        else {
            return Err(malformed_line(number, format!("word `{letter}{value}` is not a number")));
        };
        if !parsed.is_finite() {
            return Err(malformed_line(number, format!("word `{letter}{value}` is not finite")));
        }
        words.push(Word { letter: letter.to_ascii_uppercase(), value: parsed, text: format!("{letter}{value}") });
    }
    Ok(words)
}

fn malformed_line(line: usize, message: impl AsRef<str>) -> CamError {
    CamError::new("import/gcode-malformed-line", format!("line {line}: {}", message.as_ref()))
}

/// Builds the document: one `engrave` operation per cutting run, the stock
/// bounding box translated to the origin, exactly like DXF import.
fn build_document(paths: &[Vec<(f64, f64)>], request: &GcodeImportRequest) -> CamDocument {
    if paths.is_empty() {
        return CamDocument::new(DEFAULT_STOCK);
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for points in paths {
        for &(x, y) in points {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let width = dxf::clean(max_x - min_x).max(1.0);
    let depth = dxf::clean(max_y - min_y).max(1.0);
    let height = depth.max(request.depth) + IMPORT_STOCK_MARGIN;
    let mut document = CamDocument::new(Stock::new(width, depth, height));
    document.tools.push(dxf::engrave_tool());
    for (index, points) in paths.iter().enumerate() {
        let id = format!("gcode-{}", index + 1);
        let mut operation =
            Operation::new(id.clone(), OperationKind::Engrave, DEFAULT_ENGRAVE_TOOL_ID, request.depth, request.stepdown);
        operation.name = id;
        operation.path = points.iter().map(|&(x, y)| PathPoint::new(dxf::clean(x - min_x), dxf::clean(y - min_y))).collect();
        document.operations.push(operation);
    }
    document
}
