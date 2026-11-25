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
| `pocket` islands (rectangular and circular) | ✅ supported |
| `pocket` circle (concentric ring) clearing | ✅ supported |
| `pocket` circular ring arc output (`arcs: "arc"`) | ✅ supported |
| `pocket` wall finish pass (`finish`) | ✅ supported |
| `drill` with `G81` and peck `G83` | ✅ supported |
| `drill` bottom dwell (`dwell`, `G82`) | ✅ supported |
| `drill` grid and circular hole patterns (`pattern`) | ✅ supported |
| `engrave` along a polyline (no cutter-radius compensation) | ✅ supported |
| `engrave` closed polyline (`closed`) | ✅ supported |
| `contour`, `pocket` and `engrave` ramp entry (`ramp`) | ✅ supported |
| multi-pass Z depth (`stepdown`) | ✅ supported |
| raster spacing (`stepover`) | ✅ supported |
| toolpath report (moves, cut/rapid length, cycle time) | ✅ supported |
| cycle-time estimate report (cut/plunge/retract/traverse/tool-change split) | ✅ supported |
| tool-grouped operation sequencing (fewest tool changes) | ✅ supported |
| material-based spindle and feed recommendation (`feeds`) | ✅ supported |
| operation list report (fields and per-operation motion time) | ✅ supported |
| partial operation update (tool, name, depth, stepdown, stepover, peck, dwell) | ✅ supported |
| operation deletion (self-contained operations, last operation allowed) | ✅ supported |
| operation duplication (every field copied, inserted after the source, generated `op-N`) | ✅ supported |
| operation reordering (final 0-based index, same-index no-op keeps the revision) | ✅ supported |
| G-code post options (program number, line numbers, coolant `M8`/`M7`/`M9`) | ✅ supported |
| DXF import, ASCII 2D `LINE`, `LWPOLYLINE`, `POLYLINE`, `ARC` and `CIRCLE` → one `engrave` operation each | ✅ supported |
| DXF export, ASCII `LINE`, `LWPOLYLINE` and `CIRCLE` of the stock, outlines, islands and drill holes | ✅ supported |
| portable tool library (`open-cam-tools`) import and export | ✅ supported |
| tool table query with per-tool operation usage | ✅ supported |
| JSON / CSV / Markdown reports | ✅ supported |
| SVG toolpath drawing (stock, cuts, rapids, markers, legend, title block) | ✅ supported |
| PDF toolpath drawing (one A4 landscape page, fitted and centred) | ✅ supported |
| generic absolute-millimetre G-code | ✅ supported |
| 3D surfacing, STL/solid input | ❌ not implemented |
| adaptive clearing | ❌ not implemented |
| turning | ❌ not implemented |
| four-axis and five-axis work | ❌ not implemented |
| STEP import | ❌ not implemented |
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
   keep-out zone and plunges again on the far side. An island is a rectangle by
   default; `shape: "circle"` is a disc of diameter `width` centred in its
   bounding box (`height` must equal `width`) and blocks the chord of its
   keep-out circle on each line. The report records `islandCount`,
   `skippedLength` and `skippedSegments`; all three are `0` when an operation
   has no islands.
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
16. **Operation sequencing.** The plan groups the operations by tool: each
    distinct tool keeps the position of its first appearance and the operations
    inside a tool keep their document order, so the proposal is stable and
    deterministic. The report counts adjacent pairs that use different tools
    before and after the change, so alternating tools collapse to one change per
    distinct tool. `unchanged` is true when the proposal equals the document
    order, which happens when each tool is already contiguous. Applying the plan
    rewrites the document only when the order changes: an already grouped
    document keeps its revision, otherwise the reordered document is validated
    and saved atomically and the report carries the new revision. Sequencing
    changes no operation field, move or feed.
17. **G-code post options.** `programNumber`, `lineNumbers`, `lineIncrement` and
    `coolant` change only the text around the moves. `programNumber` emits
    `O<n>` immediately after the leading `%`; `lineNumbers` prefixes every line
    strictly between the two `%` markers with a sequential `N` word starting at
    `lineIncrement` and rising by it; `coolant` emits `M8` (`flood`) or `M7`
    (`mist`) after the spindle start of every operation and closes it with `M9`
    before `M5`. Units `G21` and absolute mode `G90` stay fixed. Every default
    emits exactly the program of the plain writer, and no option changes a
    coordinate, a feed, an arc or a drilling cycle. A program number of zero, a
    line increment of zero or an unknown coolant reports `post/invalid`.
