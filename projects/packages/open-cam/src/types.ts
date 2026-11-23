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
export type OperationKind = 'face' | 'contour' | 'pocket' | 'drill' | 'engrave';

/** Pocket clearing pattern: axis-aligned raster or concentric rectangles. */
export type PocketStrategy = 'raster' | 'spiral';

/** Contour or pocket outline: an axis-aligned rectangle (default) or a circle. */
export type PocketShape = 'rect' | 'circle';

/** Interpolation of the closed rings of a circular contour or pocket. */
export type ArcMode = 'linear' | 'arc';

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

/** One declared tool with the operations that reference it. */
export interface ToolEntry extends Tool {
    /** Identifiers of the operations that use this tool, in document order. */
    readonly usedBy: readonly string[];
    /** `true` when at least one operation references the tool. */
    readonly used: boolean;
}

/** Tool table of one document, as returned by `listTools()`. */
export interface ToolTableReport {
    readonly format: string;
    readonly version: number;
    readonly units: string;
    /** Number of declared tools. */
    readonly toolCount: number;
    /** Number of tools no operation references. */
    readonly unusedCount: number;
    readonly tools: readonly ToolEntry[];
}

/** One hole centre of a drilling operation. */
export interface Hole {
    readonly x: number;
    readonly y: number;
}

/** One vertex of the polyline engraved by an `engrave` operation. */
export interface PathPoint {
    readonly x: number;
    readonly y: number;
}

/** One rectangular island left standing inside a pocket. */
export interface Island {
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
}

/** Holding tabs left standing along a `contour` profile. */
export interface TabSettings {
    /** Number of tabs spaced evenly around the profile perimeter. */
    readonly count: number;
    /** Chord width of one tab along the profile, in millimetres. */
    readonly width: number;
    /** Material height left above the cut floor, in millimetres. */
    readonly height: number;
}

/** Rotation direction of the contour lead arcs. */
export type LeadDirection = 'cw' | 'ccw';

