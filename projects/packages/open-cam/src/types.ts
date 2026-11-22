/**
 * Declared types of `@wae/open-cam`.
 *
 * The shapes describe the real document: one rectangular stock, a tool table and
 * machining operations. Every editing call returns the summary of the document
 * as it was written to disk, so a caller never has to guess whether a change
 * reached the file.
 */

/** The only unit system this build accepts: millimetres and millimetres per minute. */
export type Units = 'mm';

/** Machining strategy of one operation. */
export type OperationKind = 'face' | 'contour' | 'pocket' | 'drill';

/** Export formats `exportDocument()` can write. */
export type ExportFormat = 'gcode' | 'json' | 'csv' | 'md';

/** Severity of a validation finding. */
export type Severity = 'error' | 'warning' | 'notice';

/** Rectangular stock; the top face is `z = height`. */
export interface Stock {
    readonly width: number;
    readonly depth: number;
    readonly height: number;
}

/** One cutting tool. */
export interface Tool {
    readonly id: string;
    readonly name: string;
    /** Positive tool number emitted as `T<n> M6`. */
    readonly number: number;
    readonly diameter: number;
    readonly flutes: number;
    /** Spindle speed in revolutions per minute. */
    readonly spindle: number;
    /** Cutting feed in millimetres per minute. */
    readonly feed: number;
    /** Plunge feed in millimetres per minute. */
    readonly plungeFeed: number;
}

/** One hole centre of a drilling operation. */
export interface Hole {
    readonly x: number;
    readonly y: number;
}

/** One machining operation. */
export interface Operation {
    readonly id: string;
    readonly name: string;
    readonly kind: OperationKind;
    readonly tool: string;
    /** Total depth below the stock top. */
    readonly depth: number;
    /** Maximum depth removed per Z pass. */
    readonly stepdown: number;
    /** Raster spacing; `null` selects the kind default. */
    readonly stepover: number | null;
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
    readonly holes: readonly Hole[];
    /** Peck increment of a drill operation; `null` uses a single plunge. */
    readonly peck: number | null;
}

/** Summary of one document as written to disk. */
export interface DocumentSummary {
    readonly id: string;
    readonly path: string;
    readonly units: string;
    readonly revision: number;
    readonly stock: Stock;
    readonly tools: number;
    readonly operations: number;
}

/** One validation finding with a stable code. */
export interface Issue {
    readonly code: string;
    readonly severity: Severity;
    readonly message: string;
}

/** Result of `validate()`. */
export interface ValidationReport {
    readonly ok: boolean;
    readonly issues: readonly Issue[];
}

/** Kind of one machine move. */
export type MoveKind = 'rapid' | 'plunge' | 'cut';

/** One point in machine coordinates. */
export interface Point {
    readonly x: number;
    readonly y: number;
    readonly z: number;
}

/** One machine move. */
export interface Move {
    readonly kind: MoveKind;
    readonly from: Point;
    readonly to: Point;
    readonly length: number;
    readonly feed: number;
}

/** Generated path of one operation. */
export interface OperationPath {
    readonly operation: string;
    readonly kind: OperationKind;
    readonly tool: string;
    readonly moves: number;
    readonly cutLength: number;
    readonly rapidLength: number;
    readonly cuttingSeconds: number;
    readonly rapidSeconds: number;
    readonly segments: readonly Move[];
}

/** Complete toolpath report. */
export interface ToolpathReport {
    readonly format: string;
    readonly version: number;
    readonly units: string;
    readonly safeZ: number;
    readonly rapidFeed: number;
    readonly cutLength: number;
    readonly rapidLength: number;
    readonly estimatedSeconds: number;
    readonly operations: readonly OperationPath[];
}

/** Result of `exportDocument()`. */
export interface ExportReport {
    readonly format: ExportFormat;
    readonly path: string;
    readonly files: readonly string[];
    readonly bytes: number;
}

/** Full report returned by `report()`. */
export interface Report {
    readonly format: string;
    readonly version: number;
    readonly units: string;
    readonly document: DocumentSummary;
    readonly validation: ValidationReport;
    readonly toolpaths: ToolpathReport;
    readonly gcodeLines: number;
}

/** Session options. Constructing a session opens no file. */
export interface OpenCAMOptions {
    /** BCP-47 tag used to pick the CLI locale. Defaults to `en-US`. */
    readonly locale?: string;
}

/** Request of `setStock()`. */
export interface StockRequest {
    readonly width: number;
    readonly depth: number;
    readonly height: number;
}

/** Request of `addTool()`. */
export interface ToolRequest {
    readonly id: string;
    readonly name?: string;
    /** Defaults to `1`. */
    readonly number?: number;
    readonly diameter: number;
    /** Defaults to `2`. */
    readonly flutes?: number;
    readonly spindle: number;
    readonly feed: number;
    /** Defaults to `feed / 3`. */
    readonly plungeFeed?: number;
}

/** Request of `addOperation()`; geometry fields depend on `kind`. */
export interface OperationRequest {
    readonly id: string;
    readonly name?: string;
    readonly kind: OperationKind;
    readonly tool: string;
    readonly depth: number;
    /** Defaults to `depth`, i.e. one Z pass. */
    readonly stepdown?: number;
    readonly stepover?: number;
    readonly x?: number;
    readonly y?: number;
    readonly width?: number;
    readonly height?: number;
    readonly holes?: readonly Hole[];
    readonly peck?: number;
}

/** Disclosure level of one capability entry. */
export type SupportLevel = 'supported' | 'unsupported';

/** Engine capability matrix. */
export interface OpenCAMCapabilities {
    readonly engine: { readonly package: string; readonly version: string };
    readonly operations: Readonly<Record<string, SupportLevel>>;
    readonly io: Readonly<Record<string, SupportLevel>>;
}