18. **Drill patterns.** A `pattern` replaces the explicit `holes` list; a drill
    that declares both reports `operation/invalid`. A `grid` starts at the
    operation anchor `(x, y)` and emits `columns × rows` holes in row-major
    order: `columns` holes along X spaced `spacingX` and `rows` along Y spaced
    `spacingY`. A `circular` layout centres `count` holes on `(x, y)`, places
    the first at angle 0 (+X) and runs counter-clockwise on the circle of
    `radius`, so every generated centre is exactly `radius` from the anchor.
    Generated holes feed the same drilling cycle, peck and dwell rules, stock
    check, toolpath report and G-code as an explicit list: one cycle line per
    hole at the same clearance plane `R`, cancelled once with `G80`. The
    pattern itself stores no feed, depth or clearance, and an omitted pattern
    keeps the explicit hole list byte-identical.
19. **Feeds recommendation.** `feeds` looks up one of six materials and returns
    a spindle speed and cutting feed for one tool diameter and flute count. The
    material table fixes a cutting speed in metres per minute — `aluminium` 300,
    `brass` 200, `steel` 120, `stainless` 60, `plastic` 400, `wood` 500 — and a
    chip load per tooth by diameter band: at most 3, 6 and 12 mm, then a final
    band for larger tools. The engine computes
    `spindle = 1000 × speed / (π × diameter)`, rounds it to the nearest 100 rpm
    and clamps it to `100 … 24000` rpm, then
    `feed = spindle × chip load × flutes` and `plunge = 30 % × feed`, each
    rounded to the nearest 10 mm/min with a floor of 10. `flutes` defaults to
    two and `"aluminum"` is accepted as an alias of `"aluminium"`, reported
    under the canonical name. The recommendation reads no document and writes
    nothing; it is advisory, not a machine or material simulation.
20. **Operation listing.** The report walks the operations in document order and
    reports one row each: `id`, `kind`, `tool`, `name`, `depth`, `stepdown`,
    `stepover`, the effective `strategy` (`raster` or `spiral` for a pocket,
    `null` for every other kind), the effective `shape` (`rect` or `circle` for
    a contour or pocket, `null` otherwise), the number of drilled `holes`
    (including a generated pattern, `0` for other kinds), `islandCount`, the
    pocket `finish` flag (`false` otherwise) and `estimatedSeconds`.
    `estimatedSeconds` is the kinematic motion time of that operation alone: it
    reuses the cycle-time engine at the default rapid feed with no tool-change
    overhead, so no second timing model exists. `usedTools` counts the distinct
    tools the operations reference. The report is read-only and deterministic;
    an unmachineable document reports its first validation error instead of a
    partial list.
21. **Partial operation updates.** An update changes only the supplied fields of
    one operation: `tool`, `name`, `depth`, `stepdown`, `stepover`, `peck` and
    `dwell`. At least one field is required, otherwise the request reports
    `operation/invalid`; an unknown operation reports `operation/unknown-id`.
    `depth`, `stepdown`, `stepover`, `peck` and `dwell` must be finite and
    greater than zero, and `peck` and `dwell` are accepted only by a `drill` and
    cannot be combined, which mirrors the add-time rule. A `tool` that is not
    declared reports `operation/unknown-tool`. The whole candidate document is
    validated with the same rules as `add` and `save`, including the stock,
    tool-diameter and geometry checks, before it is written. A rejected update
    writes nothing and keeps the revision; a successful update is saved
    atomically, bumps the revision once and returns the usual summary. This
    version does not change the kind, geometry coordinates, holes, islands,
    tabs, lead, ramp or arcs.
