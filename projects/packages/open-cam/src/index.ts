/**
 * Public surface of `@wae/open-cam`.
 *
 * The Rust session (`cam-model`'s `OpenCam`) is the core: it owns document
 * editing, validation, toolpath generation and export. This package is the
 * typed wrapper and the CLI is a thin assembly over it.
 */

export { CamDocument } from './document.js';
export { OpenCAM } from './open-cam.js';
export { NotImplementedError, OpenCAMError, errorCodes, isOpenCAMError } from './errors.js';
export type { OpenCAMErrorCode, OpenCAMErrorDetails, OpenCAMErrorOptions } from './errors.js';
export type * from './types.js';
