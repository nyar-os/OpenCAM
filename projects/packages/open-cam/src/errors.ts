/**
 * Stable error surface of `@wae/open-cam`.
 *
 * Codes are contract, not prose: callers compare them, localize around them and
 * never parse the message.
 */

/** Every stable code this package may report. */
export const errorCodes = [
    'session/disposed',
    'session/argument-invalid',
    'session/no-document',
    'native/unavailable',
    'native/error',
    'file/exists',
    'io/read',
    'io/write',
    'document/parse',
    'document/serialize',
    'document/format',
    'document/version',
    'document/units',
    'stock/invalid',
    'tool/invalid',
    'tool/duplicate-id',
    'tool/unknown-id',
    'tool/in-use',
    'operation/invalid',
    'operation/duplicate-id',
    'operation/unknown-id',
    'operation/unknown-tool',
    'operation/unknown-kind',
    'operation/island-invalid',
    'operation/island-outside-pocket',
    'operation/island-too-close',
    'operation/island-overlap',
    'operation/strategy-island',
    'path/outside-stock',
    'export/invalid',
    'cli/unknown-command',
    'cli/missing-argument',
    'cli/missing-option',
    'cli/missing-value',
    'cli/unknown-option',
    'cli/invalid-number',
    'cli/unknown-format',
    'capability/not-implemented',
] as const;

/** Union of {@link errorCodes}. */
export type OpenCAMErrorCode = (typeof errorCodes)[number];

/** Optional locators a failure can carry. */
export interface OpenCAMErrorDetails {
    /** Capability or field involved in the failure, e.g. `tool.add.id`. */
    readonly feature?: string;
    /** Document path involved in the failure. */
    readonly path?: string;
}

/** Options accepted by {@link OpenCAMError}. */
export interface OpenCAMErrorOptions {
    readonly recoverable?: boolean;
    readonly details?: OpenCAMErrorDetails;
    readonly cause?: unknown;
}

/** Every failure thrown or rejected by this package. */
export class OpenCAMError extends Error {
    /** Stable, comparable code. */
    readonly code: OpenCAMErrorCode;
    /** Whether retrying with different input or state can succeed. */
    readonly recoverable: boolean;
    /** Optional locators (`feature`, `path`). */
    readonly details: OpenCAMErrorDetails;

    constructor(code: OpenCAMErrorCode, message: string, options: OpenCAMErrorOptions = {}) {
        super(message, options.cause === undefined ? undefined : { cause: options.cause });
        this.name = 'OpenCAMError';
        this.code = code;
        this.recoverable = options.recoverable ?? false;
        this.details = Object.freeze({ ...options.details });
    }
}

/** Reported when the optional native addon is absent or a route is unimplemented. */
export class NotImplementedError extends OpenCAMError {
    constructor(feature: string, message = `\`${feature}\` is not implemented yet`) {
        super('capability/not-implemented', message, { recoverable: false, details: { feature } });
        this.name = 'NotImplementedError';
    }
}

/** Narrows an unknown thrown value to {@link OpenCAMError}. */
export function isOpenCAMError(value: unknown): value is OpenCAMError {
    return value instanceof OpenCAMError;
}
