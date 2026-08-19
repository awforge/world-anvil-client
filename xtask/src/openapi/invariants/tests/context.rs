use crate::openapi::invariants::{validate_patch_0001, validate_patch_0002};

#[test]
fn patch_0001_reports_malformed_paths() {
    let error = validate_patch_0001(br#"{"paths": null}"#)
        .expect_err("patch 0001 must report malformed paths");

    assert!(
        error
            .to_string()
            .contains("[WA-OP-001] paths: paths must be an object"),
        "unexpected diagnostic: {error:#}"
    );
}

#[test]
fn later_patches_do_not_repeat_patch_0001_path_errors() {
    validate_patch_0002(br#"{"paths": null}"#)
        .expect("patch 0002 must not repeat patch 0001 path diagnostics");
}
