/**
 * Orthogonal native operations.
 *
 * One function performs one operation and returns data; argument parsing, help
 * text and exit codes belong to `cli.ts`. Every function forwards to the
 * optional native addon and decodes one JSON string.
 */

import { NotImplementedError, OpenCAMError, errorCodes, type OpenCAMErrorCode } from './errors.js';
import { nativeBinding } from './native.js';
import type {
    CycleTimeReport,
    DocumentSummary,
    DrawingFormat,
    DrawingReport,
    DxfImportReport,
    DxfImportRequest,
    EstimateRequest,
    ExportFormat,
    ExportReport,
    FeedsReport,
    FeedsRequest,
    GcodeImportReport,
    GcodeImportRequest,
    OperationDuplicateRequest,
    OperationListReport,
    OperationReorderReport,
    OperationReorderRequest,
    OperationRequest,
    OperationUpdateRequest,
    PostOptions,
    Report,
    SequenceReport,
    StockRequest,
    ToolImportReport,
    ToolLibraryReport,
    ToolRequest,
    ToolTableReport,
    ToolpathReport,
    ValidationReport,
} from './types.js';

const knownCodes = new Set<string>(errorCodes);

function binding(feature: string) {
    const native = nativeBinding();
    if (!native) {
        throw new NotImplementedError(feature);
    }
    return native;
}

function decode<T>(work: () => string): T {
    try {
        return JSON.parse(work()) as T;
    } catch (error) {
        throw translate(error);
    }
}

function translate(error: unknown): OpenCAMError {
    if (error instanceof OpenCAMError) {
        return error;
    }
    const message = error instanceof Error ? error.message : String(error);
    const separator = message.indexOf(': ');
    const code = separator > 0 ? message.slice(0, separator) : '';
    if (knownCodes.has(code)) {
        return new OpenCAMError(code as OpenCAMErrorCode, message.slice(separator + 2));
    }
    return new OpenCAMError('native/error', message, { cause: error });
}

/** Reads the version of the installed addon. */
export function version(): string {
    return decode<string>(() => binding('version').version());
}

/** Reads the capability matrix of the installed addon. */
export function capabilities() {
    const matrix = decode<{ operations: Record<string, 'supported' | 'unsupported'>; io: Record<string, 'supported' | 'unsupported'> }>(
        () => binding('capabilities').capabilities(),
    );
    return {
        engine: { package: 'open-cam-napi', version: version() },
        operations: matrix.operations,
        io: matrix.io,
    };
}

/** Creates an empty document and returns its summary. */
export function create(path: string): DocumentSummary {
    return decode<DocumentSummary>(() => binding('create').create(path));
}

/** Opens a document and returns its summary. */
export function open(path: string): DocumentSummary {
    return decode<DocumentSummary>(() => binding('open').open(path));
}

/** Imports an ASCII DXF drawing into a new document and returns the report. */
export function importDxf(path: string, request: Omit<DxfImportRequest, 'path'>): DxfImportReport {
    return decode<DxfImportReport>(() => binding('import.dxf').import_dxf(path, JSON.stringify(request)));
}

/** Imports the supported G-code subset into a new document and returns the report. */
export function importGcode(path: string, request: Omit<GcodeImportRequest, 'path'>): GcodeImportReport {
    return decode<GcodeImportReport>(() => binding('import.gcode').import_gcode(path, JSON.stringify(request)));
}

/** Writes a document to `target`, or in place when `target` is undefined. */
export function save(path: string, target?: string): DocumentSummary {
    return decode<DocumentSummary>(() =>
        target === undefined ? binding('save').save(path) : binding('save').save(path, target),
    );
}

/** Replaces the stock block and returns the updated summary. */
export function setStock(path: string, request: StockRequest): DocumentSummary {
    return decode<DocumentSummary>(() => binding('stock.set').stock_set(path, JSON.stringify(request)));
}

/** Adds a tool and returns the updated summary. */
export function addTool(path: string, request: ToolRequest): DocumentSummary {
    return decode<DocumentSummary>(() => binding('tool.add').tool_add(path, JSON.stringify(request)));
}

/** Removes an unused tool and returns the updated summary. */
export function removeTool(path: string, id: string): DocumentSummary {
    return decode<DocumentSummary>(() => binding('tool.remove').tool_remove(path, JSON.stringify({ id })));
}

