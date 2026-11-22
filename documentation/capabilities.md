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
| `contour` outside rectangular profile, cutter-radius offset | ✅ supported |
| `contour` outside circular profile (`shape: "circle"`) | ✅ supported |
| `contour` holding tabs (`tabs`, rectangles only) | ✅ supported |
| `pocket` rectangular raster clearing | ✅ supported |
| `pocket` spiral (concentric rectangle) clearing | ✅ supported |
| `pocket` islands (rectangular) | ✅ supported |
| `pocket` circle (concentric ring) clearing | ✅ supported |
| `pocket` wall finish pass (`finish`) | ✅ supported |
| `drill` with `G81` and peck `G83` | ✅ supported |
| `engrave` along a polyline (no cutter-radius compensation) | ✅ supported |
| `engrave` closed polyline (`closed`) | ✅ supported |
| `contour`, `pocket` and `engrave` ramp entry (`ramp`) | ✅ supported |
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
   cutting move crosses a tab. `shape: "circle"` turns the same operation into a
   circular outside profile: `width` is the diameter, `height` must equal
   `width` (otherwise `operation/circle-size`), and the tool centre follows the
   circle of radius `width / 2 + r` centred at `(x + width / 2, y + height / 2)`.
   The ring starts at angle 0 (+X) and runs counter-clockwise as 72 linear
   cutting segments, closing at the cutting feed. Every level rapids to the
   start at the clearance plane, enters with the plunge or the ramp and
   retracts to the clearance plane, exactly like the rectangular contour. Tabs
   are not supported on a circular contour and report `operation/invalid`; the
   report adds `shape: "circle"` and leaves `loopCount` absent.
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
11. **Ramp entry.** `ramp` is a horizontal length in millimetres, not an angle.
    When set, a Z level no longer enters with one vertical plunge. The cutter
    starts at the clearance plane above the start of the first cutting move of
    the level, covers exactly `ramp` millimetres horizontally along that first
    cutting direction at the cutting feed while descending linearly from the
    clearance plane to the level depth, folds back at both ends of the first
    segment when `ramp` is longer than it, and returns to the entry point at the
    level depth. The rest of the level path is therefore unchanged and its cuts
    keep their full length. Every ramp move is a `cut` move, so `cutLength`,
    cycle time and G-code include it. The descent ends exactly at the level
    depth, so no move goes below it. Only `contour` (rectangular, tabbed or
    circular), `pocket` (raster, spiral, circular or island-aware) and
    `engrave` accept a ramp; every other kind reports `operation/invalid`, as
    does a ramp that is not finite and positive. An island-aware pocket ramps
    only the first entry
    of each level and keeps the vertical re-entry of later spans; a contour
    keeps the vertical plunge at every tab end.
12. **Pocket wall finish.** `finish: true` cuts one extra closed loop along the
    final wall after the clearing moves of every depth level. The loop sits at
    the nominal cutter-radius offset, so every point is exactly `r` from the
    wall: the rectangle corners `(x + r, y + r) … (x + width − r, y + height −
    r)` for rectangular raster and spiral clearing, or the ring of radius
    `width / 2 − r` approximated by 72 segments for a circular pocket. The
    loop runs at the level depth and the cutting feed, is reached with one
    cutting move across the already cleared floor and adds no Z move, so the
    depth and cutter-radius semantics are unchanged. Every finish move counts
    toward `cutLength` and the cycle time. `loopCount` keeps counting clearing
    loops only, so the finish loop does not change it. Islands are not
    supported: a pocket with `finish: true` and islands reports
    `operation/invalid`. `finish: false` or an omitted `finish` keeps the
    clearing output byte-identical.
