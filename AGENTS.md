# AGENTS.md

Guidance for agents working in the OpenCAM repository.

## Purpose

OpenCAM is a 2.5-axis CAM product. A document is one `.ocam` JSON file; the
product exposes it through a Rust session, a Node-API addon, a TypeScript SDK and
the `open-cam` CLI.

## Documentation map

| File | Audience |
|------|----------|
| [README.md](README.md) | Product landing page |
| [projects/packages/open-cam/README.md](projects/packages/open-cam/README.md) | npm registry page |
| [documentation/architecture.md](documentation/architecture.md) | Module boundaries and data flow |
| [documentation/capabilities.md](documentation/capabilities.md) | Strategies, algorithms, tests, limitations |
| [documentation/cli.md](documentation/cli.md) | Command and flag reference |
| [documentation/development.md](documentation/development.md) | Build, test, native staging, release |
| [documentation/README.md](documentation/README.md) | Maintainer doc index |

## Architecture invariants

1. **The Rust session is the only implementation.** Document I/O, editing,
   validation, toolpath generation, G-code and reports live in
   `projects/crates/cam-model`. Do not reimplement any rule elsewhere.
2. **The Node-API layer maps values only.** Each export accepts a path or JSON
   request and returns a JSON string. It must not add defaults that change
   results.
3. **The TypeScript SDK/CLI must not duplicate domain logic.** It parses
   arguments, calls native functions, formats output and maps error codes to exit
   codes (`capability/*` → `2`, other failures → `1`).
4. **The capability matrix is the only support claim.** If a feature is not
   marked supported in `documentation/capabilities.md` and `CAPABILITIES_JSON`,
   it is not supported, regardless of an API name.
5. **Unsupported work fails explicitly.** Return `capability/not-implemented`
   instead of approximating silently.
6. **A document is only written when it is machineable.** Validate the candidate
   document before persisting; return the first error unchanged.
7. **Edits persist atomically.** Every edit writes through the temporary-file and
   rename path. There is no undo stack.
8. **Output stays deterministic.** No timestamps and no randomised ordering in
   toolpaths, G-code or reports.
9. **Units are millimetres.** Feeds are mm/min, spindle speeds are rpm, and no
   conversion happens.

## Verified commands

```shell
pnpm install
pnpm run build:ts
pnpm --filter @wae/open-cam run typecheck
pnpm --filter @wae/open-cam run test

cargo fmt -p cam-model -p open-cam-napi
cargo test -p cam-model -p open-cam-napi --offline
cargo clippy -p cam-model -p open-cam-napi --offline --all-targets -- -D warnings
```

`pnpm run build` performs the full native → TypeScript → staging build.
`pnpm run build:napi` builds only the addon and `pnpm run stage:native` copies it
into the platform packages. The addon is not committed.

## Change checklist

- Search the repository before assuming a command exists; verify flags against
  `projects/packages/open-cam/src/cli.ts` and `projects/crates/cam-model/src/bin/ocam.rs`.
- Add or update a test for every behavioural change.
- Keep the Rust session, Node-API surface, SDK types and CLI in sync when a
  strategy or field is added or removed.
- Update `documentation/capabilities.md`, `documentation/cli.md` and
  `CAPABILITIES_JSON` when the support claim changes.
- Keep tests in `tests/`; never place a test beside `src/`.
- Run the checks above plus `git diff --check` before committing.
- Stage explicit paths; never `git add -A`. Do not push unless asked.

## Commit style

Use a one-line subject with an emoji prefix:

- `✨ Add ...` — new capability or user-visible feature
- `🐛 Fix ...` — bug fix
- `📝 ...` — documentation, comments or prose

Keep the subject imperative and specific. If a change spans documentation and
behavior, split it into separate commits.
