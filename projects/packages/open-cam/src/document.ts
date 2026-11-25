/**
 * Path-backed document.
 *
 * `OpenCAM.create()` and `OpenCAM.open()` return one of these. Every editing
 * method forwards to the native session, which writes the file atomically, and
 * stores the summary it returns so `id`, `units`, `stock` and `revision` always
 * describe what is on disk.
 */

import { OpenCAMError } from './errors.js';
import * as service from './service.js';
import type {
    CycleTimeReport,
    DocumentSummary,
    DrawingFormat,
    DrawingKind,
    DrawingReport,
    EstimateRequest,
    ExportFormat,
    ExportReport,
    OperationDuplicateRequest,
    OperationListReport,
    OperationReorderReport,
    OperationReorderRequest,
    OperationRequest,
    OperationUpdateRequest,
    PostOptions,
    Report,
    SequenceReport,
    Stock,
    StockRequest,
    ToolImportReport,
    ToolLibraryReport,
    ToolRequest,
    ToolTableReport,
    ToolpathReport,
    ValidationReport,
} from './types.js';

/** One open document. */
export class CamDocument {
    private state: DocumentSummary;
    private closed = false;

    constructor(summary: DocumentSummary) {
        this.state = summary;
    }

    /** Document identity, derived from the file stem. */
    get id(): string {
        return this.state.id;
    }

    /** Path of the document file. */
    get path(): string {
        return this.state.path;
    }

    /** Committed revision. */
    get revision(): number {
        return this.state.revision;
    }

    /** Declared unit system. */
    get units(): string {
        return this.state.units;
    }

    /** Current stock block. */
    get stock(): Stock {
        return this.state.stock;
    }

    /** Number of tools. */
    get tools(): number {
        return this.state.tools;
    }

    /** Number of operations. */
    get operations(): number {
        return this.state.operations;
    }

    /** Replaces the stock block. */
    async setStock(request: StockRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.setStock(this.state.path, request);
        return this.state;
    }

    /** Adds a tool. */
    async addTool(request: ToolRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.addTool(this.state.path, request);
        return this.state;
    }

    /** Removes an unused tool. */
    async removeTool(id: string): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.removeTool(this.state.path, id);
        return this.state;
    }

    /** Lists every declared tool with the operations that use it. */
    async listTools(): Promise<ToolTableReport> {
        this.assertOpen();
        return service.listTools(this.state.path);
    }

    /** Writes the tool table as a portable library file. */
    async exportTools(target: string): Promise<ToolLibraryReport> {
        this.assertOpen();
        return service.exportTools(this.state.path, target);
    }

    /** Merges a portable tool library into the document. */
    async importTools(source: string): Promise<ToolImportReport> {
        this.assertOpen();
        const report = service.importTools(this.state.path, source);
        this.state = service.open(this.state.path);
        return report;
    }

    /** Adds a machining operation. */
    async addOperation(request: OperationRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.addOperation(this.state.path, request);
        return this.state;
    }

    /** Removes an operation; the alias of {@link deleteOperation}. */
    async removeOperation(id: string): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.removeOperation(this.state.path, id);
        return this.state;
    }

    /** Deletes an operation. */
    async deleteOperation(id: string): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.deleteOperation(this.state.path, id);
        return this.state;
    }

    /** Copies an operation directly after itself. */
    async duplicateOperation(request: OperationDuplicateRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.duplicateOperation(this.state.path, request);
        return this.state;
    }

    /** Moves an operation to a final 0-based position; a no-op keeps the revision. */
    async reorderOperation(request: OperationReorderRequest): Promise<OperationReorderReport> {
        this.assertOpen();
        const report = service.reorderOperation(this.state.path, request);
        this.state = service.open(this.state.path);
        return report;
    }

    /** Lists every operation of the document without changing it. */
    async listOperations(): Promise<OperationListReport> {
        this.assertOpen();
        return service.listOperations(this.state.path);
    }

    /** Applies a partial update to one operation. */
    async updateOperation(request: OperationUpdateRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.updateOperation(this.state.path, request);
        return this.state;
    }

    /** Validates the document. */
    async validate(): Promise<ValidationReport> {
        this.assertOpen();
        return service.validate(this.state.path);
    }

    /** Generates the toolpath report. */
    async toolpaths(): Promise<ToolpathReport> {
        this.assertOpen();
        return service.toolpaths(this.state.path);
    }

    /** Estimates the cycle time of the document. */
    async estimateCycleTime(request?: EstimateRequest): Promise<CycleTimeReport> {
        this.assertOpen();
        return service.estimate(this.state.path, request);
    }

    /** Renders the G-code program, optionally with post options that change the program text only. */
    async gcode(options: PostOptions = {}): Promise<string> {
        this.assertOpen();
        return service.gcode(this.state.path, options);
    }

    /** Returns the tool-grouped plan without changing the document. */
    async sequence(): Promise<SequenceReport> {
        this.assertOpen();
        return service.sequence(this.state.path);
    }

    /** Reorders the document by tool and saves it. */
    async applySequence(): Promise<SequenceReport> {
        this.assertOpen();
        const report = service.applySequence(this.state.path);
        this.state = service.open(this.state.path);
        return report;
    }

    /** Exports the document as `gcode`, `json`, `csv`, `md`, `svg` or `pdf`. */
    async exportDocument(format: ExportFormat, target: string): Promise<ExportReport> {
        this.assertOpen();
        return service.exportDocument(this.state.path, format, target);
    }

    /** Renders the document as an `svg` or `pdf` sheet without writing it. */
    async renderDrawing(format: DrawingFormat, kind: DrawingKind = 'toolpath'): Promise<DrawingReport> {
        this.assertOpen();
        return service.drawing(this.state.path, format, kind);
    }

    /** Returns the full report. */
    async report(): Promise<Report> {
        this.assertOpen();
        return service.report(this.state.path);
    }

    /** Writes the document in place, or to `target` when supplied. */
    async save(target?: string): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.save(this.state.path, target);
        return this.state;
    }

    /** Marks the document closed. Idempotent. */
    close(): void {
        this.closed = true;
    }

    private assertOpen(): void {
        if (this.closed) {
            throw new OpenCAMError('session/disposed', 'the document has been closed; open it again');
        }
    }
}