22. **Operation deletion.** A delete removes one operation and keeps the
    document order of the rest. An unknown id reports `operation/unknown-id`;
    removing the last operation is allowed and leaves a document with zero
    operations, which still reports only the `operation/none` warning. An
    operation is self-contained: its geometry, holes, islands, tabs, lead, ramp
    and arcs live inside the operation, and the tool table, sequencing plan and
    operation list are derived from the operation array on demand. No structure
    stores an operation id, so a delete never leaves a dangling reference and no
    cross-reference guard (`operation/in-use`) is required. The whole candidate
    document is validated with the same rules as `add`, `update` and `save`
    before it is written; a rejected delete writes nothing and keeps the
    revision, while a successful delete is saved atomically and bumps the
    revision once.
23. **Operation duplication.** A duplicate copies one operation and inserts the
    copy immediately after its source in document order. The copy keeps every
    field of the source — kind, tool, geometry, holes, islands, tabs, lead, ramp,
    arcs and name — and only the identifier changes; the name is never
    auto-suffixed. An unknown source reports `operation/unknown-id`. A supplied
    id must be non-empty and unused, otherwise the request reports
    `operation/invalid` (empty) or `operation/duplicate-id` (already declared).
    A missing id takes the smallest free `op-<n>` identifier, starting at
    `op-1`. The whole candidate document is validated before it is written, so a
    rejected duplicate writes nothing and keeps the revision; a successful
    duplicate is saved atomically, bumps the revision once and returns the usual
    summary.
24. **Operation reordering.** A reorder moves one operation to a final 0-based
    index in the operation array: the operation is removed from its current
    position and reinserted at `to`, so `0` puts it first and
    `operationCount − 1` puts it last. An unknown id reports
    `operation/unknown-id`; an index at or beyond the operation count reports
    `operation/invalid`. Moving an operation to its current index is a no-op:
    the request succeeds with `changed: false`, the file is not rewritten and the
    revision does not rise. Every other field and every other operation keeps
    its value, and ids never change. Operation order is the execution order:
    toolpaths, G-code, the cycle-time estimate, the operation list and the
    sequence plan all read the array when they are called, so nothing is stored
    to rebuild. The whole candidate document is validated with the same rules as
    `add`, `update` and `delete` before it is written; a rejected reorder writes
    nothing and keeps the revision, while a successful move is saved atomically,
    bumps the revision once and returns a report with `changed: true` and the
    saved revision.
26. **Vector drawings.** `export svg` and `export pdf` project the stock and the
    moves of the toolpath report onto one sheet; no toolpath is regenerated.
    The stock rectangle is drawn with its width × depth × height label, every
    cutting move becomes a polyline per operation, rapid `G0` moves become light
    dashed lines (a vertical rapid has no XY extent and is skipped), each plunge
    point becomes a dot and the first non-rapid move of an operation gets a ring.
    Pocket islands are drawn as their nominal outlines (rect as a closed
    polygon, circle as a circle) in a dedicated slate colour below the cutting
    moves, and every contour holding tab is marked by a thick burnt-orange
    segment: consecutive lifted rapid chords of one tab merge into one marker
    and repeats from the remaining Z levels are dropped. A tab whose lift is
    capped at the clearance plane is not distinguishable from a normal rapid
    and stays unmarked. All dashed rapids form one underlay group painted below
    the operation groups, so they never cover a cutting polyline.
    An `arc` move is sampled at the engine's ring resolution (72 chords per turn)
    from its `from`, `to` and `centre`, and the exact move endpoint is appended,
    so the drawing matches the linear ring output without a new tolerance.
    Operation colours follow the document index through a fixed eight-colour
    palette, so equal documents produce equal colours. The legend explains the
    cutting, rapid and plunge/start symbols, adds island-outline and holding-tab
    rows only when the document has them, and names every operation with its
    kind, id and tool diameter/radius. The title block carries the document id
    (file stem), revision, units, operation count, estimated time and scale.
    SVG uses a millimetre `viewBox` at 8 px per millimetre. PDF is one A4
    landscape page (842 × 595 pt) with Helvetica, `1 mm = 2.834646 pt`, fitted
    and centred above the legend and title block; the title block reports the
    resulting percentage of 1:1. Coordinates are rounded to three decimals and
    `-0` is normalised, operations keep document order and no timestamp is
    written, so repeated runs are byte-identical. An empty document still
    produces a valid empty sheet with the stock and title block. PDF text is
    ASCII-only: every non-ASCII character becomes `?` and is counted in the
    report's `dropped` field. SVG is XML with escaped text and keeps Unicode.
