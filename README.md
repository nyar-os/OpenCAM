# OpenCAM

2.5-axis milling that stays in plain text: one JSON document, deterministic
toolpaths, and G-code you can read.

## 💡 What is OpenCAM?

OpenCAM turns a small machining job into a machine program. You describe a
rectangular stock, a tool table and a list of operations in one `.ocam`
document, then generate toolpaths and G-code from the command line or from
TypeScript. The same document always produces the same moves, byte for byte.

The engine implements four strategies:

- **face** — raster the whole stock top with a controllable stepover;
- **contour** — mill an outside rectangular profile, offset by the cutter radius;
- **pocket** — clear a rectangular pocket with a raster or spiral, or a circular
  pocket with concentric rings, inside the cutter radius;
- **drill** — emit `G81`/`G83` cycles over a list of hole centres.

Every strategy supports multi-pass Z depth through `stepdown`; facing and
pocketing take a `stepover`. The toolpath report carries explicit rapid, plunge
and cut segments with cutting length, rapid length and an estimated cycle time.

**OpenCAM is not:**

- a 3D surfacing or adaptive-clearing CAM — there is no STL/solid input;
- a post-processor library — the writer targets one generic absolute-millimetre
  mill (`G21 G90 G17 G94`) and does not emulate vendor dialects;
- a place where an unsupported feature silently degrades — an unimplemented
  route returns `capability/not-implemented` with exit code `2`.

## 🚀 Getting started

### 1. Point your agent at the repository

OpenCAM does not ship an agent skills package. Point your agent at the
documentation and the `open-cam` CLI:

```text
Use OpenCAM (`open-cam`) to face a 120 x 80 x 12 mm aluminium block with a
6 mm flat end mill, 1 mm deep, then drill two 5 mm holes 6 mm deep. Generate the
G-code and report the estimated cycle time. Read documentation/cli.md and
documentation/capabilities.md before claiming a result.
```

### 2. Install and run

```shell
pnpm add @wae/open-cam

pnpm exec open-cam create bracket.ocam
pnpm exec open-cam stock bracket.ocam --width 120 --depth 80 --height 12
pnpm exec open-cam tool add bracket.ocam --id t6 --diameter 6 --flutes 2 \
    --spindle 12000 --feed 1200 --plunge 300
pnpm exec open-cam operation add bracket.ocam --id face --kind face --tool t6 \
    --depth 1 --stepdown 0.5 --stepover 4
pnpm exec open-cam operation add bracket.ocam --id holes --kind drill --tool t6 \
    --depth 6 --stepdown 2 --holes "20,20;80,40" --peck 2
pnpm exec open-cam validate bracket.ocam
pnpm exec open-cam toolpaths bracket.ocam
pnpm exec open-cam gcode bracket.ocam --out bracket.nc
```

`create` starts from a 100 x 100 x 10 mm stock so the file is useful
immediately; `stock` replaces it. `validate` reports `{"ok": true, ...}` for a
machineable document. `open-cam capabilities` lists what the installed build
supports.

The SDK is the same engine with a typed API:

```ts
import { OpenCAM } from '@wae/open-cam';

const cam = new OpenCAM();
const document = await cam.create('bracket.ocam');
await document.setStock({ width: 120, depth: 80, height: 12 });
await document.addTool({ id: 't6', diameter: 6, spindle: 12000, feed: 1200, plungeFeed: 300 });
await document.addOperation({ id: 'face', kind: 'face', tool: 't6', depth: 1, stepdown: 0.5, stepover: 4 });
const report = await document.toolpaths();
console.log(report.estimatedSeconds);
await cam.dispose();
```

Exit code `1` means invalid input or an unmachineable document; `2` means the
requested capability is not implemented.

## ✨ Highlights

- **Four real strategies** — face, contour, pocket and drill, each with
  multi-pass depth control.
- **Deterministic output** — no timestamps; identical documents produce
  identical toolpaths and G-code.
- **Explicit geometry** — contour offsets by the cutter radius, pocket rasters,
  spirals and circular rings stay inside the shrunk boundary, drill holes are
  checked against the stock.
- **Honest validation** — a document that cannot be machined is rejected before
  it is written; each finding carries a stable code.
- **Machine-readable reports** — toolpaths as JSON, moves as CSV, a Markdown
  summary, or a G-code program with `T<n> M6`, `G81`/`G83` and `M30`.
- **One native core** — the Rust session owns every rule; the Node-API addon and
  the TypeScript SDK only map values.

## 🧪 Examples

- [Package README — CLI and SDK usage](projects/packages/open-cam/README.md)
- [Toolpath tests with exact move counts](projects/crates/cam-model/tests/toolpath.rs)
- [G-code header, cycles and determinism](projects/crates/cam-model/tests/gcode.rs)
- [Validation codes](projects/crates/cam-model/tests/validation.rs)
- [Session, atomic save and export](projects/crates/cam-model/tests/session.rs)
- [Capability matrix](documentation/capabilities.md)

## 📚 Documentation

| Document | Contents |
|----------|----------|
| [documentation/architecture.md](documentation/architecture.md) | Layers, invariants, data flow |
| [documentation/capabilities.md](documentation/capabilities.md) | Strategies, algorithms, tests and limitations |
| [documentation/cli.md](documentation/cli.md) | Every `open-cam` command, flag and example |
| [documentation/development.md](documentation/development.md) | Build, test, native staging and release |
| [projects/packages/open-cam/README.md](projects/packages/open-cam/README.md) | The npm package page |

## 📜 License

[Mozilla Public License 2.0](License.md)