/** Tangential half-circle lead-in and lead-out of a `contour` operation. */
export interface LeadSettings {
    /** Radius of both lead arcs, in millimetres. */
    readonly radius: number;
    /** Arc rotation direction; `cw` renders `G2`, `ccw` renders `G3`. */
    readonly direction: LeadDirection;
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
    /** Horizontal ramp entry length of every Z level, in millimetres; absent plunges vertically. */
    readonly ramp?: number;
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
    readonly holes: readonly Hole[];
    /** Polyline vertices of an engrave operation, connected in order. */
    readonly path?: readonly PathPoint[];
    /** Closes an engrave polyline back to its first point; absent means open. */
    readonly closed?: boolean;
    /** Rectangular islands left standing by a pocket operation. */
    readonly islands?: readonly Island[];
    /** Peck increment of a drill operation; `null` uses a single plunge. */
    readonly peck: number | null;
    /** Bottom-of-hole pause of a drill operation in seconds; omitted when unset. */
    readonly dwell?: number;
    /** Pocket clearing pattern; absent means `raster`. */
    readonly strategy?: PocketStrategy;
    /** Contour or pocket outline; absent means `rect`. */
    readonly shape?: PocketShape;
    /** Holding tabs of a rectangular contour operation; absent cuts the full profile. */
    readonly tabs?: TabSettings;
    /** Adds one wall finish loop per Z level to a pocket operation. */
    readonly finish?: boolean;
    /** Tangential lead arcs of a contour operation; absent plunges vertically. */
    readonly lead?: LeadSettings;
    /** Ring interpolation of a circular contour or pocket; absent means `linear` (72 chords). */
    readonly arcs?: ArcMode;
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
export type MoveKind = 'rapid' | 'plunge' | 'cut' | 'arc';

/** One point in machine coordinates. */
export interface Point {
    readonly x: number;
    readonly y: number;
    readonly z: number;
}

/** Arc parameters of one `arc` move. */
export interface ArcParams {
    /** Centre of the arc circle. */
    readonly centre: Point;
    /** `true` renders `G2`, `false` renders `G3`. */
    readonly clockwise: boolean;
}

/** One machine move. */
export interface Move {
    readonly kind: MoveKind;
    readonly from: Point;
    readonly to: Point;
    /** Path length: Euclidean for linear moves, arc length for arcs. */
    readonly length: number;
    readonly feed: number;
    /** Present only for an `arc` move. */
    readonly arc?: ArcParams;
}

/** Generated path of one operation. */
export interface OperationPath {
    readonly operation: string;
    readonly kind: OperationKind;
    /** Present only for a spiral pocket. */
    readonly strategy?: PocketStrategy;
    /** Present only for a circular pocket or circular contour. */
    readonly shape?: 'circle';
    readonly tool: string;
    readonly moves: number;
    readonly cutLength: number;
    readonly rapidLength: number;
    readonly cuttingSeconds: number;
    readonly rapidSeconds: number;
    /** Number of islands this operation clears around. */
    readonly islandCount: number;
    /** Raster length skipped because it falls inside an island keep-out zone. */
    readonly skippedLength: number;
    /** Number of raster passes skipped because of island keep-out zones. */
    readonly skippedSegments: number;
    /** Present only for a spiral or circular pocket. */
    readonly loopCount?: number;
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

/** Request of `estimateCycleTime()`; omitted fields select the engine defaults. */
export interface EstimateRequest {
    /** Rapid traverse rate in millimetres per minute; defaults to `8000`. */
    readonly rapidFeed?: number;
    /** Fixed tool-change overhead per distinct tool in seconds; defaults to `3`. */
    readonly toolChangeSeconds?: number;
}

/** Cycle time estimate of one operation. */
export interface OperationEstimate {
    readonly operation: string;
    readonly kind: OperationKind;
    readonly tool: string;
    readonly cuttingLength: number;
    readonly plungeLength: number;
    readonly retractLength: number;
    readonly traverseLength: number;
    readonly cuttingSeconds: number;
    readonly plungeSeconds: number;
    readonly retractSeconds: number;
    readonly traverseSeconds: number;
    /** Non-zero only on the first operation that uses its tool. */
    readonly toolchangeSeconds: number;
    readonly totalSeconds: number;
}

/** Complete cycle time report. */
export interface CycleTimeReport {
    readonly format: string;
    readonly version: number;
    readonly units: string;
    readonly rapidFeed: number;
    readonly toolChangeSeconds: number;
    readonly toolChanges: number;
    readonly cuttingSeconds: number;
    readonly plungeSeconds: number;
    readonly retractSeconds: number;
    readonly traverseSeconds: number;
    readonly toolchangeSeconds: number;
    readonly totalSeconds: number;
    readonly operations: readonly OperationEstimate[];
}

/** Result of `exportDocument()`. */
export interface ExportReport {
    readonly format: ExportFormat;
    readonly path: string;
    readonly files: readonly string[];
    readonly bytes: number;
}

/** Result of `exportTools()`. */
export interface ToolLibraryReport {
    readonly format: 'tools';
    readonly path: string;
    readonly files: readonly string[];
    readonly bytes: number;
}

/** Result of `importTools()`. */
export interface ToolImportReport {
    readonly format: string;
    readonly version: number;
    /** Library file that was read. */
    readonly path: string;
    /** Tool ids that were added, in library order. */
    readonly imported: readonly string[];
    /** Number of tools the document declares after the import. */
    readonly toolCount: number;
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
    /** Horizontal ramp entry length of every Z level, in millimetres; omitted plunges vertically. */
    readonly ramp?: number;
    readonly x?: number;
    readonly y?: number;
    readonly width?: number;
    readonly height?: number;
    readonly holes?: readonly Hole[];
    /** Polyline vertices of an engrave operation, connected in order. */
    readonly path?: readonly PathPoint[];
    /** Closes the engrave polyline back to its first point; `false` when omitted. */
    readonly closed?: boolean;
    /** Rectangular islands left standing by a pocket operation. */
    readonly islands?: readonly Island[];
    readonly peck?: number;
    /** Bottom-of-hole pause in seconds; only `drill` accepts it and it cannot be combined with `peck`. */
    readonly dwell?: number;
    /** Pocket clearing pattern; `raster` when omitted. */
    readonly strategy?: PocketStrategy;
    /** Contour or pocket outline; `rect` when omitted. `circle` uses `width` as the diameter. */
    readonly shape?: PocketShape;
    /** Holding tabs of a rectangular contour operation; omitted cuts the full profile. */
    readonly tabs?: TabSettings;
    /** Adds one wall finish loop per Z level to a pocket operation; `false` when omitted. */
    readonly finish?: boolean;
    /** Tangential lead arcs of a contour operation; omitted plunges vertically. Cannot be combined with `ramp`. */
    readonly lead?: LeadSettings;
    /** Ring interpolation of a circular contour or pocket; `linear` when omitted. */
    readonly arcs?: ArcMode;
}

/** Disclosure level of one capability entry. */
export type SupportLevel = 'supported' | 'unsupported';

/** Engine capability matrix. */
export interface OpenCAMCapabilities {
    readonly engine: { readonly package: string; readonly version: string };
    readonly operations: Readonly<Record<string, SupportLevel>>;
    readonly io: Readonly<Record<string, SupportLevel>>;
}
