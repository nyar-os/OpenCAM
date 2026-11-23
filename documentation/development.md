# Development

## Layout

```
Cargo.toml                     Rust workspace (cam-model, open-cam-napi)
package.json                   pnpm workspace scripts
wae.config.ts                  native targets and staging
projects/crates/cam-model      engine
projects/crates/cam-napi       Node-API addon
projects/packages/open-cam     TypeScript SDK and `open-cam` CLI
projects/packages/open-cam-*   per-platform addon placeholders
projects/sites/homepage        static product site
documentation/                 maintainer documentation
```

## Prerequisites

- Rust stable with `cargo`, `rustfmt` and `clippy`;
- Node.js 22.6 or newer;
- pnpm 10.28.

## Commands

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

## Native addon

`wae.config.ts` declares five targets (`win32-x64`, `darwin-x64`,
`darwin-arm64`, `linux-x64`, `linux-arm64`). Each target stages
`open-cam.<target>.node` into `projects/packages/open-cam-<target>`. The SDK
loads `@wae/open-cam-<platform>-<arch>`; set `OPEN_CAM_NATIVE` to an absolute
path to test a locally built addon.

## Tests

- Rust integration tests: `projects/crates/cam-model/tests/*.rs`.
- TypeScript tests: `projects/packages/open-cam/tests/*.test.ts`, compiled to
  `dist-test/` by `tsconfig.test.json`.
- `dist-test/` is ignored.

A behavioural change needs a test that fails before the change. A new capability
also updates `documentation/capabilities.md` and `CAPABILITIES_JSON`.

## Release

Tag `v*` and push. `.github/workflows/publish-npm.yml` builds every target,
stages the addon into the platform packages and publishes through npm Trusted
Publisher. `check-rust.yml` runs formatting, clippy and tests;
`check-typescript.yml` runs the frozen install, typecheck and build.

## Style

- Keep `cam-model` free of argument parsing and CLI text.
- Keep `cam-napi` free of domain rules.
- Keep the TypeScript layer free of toolpath rules.
- Comments explain non-obvious intent, not syntax.