27. **DXF import.** `import dxf` reads the `ENTITIES` section of an ASCII DXF
    file and turns every supported 2D entity into one `engrave` operation that
    references a fixed `engrave` tool (0.2 mm, 12 000 rpm, 300 mm/min feed,
    100 mm/min plunge). Supported entities are `LINE`, `LWPOLYLINE` (including
    the closed flag), `POLYLINE` with its following `VERTEX` records until
    `SEQEND`, `ARC` and `CIRCLE`; Z coordinates are ignored. Arcs and circles are
    flattened with a deterministic sagitta tolerance (`tolerance`, default
    0.05 mm): the chord angle is `2·acos(1 − tolerance / radius)`, clamped to at
    most 4096 chords, and a circle always gets at least three. An `ARC` runs
    counter-clockwise from group 50 to 51 and an equal start and end angle is a
    full circle. The DXF layer names the operation: the name is the layer and the
    id is `<layer slug>-<n>` with the 1-based imported-entity ordinal, so the
    layer mapping is deterministic. The drawing is translated so its bounding
    box minimum is `(0, 0)`; stock width and depth follow the bounding box
    (minimum 1 mm) and height is `max(bbox depth, depth) + 5` mm, so the imported
    document always validates. `$INSUNITS` is reported, never converted:
    `4` is `millimetres`, `0` is `unitless`, a missing header is `unspecified`;
    any other value is reported as a loss and read as millimetres. Every
    section other than `HEADER` and `ENTITIES`, every unsupported entity
    (`SPLINE`, `ELLIPSE`, `HATCH`, `INSERT`/blocks, …), a non-zero `LWPOLYLINE`
    bulge and an entity with no usable geometry become a loss with a stable code
    and line number instead of an error. Adjacent duplicate vertices are merged
    (1 µm) and a closed polyline that repeats its first vertex at the end is
    normalised. The import creates a new document at revision `0` and writes it
    atomically once; an existing target reports `file/exists`, a malformed group
    code reports `import/dxf-malformed-group`, and a supported entity with
    missing or invalid values reports `import/dxf-malformed-entity`, each with
    its 1-based source line. The report carries the source and target paths,
    `insUnits`, `sourceUnits`, tolerance, depth, stepdown,
    entity/operation/skipped counts, every loss and the written document
    summary.
28. **DXF export.** `export --format dxf` writes the document's nominal
    geometry, not the toolpath, as an ASCII DXF file: a `HEADER` declaring
    `$INSUNITS` `4` (millimetres) and an `ENTITIES` section. The stock is a
    closed `LWPOLYLINE` on layer `STOCK`; every `engrave` path becomes a
    `LINE` (two open points) or a `LWPOLYLINE` whose flag `70` is `1` when
    closed; every `contour` and `pocket` outline becomes a closed rectangle
    `LWPOLYLINE` or, for `shape` `circle`, a `CIRCLE`; every pocket island
    becomes a closed rectangle `LWPOLYLINE` or, for a circle island, a
    `CIRCLE`; every `drill` hole becomes a
    `CIRCLE` of the tool radius; `face` adds no entity. Operation layers are
    the operation ids with every character outside `[A-Za-z0-9_-]` replaced
    by `-`. Coordinates are rounded to three decimals and `-0` is normalised,
    so the same document always writes the same bytes. The output stays inside
    the importer's subset: an engrave-only drawing round-trips through
    `import dxf` with the same points and closed flags.

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

A `drill` operation emits one `G81`, `G82` or `G83` line per hole and cancels the
cycle once with `G80`. A `pattern` expands to the same cycle lines, one per
generated hole, without changing the cycle, feed or clearance plane.

Contour lead arcs are the only interpolation the writer emits as `G2`/`G3`: the
absolute end coordinates plus `I`/`J` centre offsets from the arc start in the
XY plane, at the cutting feed. Every other move stays `G0` or `G1`.

