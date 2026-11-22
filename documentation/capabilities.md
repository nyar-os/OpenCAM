# Capabilities

This document is the support claim for OpenCAM. Anything not marked supported
here is not a support claim, even if an API name suggests it.

The machine-readable version is printed by `open-cam capabilities` and by
`OpenCAM.capabilities()`; both read `CAPABILITIES_JSON`.

## Capability matrix

| Capability | Status |
|------------|--------|
| rectangular stock, millimetres | ✅ supported |
| tool table with diameter, flutes, spindle and feeds | ✅ supported |
| `face` raster facing | ✅ supported |
| `contour` outside profile, cutter-radius offset | ✅ supported |
| `pocket` rectangular raster clearing | ✅ supported |
| `drill` with `G81` and peck `G83` | ✅ supported |
| multi-pass Z depth (`stepdown`) | ✅ supported |
| raster spacing (`stepover`) | ✅ supported |
| toolpath report (moves, cut/rapid length, cycle time) | ✅ supported |
| JSON / CSV / Markdown reports | ✅ supported |
| generic absolute-millimetre G-code | ✅ supported |
| 3D surfacing, STL/solid input | ❌ not implemented |
| adaptive clearing | ❌ not implemented |
| turning | ❌ not implemented |
| four-axis and five-axis work | ❌ not implemented |
| STEP / DXF import | ❌ not implemented |
| vendor post-processors | ❌ not implemented |

## Algorithms

1. **Depth levels.** `level_depths(depth, stepdown)` emits `stepdown`,
   `2·stepdown`, … and always ends at `depth`. A pass at the final depth is never
   duplicated.
2. **Raster lines.** `line_positions(span, step)` emits `0, step, 2·step, …` and
   appends `span` when the last line does not land on the boundary. Both ends of
   the surface are therefore always cut.
3. **Facing.** For every depth level the tool rapids to the first raster line,
   plunges at `plungeFeed`, zig-zags across the full stock width, steps over at
   the cutting feed and retracts to the clearance plane.
4. **Contour.** The tool centre follows the rectangle offset outward by
   `diameter / 2`, closing all four edges at every depth level.
5. **Pocket.** The raster runs between `x + r … x + width − r` and
   `y + r … y + height − r`; the validator rejects a pocket narrower than the
   tool so the shrunk rectangle can never invert.
6. **Drilling.** Each hole is a rapid to the centre, a plunge to
   `height − depth`, and a rapid retract. G-code uses `G81` (or `G83` with `Q` for
   peck) at the clearance plane `R` and cancels the cycle with `G80`.
7. **Metrics.** Cutting length and time use the cutting feed; rapid length and
   time use a fixed 8000 mm/min traverse. Cycle time is the sum of both.

## G-code

The program starts with `%`, a comment block, `G21 G90 G17 G94` and `G0 Z<safe>`.
Each operation emits a comment, `T<n> M6`, `S<spindle> M3`, the moves or cycle,
`G0 Z<safe>` and `M5`. The program ends with `G0 Z<safe>`, `M30`, `%`.

`safe_z` is `stock.height + 5` mm. Coordinates render with at most three decimals
and `-0` is normalised to `0`.

## Validation

| Code | Meaning |
|------|---------|
| `document/format`, `document/version`, `document/units` | the file is not an OpenCAM millimetre document |
| `stock/invalid` | a stock size is not finite and positive |
| `tool/invalid`, `tool/duplicate-id` | a tool is malformed or declared twice |
| `operation/invalid` | depth, stepdown, stepover, geometry, holes or peck are invalid |
| `operation/duplicate-id` | an operation id is declared twice |
| `operation/unknown-tool` | an operation references a missing tool |
| `path/outside-stock` | a profile, pocket or hole leaves the stock |

Warnings (`tool/none`, `operation/none`, `tool/duplicate-number`, contour offset
outside the stock) do not make `ok` false.

## Tests

| File | Asserts |
|------|---------|
| `tests/toolpath.rs` | depth levels, closed contour, pocket containment, drill plunges, determinism |
| `tests/gcode.rs` | header/footer, `T<n> M6`, `G81`/`G80`, clearance plane, byte identity |
| `tests/validation.rs` | every error code above and warning-only documents |
| `tests/session.rs` | create, edit, reopen, export, in-use tool, atomic save-as |
| `tests/smoke.test.ts` (TypeScript) | session lifecycle and argument errors before the addon |

## Limitations

- Rasters are axis-aligned; there is no trochoidal or adaptive strategy.
- Pockets are rectangular only; no islands or arbitrary boundaries.
- Contours are rectangles with a constant cutter-radius offset; no arcs or
  lead-in/lead-out moves.
- The validator checks geometry against the stock, not against fixtures, clamps
  or machine travel limits.
- Cycle time is a kinematic estimate, not a machine simulation.
