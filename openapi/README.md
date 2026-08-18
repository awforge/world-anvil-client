# OpenAPI

`cargo build` generates the World Anvil client from
`world-anvil.openapi.json`. The generated Rust file is written to Cargo's
`OUT_DIR` under `target/` and included by `src/lib.rs`; do not edit it directly.

## Files

- `upstream/` contains the vendored World Anvil OpenAPI source. Do not edit it.
- `patches/` contains local corrections, applied in filename order.
- `world-anvil.openapi.json` is the generated bundle consumed by `build.rs`.
  Do not edit it directly.

Patch details are documented in [`patches/README.md`](patches/README.md).
Source information, licensing, and checksums are in
[`upstream/SOURCE.md`](upstream/SOURCE.md) and `upstream/SHA256SUMS`.

## Maintaining the specification

Normal Rust builds do not require Node.js. The `bundle` and `check` commands do;
install the pinned Redocly CLI first:

```console
npm ci
```

Available commands:

```console
cargo xtask openapi fetch
cargo xtask openapi bundle
cargo xtask openapi patch-worktree --output target/openapi-edit
cargo xtask openapi check
```

- `fetch` refreshes the vendored files from World Anvil.
- `bundle` applies and validates the patches, then rebuilds
  `world-anvil.openapi.json`.
- `patch-worktree` creates an editable, Git-backed copy of the patched source
  for authoring a new patch.
- `check` verifies checksums, reproducibility, and Rust code generation.

After `fetch`, review the upstream changes, update `upstream/SOURCE.md`, adjust
the patches if necessary, then run `bundle` and `check`.

See [`patches/README.md`](patches/README.md) for the patch-authoring workflow.

## Known limitation

Progenitor 0.14 cannot generate operations with different error-body types.
The compatibility patch keeps their status codes and descriptions but removes
their typed bodies. Generated errors still expose the HTTP status and headers.