`PostOptions` decorates that program without changing a move: a program number
becomes `O<n>` right after the leading `%`, line numbers prefix every interior
line with `N<n>` starting at `lineIncrement`, and coolant emits `M8` or `M7`
after `S… M3` and `M9` before `M5` of every operation. The defaults render the
plain program byte for byte. Units `G21`, absolute mode `G90`, the `G17` plane
and `G94` feed mode stay fixed, so the writer still never emits `G20`, `G91` or
a vendor dialect.

## Validation

| Code | Meaning |
|------|---------|
| `document/format`, `document/version`, `document/units` | the file is not an OpenCAM millimetre document |
| `stock/invalid` | a stock size is not finite and positive |
| `tool/invalid`, `tool/duplicate-id` | a tool is malformed or declared twice |
| `tool/library-invalid` | a tool library file is not JSON, not `open-cam-tools` version 1 in millimetres, or cannot be parsed |
| `operation/invalid` | depth, stepdown, stepover, strategy, geometry, holes, peck, `dwell`, tabs, `closed`, `ramp`, `finish`, `lead` or an engrave polyline are invalid; an engrave polyline needs at least two points, may not repeat an adjacent point and, when `closed`, may not end on its first point; `closed` is valid only for `engrave`; tabs are valid only for `contour`, need `count ≥ 1` and a positive `width` and `height`; a rectangular tab set also needs `height ≤ depth` and `count × width` below the profile perimeter `2 × (width + height)`, while a circular tab set needs `count × width` below the tool-centre circumference `2 × pi × (width / 2 + r)`; `ramp` is valid only for `contour`, `pocket` and `engrave` and must be finite and greater than zero; `finish` is valid only for `pocket` and is rejected when the pocket declares islands; `lead` is valid only for `contour`, needs a finite positive `radius` and a `direction` of `cw` or `ccw`, and is rejected together with `ramp`; `dwell` is valid only for `drill`, must be finite and greater than zero, and is rejected together with `peck`; `arcs` is valid only for a `circle` contour or pocket and must be `linear` or `arc`; `pattern` is valid only for `drill`, may not be combined with `holes`, must be `grid` or `circular`, a `grid` needs `rows ≥ 1`, `columns ≥ 1` and positive `spacingX` and `spacingY`, and a `circular` layout needs `count ≥ 1` and a positive `radius` |
| `operation/duplicate-id` | an operation id is declared twice, or a duplicate requests an id that is already declared |
| `operation/unknown-id` | an update, delete or duplicate names an operation the document does not declare |
| `operation/unknown-tool` | an operation references a missing tool |
| `operation/island-invalid` | an island coordinate is not finite, its width/height is not positive, a circle island has `height` different from `width`, or its `shape` is neither `rect` nor `circle` |
| `operation/island-outside-pocket` | an island is not fully inside its pocket |
| `operation/island-too-close` | an island expanded by the cutter radius does not fit inside the pocket shrunk by the cutter radius |
| `operation/island-overlap` | two expanded islands overlap, so the cutter cannot pass between them |
| `operation/strategy-island` | a `spiral` pocket declares islands, or a `circle` pocket declares islands, which the strategy does not support |
| `operation/strategy-circle` | a `circle` pocket declares an explicit `strategy`, which only `rect` pockets accept |
| `operation/circle-size` | a `circle` contour or pocket has `height` different from `width` (the diameter) |
| `import/invalid` | a `depth`, `stepdown` or `tolerance` import request value is not finite and greater than zero |
| `import/dxf-malformed-group` | the ASCII DXF has a group code that is not an integer, a blank code line, a group without a value or a section without a name or `ENDSEC`; the message carries the 1-based line |
| `import/dxf-malformed-entity` | a supported DXF entity is missing a required group or has a non-numeric or non-positive value; the message carries the 1-based line |
| `path/outside-stock` | a profile, pocket, hole or engrave path point leaves the stock |
| `post/invalid` | a G-code post option is invalid: a program number of zero, a line increment of zero or an unknown coolant |
| `feeds/invalid` | the feed recommendation request has a diameter that is not finite and positive, or a flute count of zero |
| `feeds/unknown-material` | the feed recommendation names a material outside the supported table |

Warnings (`tool/none`, `operation/none`, `tool/duplicate-number`, contour offset
outside the stock) do not make `ok` false.

## Tests

