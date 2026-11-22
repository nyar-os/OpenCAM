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
    DocumentSummary,
    ExportFormat,
    ExportReport,
    OperationRequest,
    Report,
    Stock,
    StockRequest,
    ToolRequest,
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

    /** Adds a machining operation. */
    async addOperation(request: OperationRequest): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.addOperation(this.state.path, request);
        return this.state;
    }

    /** Removes an operation. */
    async removeOperation(id: string): Promise<DocumentSummary> {
        this.assertOpen();
        this.state = service.removeOperation(this.state.path, id);
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

    /** Renders the G-code program. */
    async gcode(): Promise<string> {
        this.assertOpen();
        return service.gcode(this.state.path);
    }

    /** Exports the document as `gcode`, `json`, `csv` or `md`. */
    async exportDocument(format: ExportFormat, target: string): Promise<ExportReport> {
        this.assertOpen();
        return service.exportDocument(this.state.path, format, target);
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
