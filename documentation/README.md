# Documentation

Maintainer documentation for OpenCAM. Product narrative lives in the
[repository README](../README.md) and the registry page lives in
[projects/packages/open-cam/README.md](../projects/packages/open-cam/README.md).

| Document | Contents |
|----------|----------|
| [architecture.md](architecture.md) | Layers, module boundaries, data flow, error semantics |
| [capabilities.md](capabilities.md) | Strategies, algorithms, tests and limitations |
| [cli.md](cli.md) | Every `open-cam` command, flag and example |
| [development.md](development.md) | Build, test, native staging and release |

## Source of truth

1. `projects/crates/cam-model` owns the document model, validation, toolpath
   generation, G-code and every report.
2. `projects/crates/cam-napi` maps values only.
3. `projects/packages/open-cam` is the TypeScript SDK and CLI.
4. Capability claims live only in [capabilities.md](capabilities.md) and in
   `CAPABILITIES_JSON`.

## Tests

Rust integration tests live in `projects/crates/cam-model/tests/`; TypeScript
tests live in `projects/packages/open-cam/tests/`. Nothing is mixed into `src/`.
