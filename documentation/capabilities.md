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
| `contour` circular ring arc output (`arcs: "arc"`) | ✅ supported |
| `contour` holding tabs (`tabs`, rectangle or circle) | ✅ supported |
| `contour` tangential lead-in / lead-out arcs (`lead`, rectangle or circle) | ✅ supported |
| `pocket` rectangular raster clearing | ✅ supported |
| `pocket` spiral (concentric rectangle) clearing | ✅ supported |
| `pocket` islands (rectangular) | ✅ supported |
| `pocket` circle (concentric ring) clearing | ✅ supported |
| `pocket` circular ring arc output (`arcs: "arc"`) | ✅ supported |
| `pocket` wall finish pass (`finish`) | ✅ supported |
| `drill` with `G81` and peck `G83` | ✅ supported |
| `drill` bottom dwell (`dwell`, `G82`) | ✅ supported |
| `engrave` along a polyline (no cutter-radius compensation) | ✅ supported |
| `engrave` closed polyline (`closed`) | ✅ supported |
| `contour`, `pocket` and `engrave` ramp entry (`ramp`) | ✅ supported |
| multi-pass Z depth (`stepdown`) | ✅ supported |
| raster spacing (`stepover`) | ✅ supported |
| toolpath report (moves, cut/rapid length, cycle time) | ✅ supported |
| cycle-time estimate report (cut/plunge/retract/traverse/tool-change split) | ✅ supported |
| portable tool library (`open-cam-tools`) import and export | ✅ supported |
| tool table query with per-tool operation usage | ✅ supported |
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
   retracts to the clearance plane, exactly like the rectangular contour.
   Circular contours accept the same `tabs`: each tab is centred in an equal
   angular share at angle `2·pi·(k + 0.5) / count`, so the path never starts
   inside a tab, and covers an arc length of `width` along the tool-centre
   circle. The baseline segments are split at every tab entry and exit angle,
   so no cutting move crosses a tab interval. Over a tab the cutter lifts to
   `min(z + height, safe_z)`, rapids along the circle to the tab exit and
   plunges back to `z`. Tab widths are measured along the tool-centre path, so
   `count × width` must stay below its circumference
   `2 × pi × (width / 2 + r)`. The report adds `shape: "circle"` and leaves
   `loopCount` absent.
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
   `height − depth`, and a rapid retract. G-code uses `G81`, `G82` with `P` for a
   bottom dwell, or `G83` with `Q` for peck, at the clearance plane `R`, and
   cancels the cycle with `G80`. `dwell` is a pause in seconds at the bottom of
   the hole and adds no move, so the toolpath and the kinematic cycle time are
   unchanged. It is machine time, not motion, and the report does not estimate
   it. `dwell` applies only to `drill` and cannot be combined with `peck`,
   because `G83` has no dwell and the writer does not approximate it.
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
13. **Lead-in / lead-out arcs.** `lead` replaces the vertical entry plunge and
    the direct closing retract of every `contour` Z level with tangential
    half-circle arcs of radius `lead.radius`. The cutter rapids to the point
    diametrically opposite the profile start, plunges there, cuts one half
    circle and joins the profile exactly along its tangent at the start point;
    after the level loop closes it leaves through the mirror half circle at the
    closing point before retracting. The counter-clockwise rectangular loop
    touches the bottom edge on entry and the left edge on exit, so the two arcs
    sit on different normals and never retrace each other; a circular contour
    has one tangent at the start, so both arcs share their circle and cover
    opposite halves. `direction` is the rotation of both arcs: `cw` renders
    `G2`, `ccw` renders `G3`; for the counter-clockwise contour `cw` keeps the
    arcs outside the profile. Each arc is an `arc` move at the cutting feed
    carrying its centre and direction in the report, spans `pi × radius` and
    counts toward `cutLength` and the cycle time. Only `contour` accepts a
    lead, and it cannot be combined with `ramp`, which replaces the same entry
    plunge. An omitted `lead` keeps the vertical plunge and the direct retract,
    so existing output is byte-identical.
14. **Metrics.** Cutting length and time use the cutting feed; rapid length and
    time use a fixed 8000 mm/min traverse. Cycle time is the sum of both. A
    drill `dwell` is machine time and is not included. The `estimate` report
    splits the same moves into cutting, plunge, retract (vertical rapid lifts),
    traverse (other rapids) and tool-change time, charges one fixed overhead to
    the first operation using each distinct tool and accepts `rapidFeed` and
    `toolChangeSeconds` overrides; with the defaults its motion time equals the
    toolpath report's cycle time.
