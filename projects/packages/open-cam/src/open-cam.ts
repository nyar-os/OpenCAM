/**
 * `new OpenCAM(options?)` machining session.
 *
 * The constructor is synchronous and opens nothing: no network, no disk scan,
 * no window, no file. Native initialisation happens inside the async methods, so
 * a failure is always awaited and always carries a stable code.
 */

import { CamDocument } from './document.js';
import { OpenCAMError } from './errors.js';
import * as service from './service.js';
import type { FeedsReport, FeedsRequest, OpenCAMCapabilities, OpenCAMOptions } from './types.js';

/** Machining session. One instance owns the documents it opens. */
export class OpenCAM {
    /** BCP-47 tag used to pick the CLI locale. */
    readonly locale: string;
    private readonly documents = new Set<CamDocument>();
    private closed = false;

    constructor(options: OpenCAMOptions = {}) {
        this.locale = options.locale ?? 'en-US';
    }

    /** Whether {@link dispose} has run. */
    get disposed(): boolean {
        return this.closed;
    }

    /** Operation and I/O disclosure for the installed engine. */
    async capabilities(): Promise<OpenCAMCapabilities> {
        this.assertActive();
        return service.capabilities();
    }

    /** Suggests spindle and feed rates for one material and tool diameter. */
    async feeds(request: FeedsRequest): Promise<FeedsReport> {
        this.assertActive();
        return service.feeds(request);
    }

    /** Creates an empty document at `path` and returns it. */
    async create(path: string): Promise<CamDocument> {
        this.assertActive();
        assertPath(path);
        const document = new CamDocument(service.create(path));
        this.documents.add(document);
        return document;
    }

    /** Opens the document at `path` and returns it. */
    async open(path: string): Promise<CamDocument> {
        this.assertActive();
        assertPath(path);
        const document = new CamDocument(service.open(path));
        this.documents.add(document);
        return document;
    }

    /** Closes every document the session still owns. Idempotent. */
    async dispose(): Promise<void> {
        if (this.closed) {
            return;
        }
        this.closed = true;
        for (const document of [...this.documents]) {
            document.close();
        }
        this.documents.clear();
    }

    private assertActive(): void {
        if (this.closed) {
            throw new OpenCAMError('session/disposed', 'the OpenCAM session has been disposed; create a new instance');
        }
    }
}

function assertPath(path: string): void {
    if (typeof path !== 'string' || path.length === 0) {
        throw new OpenCAMError('session/argument-invalid', '`path` must be a non-empty string', {
            details: { feature: 'session.path' },
        });
    }
}
