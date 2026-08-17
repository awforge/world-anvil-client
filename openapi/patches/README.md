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

Add new corrections as sequentially numbered patches and keep each patch
focused on one concern.