15. **Arc rings.** `arcs: "arc"` changes only the interpolation of the closed
    rings of a `circle` `contour` or `pocket`; `arcs: "linear"` and an omitted
    value keep the 72-chord approximation byte for byte. Every ring is emitted
    as counter-clockwise arcs of at most half a circle, so a full turn becomes
    two `G3` arcs and a tab-free interval between two tabs becomes as many arcs
    as its angular span needs. Ring radii, `stepover`, depth levels, feeds, ring
    transitions, tab lifts, ramp entries and `loopCount` are unchanged. Each arc
    contributes its true length `radius × sweep` to `cutLength` and the cycle
    time. `arcs` is valid only for a `circle` contour or pocket, and an unknown
    value reports `operation/invalid` like any other kind or shape that uses it.

## Tool library

A document keeps its tool table local. One `open-cam-tools` file is a portable
library: `format` `open-cam-tools`, `version` 1, `units` `mm` and a `tools` array
in the document's tool schema. Export writes the document table in document
order; import validates the whole file, then merges its tools and saves the
document atomically. A malformed file, an unsupported format, version or unit
set reports `tool/library-invalid`, a malformed tool reports `tool/invalid` and
a tool id declared twice — inside the file or against the document — reports
`tool/duplicate-id`. A rejected import leaves the document and its revision
untouched. The tool-table report (`open-cam-tool-table`, printed by `tool list`)
lists every declared tool in document order with `usedBy` (operation ids) and
`used`, plus `toolCount` and `unusedCount`.

## G-code

The program starts with `%`, a comment block, `G21 G90 G17 G94` and `G0 Z<safe>`.
Each operation emits a comment, `T<n> M6`, `S<spindle> M3`, the moves or cycle,
`G0 Z<safe>` and `M5`. The program ends with `G0 Z<safe>`, `M30`, `%`.

`safe_z` is `stock.height + 5` mm. Coordinates render with at most three decimals
and `-0` is normalised to `0`.

Contour lead arcs are the only interpolation the writer emits as `G2`/`G3`: the
absolute end coordinates plus `I`/`J` centre offsets from the arc start in the
XY plane, at the cutting feed. Every other move stays `G0` or `G1`.

## Validation

