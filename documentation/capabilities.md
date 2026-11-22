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
| `contour` holding tabs (`tabs`) | ✅ supported |
| `pocket` rectangular raster clearing | ✅ supported |
| `pocket` spiral (concentric rectangle) clearing | ✅ supported |
| `pocket` islands (rectangular) | ✅ supported |
| `pocket` circle (concentric ring) clearing | ✅ supported |
| `drill` with `G81` and peck `G83` | ✅ supported |
| `engrave` along a polyline (no cutter-radius compensation) | ✅ supported |
| `engrave` closed polyline (`closed`) | ✅ supported |
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
   `diameter / 2`, closing all four edges at every depth level. With holding
   tabs the perimeter is parameterised from the lower-left corner and `count`
   tabs are centred in equal shares of it. Over each tab interval the cutter
   lifts to `z + height` (capped at `safe_z`), rapids along the perimeter to the
   far edge of the tab and plunges back to `z`; the intervals between tabs are
   cut at the cutting feed, including any corners they span. Every level
   therefore emits exactly `count` lifts and `count` tab-end plunges, and no
   cutting move crosses a tab.
5. **Pocket.** The raster runs between `x + r … x + width − r` and
   `y + r … y + height − r`; the validator rejects a pocket narrower than the
   tool so the shrunk rectangle can never invert.
6. **Spiral pocket.** The first closed ring is the pocket shrunk by the cutter
   radius; every next ring is inset one `stepover` on all four sides. Rings are
   emitted until the uncut core inside a ring is no wider or taller than the
   tool diameter; at that point the opposing tool passes overlap and clear the
   core, so no further closed ring is needed. Rings close at the cutting feed and
   the cutter advances to the next ring at the cutting feed without lifting. The
   report adds `strategy` and `loopCount` for spiral operations only; `loopCount`
   counts closed loops over every depth level, and cut/rapid length and time come
   from the generated moves. Islands are not supported by `spiral`.
7. **Circular pocket.** `shape: "circle"` treats `width` as the diameter and
   requires `height` equal to `width`. With pocket radius `R = width / 2` and
   cutter radius `r`, the first ring has radius `R − r`; every next ring shrinks
   by `stepover` until the radius is no longer positive. Each ring is
   approximated by 72 cutting segments, closes at the cutting feed and the
   cutter advances to the next ring at the cutting feed without lifting. A final
   cutting move reaches the centre so the core is always cleared. Every ring
   point is at most `R − r` from the centre, so no cutting point leaves the
   pocket or comes closer than `r` to its wall. `circle` supports neither an
   explicit `strategy` nor islands. The report adds `shape: "circle"` and
   `loopCount`, which counts the closed rings over every depth level.
8. **Island-aware pocket.** Every island is expanded by the cutter radius. On
   each raster line the expanded islands become keep-out intervals that split
   the line into cut spans; only spans outside every keep-out zone are cut, so
   every cutting point — including the span endpoints — stays at least `r` from
   an island. The tool retracts to the clearance plane before crossing a
   keep-out zone and plunges again on the far side. The report records
   `islandCount`, `skippedLength` and `skippedSegments`; all three are `0` when
   an operation has no islands.
9. **Drilling.** Each hole is a rapid to the centre, a plunge to
   `height − depth`, and a rapid retract. G-code uses `G81` (or `G83` with `Q` for
   peck) at the clearance plane `R` and cancels the cycle with `G80`.
10. **Engraving.** Every depth level rapids to the first path point at the
    clearance plane, plunges at `plungeFeed` and connects each consecutive pair
    of path points with one cutting move. With `closed: true` the level also
    cuts from the last point back to the first, so the polyline forms one
    closed loop; the last point may therefore not already coincide with the
    first. The tool then rapids back to the clearance plane above the last
    point. The cutter follows the polyline centre line, so no cutter-radius
    compensation is applied. Adjacent duplicate points are rejected by
    validation instead of being silently skipped. The closing cut counts toward
    `cutLength` and the cycle time like every other cutting move.
