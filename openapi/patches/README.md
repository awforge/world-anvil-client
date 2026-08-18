# OpenAPI patches

`cargo xtask openapi bundle` applies these patches in filename order to a
temporary copy of `../upstream`. The vendored source remains unchanged.

- `0001-normalize-upstream.patch` fixes invalid OpenAPI structures and combines
  the two required authentication headers.
- `0002-progenitor-compatibility.patch` fixes schemas and requests that
  Progenitor cannot generate.
- `0003-bodyless-error-responses.patch` removes typed error bodies because
  Progenitor 0.14 supports only one error-body type per operation. Status codes
  and descriptions are preserved.
- `0004-required-request-bodies.patch` marks every supported JSON request body
  as required by the OpenAPI contract.
- `0005-manuscript-error-responses.patch` adds the standard shared client-error
  responses omitted from the manuscript operations.

Add new corrections as sequentially numbered patches and keep each patch
focused on one concern.

## Authoring a patch

Create a disposable worktree containing the upstream snapshot with every
existing patch applied:

```console
cargo xtask openapi patch-worktree \
  --output target/openapi-edit
```

The output must be absent or an empty regular directory, and its parent must
already exist. The command initializes a standalone Git repository and stages
the patched source as its baseline. Edit the YAML files in that directory, then
generate the next patch from the unstaged changes:

```console
git -C target/openapi-edit diff \
  --binary \
  --no-ext-diff \
  --src-prefix=a/ \
  --dst-prefix=b/ \
  -- . > openapi/patches/0005-description.patch

cargo xtask openapi bundle
cargo xtask openapi check
```

Use `git diff`, not `git diff --cached`: the staged files are the baseline.
Explicit source and destination prefixes ensure the patch paths are relative to
`openapi/upstream`, regardless of the user's Git configuration.

To regenerate an existing patch, stop immediately before that patch. For
example, this materializes the upstream snapshot with patches `0001` through
`0003` applied:

```console
cargo xtask openapi patch-worktree \
  --before 0004-required-request-bodies.patch \
  --output target/openapi-edit
```

`--before` must exactly match an existing patch filename. The patch applier
currently supports modifications to text files; additions, deletions, renames,
and binary patches are rejected.
