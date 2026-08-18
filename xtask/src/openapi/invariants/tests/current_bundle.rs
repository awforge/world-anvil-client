use std::{fs, path::Path};

use super::validate;

#[test]
fn committed_bundle_satisfies_every_semantic_invariant() {
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("openapi")
        .join("world-anvil.openapi.json");
    let specification = fs::read(&bundle)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", bundle.display()));

    validate(&specification).expect("the committed OpenAPI bundle must satisfy every invariant");
}