/** Lists every declared tool with the operations that use it. */
export function listTools(path: string): ToolTableReport {
    return decode<ToolTableReport>(() => binding('tool.list').tool_list(path));
}

/** Writes the tool table as a portable library file. */
export function exportTools(path: string, target: string): ToolLibraryReport {
    return decode<ToolLibraryReport>(() => binding('tool.export').tool_export(path, target));
}

/** Merges a portable tool library into the document. */
export function importTools(path: string, source: string): ToolImportReport {
    return decode<ToolImportReport>(() => binding('tool.import').tool_import(path, source));
}

/** Adds a machining operation and returns the updated summary. */
export function addOperation(path: string, request: OperationRequest): DocumentSummary {
    return decode<DocumentSummary>(() => binding('operation.add').operation_add(path, JSON.stringify(request)));
}

/** Removes an operation and returns the updated summary. */
export function removeOperation(path: string, id: string): DocumentSummary {
    return decode<DocumentSummary>(() =>
        binding('operation.remove').operation_remove(path, JSON.stringify({ id })),
    );
}

/** Deletes an operation and returns the updated summary. */
export function deleteOperation(path: string, id: string): DocumentSummary {
    return decode<DocumentSummary>(() =>
        binding('operation.delete').operation_delete(path, JSON.stringify({ id })),
    );
}

/** Copies an operation after itself and returns the updated summary. */
export function duplicateOperation(path: string, request: OperationDuplicateRequest): DocumentSummary {
    return decode<DocumentSummary>(() =>
        binding('operation.duplicate').operation_duplicate(path, JSON.stringify(request)),
    );
}

/** Moves an operation to a final 0-based position and returns the reorder report. */
export function reorderOperation(path: string, request: OperationReorderRequest): OperationReorderReport {
    return decode<OperationReorderReport>(() =>
        binding('operation.reorder').operation_reorder(path, JSON.stringify(request)),
    );
}

/** Lists every operation of a document without changing it. */
export function listOperations(path: string): OperationListReport {
    return decode<OperationListReport>(() => binding('operation.list').operation_list(path));
}

/** Applies a partial update to one operation and returns the updated summary. */
export function updateOperation(path: string, request: OperationUpdateRequest): DocumentSummary {
    return decode<DocumentSummary>(() =>
        binding('operation.update').operation_update(path, JSON.stringify(request)),
    );
}

/** Validates a document. */
export function validate(path: string): ValidationReport {
    return decode<ValidationReport>(() => binding('validate').validate(path));
}

/** Generates the toolpath report of a document. */
export function toolpaths(path: string): ToolpathReport {
    return decode<ToolpathReport>(() => binding('toolpaths').toolpaths(path));
}

/** Estimates the cycle time of a document. */
export function estimate(path: string, request: EstimateRequest = {}): CycleTimeReport {
    return decode<CycleTimeReport>(() => binding('estimate').estimate(path, JSON.stringify(request)));
}

/** Renders the G-code program of a document. */
export function gcode(path: string, request: PostOptions = {}): string {
    return decode<string>(() => binding('gcode').gcode(path, JSON.stringify(request)));
}

/** Plans the tool-grouped operation order of a document without changing it. */
export function sequence(path: string): SequenceReport {
    return decode<SequenceReport>(() => binding('sequence').sequence(path));
}

/** Reorders a document by tool, saves it and returns the applied plan. */
export function applySequence(path: string): SequenceReport {
    return decode<SequenceReport>(() => binding('sequence.apply').sequence_apply(path));
}

/** Suggests spindle and feed rates for one material and tool diameter. */
export function feeds(request: FeedsRequest): FeedsReport {
    return decode<FeedsReport>(() => binding('feeds').feeds(JSON.stringify(request)));
}

/** Exports a document as `gcode`, `json`, `csv`, `md`, `svg` or `pdf`. */
export function exportDocument(path: string, format: ExportFormat, target: string): ExportReport {
    return decode<ExportReport>(() => binding('export').export(path, format, target));
}

/** Renders a document as an `svg` or `pdf` drawing without writing it. */
export function drawing(path: string, format: DrawingFormat): DrawingReport {
    return decode<DrawingReport>(() => binding('drawing').drawing(path, format));
}

/** Returns the full report of a document. */
export function report(path: string): Report {
    return decode<Report>(() => binding('report').report(path));
}