| File | Asserts |
|------|---------|
| `tests/toolpath.rs` | depth levels, closed contour, pocket containment, drill plunges, determinism |
| `tests/estimate.rs` | toolpath cycle-time parity, per-tool change accounting, rapid-feed scaling, drill plunge/retract/traverse split, invalid request codes, serde defaults, empty document, determinism |
| `tests/islands.rs` | island clearance, multi-island geometry, island validation codes, no-island byte identity, determinism, save/reopen |
| `tests/island_circle.rs` | circle island validation codes, chord clearance and rapid crossings, touching circles accepted, shape round trip with unshaped rectangles, circle islands as `CIRCLE` in the DXF export |
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
| `tests/patterns.rs` | grid row-major expansion and `G81` coordinates, circular angles and centre distance, pattern/hole exclusivity, non-drill rejection, invalid grid and circular geometry, unknown kind, generated hole outside the stock, `G83` per generated hole, explicit-hole byte identity, omitted fields, determinism and round trip |
| `tests/feeds.rs` | documented formula per material and diameter band, spelling alias, default flute count, spindle and feed clamps, unknown material names, invalid diameter and flute count, session parity, serde defaults and determinism |
| `tests/operation_list.rs` | document order, every row field, effective strategy and shape, pattern and explicit hole counts, island and finish flags, per-operation motion time parity with the cycle-time engine, read-only revision and file, determinism |
| `tests/operation_update.rs` | partial updates per field, untouched fields and operations, tool-usage change, invalid values, peck/dwell kind and combination rules, unknown id and tool, whole-document validation, revision bump only on success, unchanged file after failure, round trip, determinism and the unsaved candidate |
| `tests/operation_delete.rs` | delete order and revision bump, final operation leaving an empty valid document, unknown id, failed validation leaving the file byte-identical, tool-table and sequence references after a delete, round trip, determinism and the unsaved candidate |
| `tests/operation_duplicate.rs` | every field copied per kind, insertion after the source, supplied and generated `op-N` ids, empty and clashing ids, unknown source, failed validation leaving the file byte-identical, revision bump, round trip, determinism and the unsaved candidate |
| `tests/operation_reorder.rs` | move up, move down and to the first and last index, every field preserved, same-index no-op without a write or revision bump, out-of-range and unknown-id codes, failed validation leaving the file byte-identical and the revision unchanged, derived reports following the new order without changing ids, determinism and round trip |
| `tests/gcode.rs` | header/footer, `T<n> M6`, `G81`/`G80`, clearance plane, byte identity |
| `tests/post.rs` | default byte identity, empty request defaults, `O` program number placement, line numbering and increment, `M8`/`M7`/`M9` pairing, unchanged moves, `post/invalid` codes and determinism |
| `tests/sequence.rs` | stable tool grouping, before/after tool-change counts, per-tool order, single-tool and empty documents, source immutability, applied revision and idempotent apply, determinism |
| `tests/validation.rs` | every error code above and warning-only documents |
| `tests/session.rs` | create, edit, reopen, export, in-use tool, atomic save-as |
| `tests/dxf.rs` | LINE, closed LWPOLYLINE, POLYLINE vertices, ARC and CIRCLE flattening, segment counts and exact endpoints, sagitta tolerance, layer-derived ids, derived stock, loss reporting, malformed groups and entities with line numbers, units, determinism, revision 0 and existing-target rejection |
| `tests/dxf_export.rs` | engrave round trip with points and closed flags, two-point `LINE`, stock-only empty document, circle outlines and drill holes as `CIRCLE`, layer sanitisation, byte determinism, `dxf` format parsing with `step` still unimplemented |
| `tests/drawing.rs` | SVG structure, `viewBox`, stock/operations/legend/title block, XML escaping, rapid dashes and cut polylines, rapid underlay below cuts, island outlines with legend row (absent without islands), merged tab markers with legend row, start and plunge markers, PDF 1.4 MediaBox and single page, determinism, empty document and PDF-only non-ASCII loss |
| `tests/smoke.test.ts` (TypeScript) | session lifecycle, argument errors, island pocket reporting, contour lead arcs, `G3` ring output, the tool library, G-code post options, operation sequencing, drill patterns and feed recommendations (skipped without the addon) |
| `tests/dxf.test.ts` (TypeScript) | DXF import report and loss list, written engrave operations, existing-target rejection, CLI success output and CLI malformed-group failure (skipped without the addon) |
| `tests/drawing.test.ts` (TypeScript) | SDK SVG/PDF rendering and loss reporting, `exportDocument('svg')` byte parity, CLI stdout drawing and `--out` report (skipped without the addon) |

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
- Pockets are rectangles or full circles; islands are rectangles or circles inside a
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
- DXF import is a deliberately small ASCII subset: no binary DXF, no block or
  `INSERT` expansion, no `SPLINE`/`ELLIPSE`/`HATCH` approximation, no bulge
  arcs, text, dimensions or 3D geometry, and no unit conversion. Every supported
  entity becomes a centre-line `engrave` operation with one fixed tool; layers
  only name operations and create no tools, depths or feeds. Imported geometry
  is translated so the bounding-box minimum is `(0, 0)` and the stock is derived
  from it, so the original drawing coordinates are not preserved.