| Code | Meaning |
|------|---------|
| `document/format`, `document/version`, `document/units` | the file is not an OpenCAM millimetre document |
| `stock/invalid` | a stock size is not finite and positive |
| `tool/invalid`, `tool/duplicate-id` | a tool is malformed or declared twice |
| `tool/library-invalid` | a tool library file is not JSON, not `open-cam-tools` version 1 in millimetres, or cannot be parsed |
| `operation/invalid` | depth, stepdown, stepover, strategy, geometry, holes, peck, `dwell`, tabs, `closed`, `ramp`, `finish`, `lead` or an engrave polyline are invalid; an engrave polyline needs at least two points, may not repeat an adjacent point and, when `closed`, may not end on its first point; `closed` is valid only for `engrave`; tabs are valid only for `contour`, need `count ≥ 1` and a positive `width` and `height`; a rectangular tab set also needs `height ≤ depth` and `count × width` below the profile perimeter `2 × (width + height)`, while a circular tab set needs `count × width` below the tool-centre circumference `2 × pi × (width / 2 + r)`; `ramp` is valid only for `contour`, `pocket` and `engrave` and must be finite and greater than zero; `finish` is valid only for `pocket` and is rejected when the pocket declares islands; `lead` is valid only for `contour`, needs a finite positive `radius` and a `direction` of `cw` or `ccw`, and is rejected together with `ramp`; `dwell` is valid only for `drill`, must be finite and greater than zero, and is rejected together with `peck`; `arcs` is valid only for a `circle` contour or pocket and must be `linear` or `arc` |
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
| `tests/estimate.rs` | toolpath cycle-time parity, per-tool change accounting, rapid-feed scaling, drill plunge/retract/traverse split, invalid request codes, serde defaults, empty document, determinism |
| `tests/islands.rs` | island clearance, multi-island geometry, island validation codes, no-island byte identity, determinism, save/reopen |
| `tests/spiral.rs` | spiral containment and wall clearance, stepover ring spacing, loop count, metrics, determinism, raster byte identity, invalid strategy, spiral+islands rejection, save/reopen, G-code |
| `tests/circle.rs` | circular containment and wall clearance, stepover ring spacing, loop count, metrics, determinism, rect byte identity, invalid shape, circle size mismatch, circle+strategy and circle+islands rejection, save/reopen, G-code |
| `tests/circle_contour.rs` | circular contour at exactly `width / 2 + r`, 72 counter-clockwise segments and closure per level, multiple levels, linear-only G-code, rectangular byte identity, tab lift/rapid/plunge groups per level, tab intervals without cuts, on-circle points, closure, safe-z cap, single tab, tab fit and size validation, no-tab identity, ramp with tabs, circle-size mismatch, outside-stock, determinism and save/reopen |
| `tests/engrave.rs` | polyline plunge and cut count, cut length, multi-level stepdown, closed square perimeter and extra move, open/closed move difference, closed first/last coincidence rejection, non-engrave `closed` rejection, default document byte identity, validation codes, G-code, empty `path` serialization |
| `tests/tabs.rs` | tab lift and plunge counts per level, tab positions and heights, validation codes, no-tab output identity, G-code |
| `tests/ramp.rs` | no-ramp byte identity, non-vertical first move, ramp endpoint at the level depth and no overcut, cut length increase, pocket entry, invalid kind and value, determinism and round trip |
| `tests/lead.rs` | tangential lead-in/lead-out geometry, arc centre, radius and half-circle length, `G2`/`G3` lines with `I`/`J` centre offsets, mirrored `ccw` arcs, circular arcs covering opposite halves, cut length, tabs and stepdown combination, invalid kind, radius, direction and ramp rejection, no-lead byte identity, determinism and round trip |
| `tests/finish.rs` | no-finish byte identity, one wall loop per level for raster, spiral and circular pockets, wall clearance and depth, invalid kinds and island rejection, cut length and loop count, determinism and round trip |
| `tests/arcs.rs` | opt-in byte identity, two `G3` half arcs per pocket ring and contour level, tab intervals as arcs between the lifts, arc lengths below a half circle, validation codes, determinism and round trip |
| `tests/tools.rs` | tool-table usage, library export and import round trip, rejection of duplicate ids, bad format, version, units, malformed JSON, invalid tools and a missing file without touching the document |
| `tests/dwell.rs` | `G82`/`P` and `G80` per hole, no `G4`, no-dwell byte identity, peck and non-drill rejection, invalid values, unchanged toolpath moves, determinism and round trip |
| `tests/gcode.rs` | header/footer, `T<n> M6`, `G81`/`G80`, clearance plane, byte identity |
| `tests/validation.rs` | every error code above and warning-only documents |
| `tests/session.rs` | create, edit, reopen, export, in-use tool, atomic save-as |
| `tests/smoke.test.ts` (TypeScript) | session lifecycle, argument errors, island pocket reporting, contour lead arcs, `G3` ring output and the tool library (skipped without the addon) |

## Limitations

- Rasters are axis-aligned and spiral pockets are concentric rectangles; there
  is no trochoidal, adaptive or arc-following strategy. By default circular
  pockets and circular contours are approximated by 72 linear segments per ring;
  `arcs: "arc"` replaces the cutting rings with counter-clockwise `G3` arcs of at
  most a half circle, but no other move becomes an arc and no clockwise ring is
  emitted. Contour lead arcs stay the only `G2` interpolation.
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
  arcs. Holding tabs are evenly spaced along the path: rectangles measure tab
  widths along the perimeter, circles along the tool-centre circle. Individual
  tab positions, tab shapes other than a chord of the path and a separate
  tab-removal pass are not supported.
- A contour lead is exactly one tangential half circle at the level start and
  one at the closing point, always of radius `lead.radius` and spanning a full
  180°; straight tangent leads, quarter-circle leads, separate lead-in and
  lead-out radii, lead lengths and lead arcs on `pocket`, `drill` or engrave
  entries are not supported. A `ccw` lead places the arcs on the profile side
  of the tangent, and the validator does not check lead arcs against the stock
  or fixtures.
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
- A tool library carries tool data only: no holders, materials, vendor catalogs
  or per-operation overrides. Import is an id-based merge with no conflict
  resolution beyond `tool/duplicate-id`; a tool already declared in the document
  must be removed first.
- Cycle time is a kinematic estimate, not a machine simulation.