11. **Metrics.** Cutting length and time use the cutting feed; rapid length and
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
| `operation/invalid` | depth, stepdown, stepover, strategy, geometry, holes, peck, tabs, `closed` or an engrave polyline are invalid; an engrave polyline needs at least two points, may not repeat an adjacent point and, when `closed`, may not end on its first point; `closed` is valid only for `engrave`; tabs are valid only for `contour`, need `count ≥ 1`, a positive `width` and `height`, `height ≤ depth` and `count × width` below the profile perimeter `2 × (width + height)` |
| `operation/duplicate-id` | an operation id is declared twice |
| `operation/unknown-tool` | an operation references a missing tool |
| `operation/island-invalid` | an island coordinate is not finite or its width/height is not positive |
| `operation/island-outside-pocket` | an island is not fully inside its pocket |
| `operation/island-too-close` | an island expanded by the cutter radius does not fit inside the pocket shrunk by the cutter radius |
| `operation/island-overlap` | two expanded islands overlap, so the cutter cannot pass between them |
| `operation/strategy-island` | a `spiral` pocket declares islands, or a `circle` pocket declares islands, which the strategy does not support |
| `operation/strategy-circle` | a `circle` pocket declares an explicit `strategy`, which only `rect` pockets accept |
| `operation/circle-size` | a `circle` pocket has `height` different from `width` (the diameter) |
| `path/outside-stock` | a profile, pocket, hole or engrave path point leaves the stock |

Warnings (`tool/none`, `operation/none`, `tool/duplicate-number`, contour offset
outside the stock) do not make `ok` false.

## Tests

| File | Asserts |
|------|---------|
| `tests/toolpath.rs` | depth levels, closed contour, pocket containment, drill plunges, determinism |
| `tests/islands.rs` | island clearance, multi-island geometry, island validation codes, no-island byte identity, determinism, save/reopen |
| `tests/spiral.rs` | spiral containment and wall clearance, stepover ring spacing, loop count, metrics, determinism, raster byte identity, invalid strategy, spiral+islands rejection, save/reopen, G-code |
| `tests/circle.rs` | circular containment and wall clearance, stepover ring spacing, loop count, metrics, determinism, rect byte identity, invalid shape, circle size mismatch, circle+strategy and circle+islands rejection, save/reopen, G-code |
| `tests/engrave.rs` | polyline plunge and cut count, cut length, multi-level stepdown, closed square perimeter and extra move, open/closed move difference, closed first/last coincidence rejection, non-engrave `closed` rejection, default document byte identity, validation codes, G-code, empty `path` serialization |
| `tests/tabs.rs` | tab lift and plunge counts per level, tab positions and heights, validation codes, no-tab output identity, G-code |
| `tests/gcode.rs` | header/footer, `T<n> M6`, `G81`/`G80`, clearance plane, byte identity |
| `tests/validation.rs` | every error code above and warning-only documents |
| `tests/session.rs` | create, edit, reopen, export, in-use tool, atomic save-as |
| `tests/smoke.test.ts` (TypeScript) | session lifecycle, argument errors and island pocket reporting (skipped without the addon) |

## Limitations

- Rasters are axis-aligned and spiral pockets are concentric rectangles; there
  is no trochoidal, adaptive or arc-following strategy. Circular pockets are
  approximated by 72 linear segments per ring, not by `G2`/`G3` arcs.
- Spiral pockets do not support islands and reject them with
  `operation/strategy-island`; use the raster strategy when islands are needed.
- Circular pockets support only concentric rings: an explicit `strategy` is
  rejected with `operation/strategy-circle` and islands with
  `operation/strategy-island`.
- Pockets are rectangles or full circles; islands are rectangles inside a
  rectangular pocket. Arbitrary pocket outlines, fillets and adaptive corner
  clearing are not supported.
- Contours are rectangles with a constant cutter-radius offset; no arcs or
  lead-in/lead-out moves. Holding tabs are evenly spaced along that rectangle
  only: individual tab positions, tab shapes other than a chord of the profile
  and a separate tab-removal pass are not supported.
- Engraving follows the supplied polyline exactly: no cutter-radius
  compensation, curve fitting, tangency, lead-in or lead-out moves. A closed
  polyline only adds a straight cut back to its first point. Holes and islands
  are not supported by an engrave operation.
- The validator checks geometry against the stock, not against fixtures, clamps
  or machine travel limits.
- Cycle time is a kinematic estimate, not a machine simulation.