- DXF export writes nominal geometry, not toolpaths: no cutter offset, no
  raster or ring moves, no tabs, leads or ramps. Circular outlines and drill
  holes are exported as true `CIRCLE` entities, no `ARC` entity is emitted and
  the file has no `TABLES` section, so layers are not declared.
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
- G-code post options cover a program number, line numbering and coolant only.
  There are no configurable header/footer templates, no inch output (`G20`), no
  incremental mode (`G91`), no tool-length compensation, subprograms, probing or
  vendor dialects. Every option leaves the generated moves, feeds and cycles
  untouched.
- Operation sequencing groups by tool alone: tools keep their first-appearance
  order and the operations inside a tool keep their document order. It does not
  shorten travel, merge operations, reorder for fixturing or feature
  dependencies, and it never changes an operation field other than the position
  in the list.
- Operation listing is read-only and a partial update is deliberately narrow: it
  changes only `tool`, `name`, `depth`, `stepdown`, `stepover`, `peck` and
  `dwell`, never the kind, geometry coordinates, holes, islands, tabs, lead,
  ramp or arcs. An update cannot clear an optional field, reorder operations or
  merge two operations. `estimatedSeconds` is kinematic motion time of one
  operation without tool-change overhead, not a machine simulation.
- A delete removes exactly one operation and an update cannot reorder;
  duplication is the only insertion primitive and it always places the copy
  directly after its source, while `operation reorder` moves one existing
  operation to one final 0-based index. Sequencing groups the whole document by
  tool instead. There is no multi-select reorder or delete, no cascade cleanup
  (operations reference nothing else), no undo stack and no automatic name
  suffix on a duplicate.
- Drilling takes an explicit hole list or one generated pattern: a rectangular
  grid (`columns × rows`) or a full circle of evenly spaced holes. A pattern
  replaces the list rather than extending it, and there are no staggered,
  sub-grid, bolt-circle arc or per-hole depth, feed or clearance overrides.
- `feeds` is one advisory lookup for one tool in one material. It uses a fixed
  six-material surface-speed and chip-load table, ignores machine rigidity,
  coolant, radial and axial depth of cut and chatter, and is never stored in the
  document or applied to a tool automatically.
- Cycle time is a kinematic estimate, not a machine simulation.
- SVG and PDF export are preview drawings, not dimensioned shop drawings: they
  carry no dimensions, tolerances, annotations, multiple views, layers, hidden
  lines or raster images. Only the stock outline, the generated XY toolpath
  (projected; Z is not drawn), rapid traverses, plunge/start markers, a legend
  and a title block are shown. Arc moves are sampled as polylines at 72 chords
  per turn; the drawing does not emit SVG/PDF arc operators. PDF text uses
  base-14 Helvetica and is ASCII-only, so non-ASCII operation ids or document
  names become `?` (reported as `dropped`); SVG keeps Unicode. SVG is a single
  `viewBox` at a fixed 8 px/mm and PDF is always one fitted A4 landscape page,
  so page size, scale, orientation and colours are not configurable, and a
  drawing never replaces the G-code or the JSON/CSV/Markdown reports.
