# Architecture

OpenCAM is one native core with three thin surfaces. This document records the
boundaries a change must respect.

## Layers

```
projects/crates/cam-model      document model, validation, toolpaths, G-code, reports
projects/crates/cam-napi       Node-API exports; values in, JSON strings out
projects/packages/open-cam     TypeScript SDK (session + document) and CLI
projects/sites/homepage        static product site (VMZ)
```

## Data flow

1. `OpenCam.create` writes a fresh `.ocam` document; `OpenCam.open` parses one.
2. An edit mutates a candidate document in memory, validates the whole document,
   writes it atomically, then returns a `DocumentSummary`.
3. `toolpaths` validates and expands every operation into explicit moves.
4. `gcode` renders the generated moves plus `G81`/`G83` drilling cycles.
5. `export` writes G-code, JSON, CSV or Markdown through the same atomic writer.

Because the Node-API addon and the SDK both call the same session, they cannot
disagree about a toolpath or an error code.

## Invariants

1. **The Rust session is the only implementation.** No toolpath, validation or
   G-code rule may be reimplemented in TypeScript.
2. **The Node-API layer maps values only.** Each export accepts a path or a JSON
   request and returns a JSON string; it adds no defaults that change results.
3. **The TypeScript layer parses arguments and formats output.** It owns help
   text and exit codes (`capability/*` → `2`, everything else → `1`).
4. **The capability matrix is the only support claim.** A feature not marked
   supported in `documentation/capabilities.md` and `CAPABILITIES_JSON` is not
   supported, regardless of an API name.
5. **A document is written only when it is machineable.** Every edit validates
   the candidate document and returns the first error instead of persisting a
   broken file.
6. **Writes are atomic.** The writer uses a temporary file and a rename; a reader
   never observes a partial program or document.
7. **Output is deterministic.** No timestamps and no randomised ordering appear
   in toolpaths, G-code or reports.
8. **Units are millimetres.** The engine does not convert; feeds are mm/min and
   spindle speeds are rpm.

## Document format

```json
{
  "format": "open-cam",
  "version": 1,
  "revision": 3,
  "units": "mm",
  "stock": { "width": 120, "depth": 80, "height": 12 },
  "tools": [
    { "id": "t6", "name": "6 mm flat", "number": 1, "diameter": 6, "flutes": 2,
      "spindle": 12000, "feed": 1200, "plungeFeed": 300 }
  ],
  "operations": [
    { "id": "face", "name": "face", "kind": "face", "tool": "t6", "depth": 1,
      "stepdown": 0.5, "stepover": 4, "x": 0, "y": 0, "width": 0, "height": 0,
      "holes": [], "peck": null }
  ]
}
```

`revision` increases on every successful edit and never decreases.

## Error semantics

Errors carry a stable `code` and a human `message`. The CLI prints
`code: message` to stderr. `capability/not-implemented` is reserved for declared
but unimplemented routes and maps to exit code `2`.

## Node-API surface

`version`, `capabilities`, `create`, `import_dxf`, `open`, `save`, `stock_set`, `tool_add`,
`tool_remove`, `tool_list`, `tool_export`, `tool_import`, `operation_add`,
`operation_remove`, `validate`, `toolpaths`, `estimate`, `gcode`, `export`,
`report`.

## Determinism

Raster lines and depth levels are produced by simple accumulating loops with a
`1e-9` tolerance, so the same inputs give the same breakpoints on every machine.
Metrics round only when rendered, not while calculating.