13. **Metrics.** Cutting length and time use the cutting feed; rapid length and
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
| `operation/invalid` | depth, stepdown, stepover, strategy, geometry, holes, peck, tabs, `closed`, `ramp`, `finish` or an engrave polyline are invalid; an engrave polyline needs at least two points, may not repeat an adjacent point and, when `closed`, may not end on its first point; `closed` is valid only for `engrave`; tabs are valid only for a rectangular `contour`, need `count ≥ 1`, a positive `width` and `height`, `height ≤ depth` and `count × width` below the profile perimeter `2 × (width + height)`; a circular contour rejects tabs and reports `operation/invalid`; `ramp` is valid only for `contour`, `pocket` and `engrave` and must be finite and greater than zero; `finish` is valid only for `pocket` and is rejected when the pocket declares islands |
| `operation/duplicate-id` | an operation id is declared twice |
| `operation/unknown-tool` | an operation references a missing tool |
| `operation/island-invalid` | an island coordinate is not finite or its width/height is not positive |
| `operation/island-outside-pocket` | an island is not fully inside its pocket |
| `operation/island-too-close` | an island expanded by the cutter radius does not fit inside the pocket shrunk by the cutter radius |
| `operation/island-overlap` | two expanded islands overlap, so the cutter cannot pass between them |
| `operation/strategy-island` | a `spiral` pocket declares islands, or a `circle` pocket declares islands, which the strategy does not support |
| `operation/strategy-circle` | a `circle` pocket declares an explicit `strategy`, which only `rect` pockets accept |
| `operation/circle-size` | a `circle` contour or pocket has `height` different from `width` (the diameter) |
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
| `tests/circle_contour.rs` | circular contour at exactly `width / 2 + r`, 72 counter-clockwise segments and closure per level, multiple levels, linear-only G-code, rectangular byte identity, tab rejection, circle-size mismatch, ramp depth, outside-stock, determinism and save/reopen |
| `tests/engrave.rs` | polyline plunge and cut count, cut length, multi-level stepdown, closed square perimeter and extra move, open/closed move difference, closed first/last coincidence rejection, non-engrave `closed` rejection, default document byte identity, validation codes, G-code, empty `path` serialization |
| `tests/tabs.rs` | tab lift and plunge counts per level, tab positions and heights, validation codes, no-tab output identity, G-code |
| `tests/ramp.rs` | no-ramp byte identity, non-vertical first move, ramp endpoint at the level depth and no overcut, cut length increase, pocket entry, invalid kind and value, determinism and round trip |
| `tests/finish.rs` | no-finish byte identity, one wall loop per level for raster, spiral and circular pockets, wall clearance and depth, invalid kinds and island rejection, cut length and loop count, determinism and round trip |
| `tests/gcode.rs` | header/footer, `T<n> M6`, `G81`/`G80`, clearance plane, byte identity |
| `tests/validation.rs` | every error code above and warning-only documents |
| `tests/session.rs` | create, edit, reopen, export, in-use tool, atomic save-as |
| `tests/smoke.test.ts` (TypeScript) | session lifecycle, argument errors and island pocket reporting (skipped without the addon) |

## Limitations

- Rasters are axis-aligned and spiral pockets are concentric rectangles; there
  is no trochoidal, adaptive or arc-following strategy. Circular pockets and
  circular contours are approximated by 72 linear segments per ring, not by
  `G2`/`G3` arcs.
- Spiral pockets do not support islands and reject them with
  `operation/strategy-island`; use the raster strategy when islands are needed.
- Circular pockets support only concentric rings: an explicit `strategy` is
  rejected with `operation/strategy-circle` and islands with
  `operation/strategy-island`.
- Pockets are rectangles or full circles; islands are rectangles inside a
  rectangular pocket. Arbitrary pocket outlines, fillets and adaptive corner
  clearing are not supported.
- Contours are rectangles or full circles with a constant cutter-radius offset;
  a circular contour is approximated by 72 linear segments, not by `G2`/`G3`
  arcs, and has no lead-in/lead-out moves. Holding tabs are evenly spaced along
  the rectangle only and a circular contour rejects them with
  `operation/invalid`: individual tab positions, tab shapes other than a chord
  of the profile and a separate tab-removal pass are not supported.
- Engraving follows the supplied polyline exactly: no cutter-radius
  compensation, curve fitting, tangency, lead-in or lead-out moves. A closed
  polyline only adds a straight cut back to its first point. Holes and islands
  are not supported by an engrave operation.
- A ramp entry is a straight-line descent along the first cutting direction of
  the level, folded back inside that first segment and returned to its start; it
  is not a helical, spiral or trochoidal entry and does not follow corners. It
  ramps only the first entry of a level, so island re-entries and contour
  tab-end plunges stay vertical, and it cannot be combined with `drill` or
  `face`.
- The wall finish pass is one closed loop at the nominal cutter radius. It is
  not a spring pass, does not offset further into the wall and does not follow
  islands: a pocket with islands and `finish: true` reports
  `operation/invalid`. It applies to `pocket` only and cannot be combined with
  `face`, `contour`, `drill` or `engrave`.
- The validator checks geometry against the stock, not against fixtures, clamps
  or machine travel limits.
- Cycle time is a kinematic estimate, not a machine simulation.
