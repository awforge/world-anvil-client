use serde_json::{Map, Value, json};

use super::{MANUSCRIPT_OPERATION_IDS, validate_patch_0005};

const RULE_ID: &str = "[WA-CONTRACT-MANUSCRIPT-RESPONSES]";
const REQUIRED_ERROR_STATUSES: &[&str] = &["400", "401", "403", "404", "405", "422"];

fn document() -> Value {
    let statuses = standard_statuses();
    let paths = MANUSCRIPT_OPERATION_IDS
        .iter()
        .enumerate()
        .map(|(index, operation_id)| {
            (
                format!("/baseline/{index}"),
                json!({
                    "get": operation(operation_id, &["Manuscript"], &statuses)
                }),
            )
        })
        .collect::<Map<_, _>>();

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "manuscript response fixture",
            "version": "1.0.0"
        },
        "paths": paths
    })
}

fn extend_paths(specification: &mut Value, additional: Value) {
    specification["paths"]
        .as_object_mut()
        .expect("the fixture paths should be an object")
        .extend(
            additional
                .as_object()
                .expect("the additional paths should be an object")
                .clone(),
        );
}

fn responses(statuses: &[&str]) -> Value {
    Value::Object(
        statuses
            .iter()
            .map(|status| {
                (
                    (*status).to_owned(),
                    json!({ "description": format!("HTTP {status}") }),
                )
            })
            .collect::<Map<_, _>>(),
    )
}

fn operation(operation_id: &str, tags: &[&str], statuses: &[&str]) -> Value {
    json!({
        "operationId": operation_id,
        "tags": tags,
        "responses": responses(statuses)
    })
}

fn validate_value(specification: &Value) -> anyhow::Result<()> {
    let specification =
        serde_json::to_vec(specification).expect("the test fixture should serialize");
    validate_patch_0005(&specification)
}

fn validation_error(specification: &Value) -> String {
    let error = validate_value(specification).expect_err("the semantic invariant should fail");
    format!("{error:#}")
}

fn standard_statuses() -> Vec<&'static str> {
    let mut statuses = vec!["200"];
    statuses.extend(REQUIRED_ERROR_STATUSES);
    statuses
}

#[test]
fn manuscript_and_manuscript_subresource_operations_pass_with_the_required_subset() {
    let mut specification = document();
    let statuses = standard_statuses();
    extend_paths(
        &mut specification,
        json!({
            "/manuscript": {
                "get": operation("readManuscript", &["Manuscript"], &statuses)
            },
            "/manuscript_version/manuscript_parts": {
                "post": operation(
                    "listManuscriptPartsByManuscriptVersion",
                    &["Internal", "Manuscript Part"],
                    &statuses
                )
            }
        }),
    );

    validate_value(&specification).expect("all required manuscript error responses are present");
}

#[test]
fn additional_response_statuses_are_allowed() {
    let mut specification = document();
    let mut statuses = standard_statuses();
    statuses.extend(["201", "409", "429", "500"]);
    specification["paths"]["/manuscript_plot"] = json!({
        "patch": operation("updateManuscriptPlot", &["Manuscript Plot"], &statuses)
    });

    validate_value(&specification)
        .expect("the invariant requires a subset rather than an exact response set");
}

#[test]
fn each_required_error_status_is_enforced() {
    let complete = standard_statuses();

    for missing in REQUIRED_ERROR_STATUSES {
        let statuses = complete
            .iter()
            .copied()
            .filter(|status| status != missing)
            .collect::<Vec<_>>();
        let mut specification = document();
        specification["paths"]["/manuscript_beat"] = json!({
            "get": operation("readManuscriptBeat", &["Manuscript Beat"], &statuses)
        });

        let message = validation_error(&specification);
        assert!(
            message.contains(RULE_ID),
            "missing rule ID when response {missing} is absent: {message}"
        );
        assert!(
            message.contains(missing),
            "diagnostic does not identify missing response {missing}: {message}"
        );
    }
}

#[test]
fn a_range_or_default_does_not_replace_explicit_error_statuses() {
    let mut specification = document();
    specification["paths"]["/manuscript_tag"] = json!({
        "get": operation(
            "readManuscriptTag",
            &["Manuscript Tag"],
            &["200", "4XX", "default"]
        )
    });

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
    for status in REQUIRED_ERROR_STATUSES {
        assert!(
            message.contains(status),
            "diagnostic does not identify missing response {status}: {message}"
        );
    }
}

#[test]
fn a_new_manuscript_subresource_is_selected_by_its_tag() {
    let mut specification = document();
    specification["paths"]["/manuscript_comment"] = json!({
        "get": operation(
            "readManuscriptComment",
            &["Manuscript Comment"],
            &["200"]
        )
    });

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
}

#[test]
fn a_known_manuscript_operation_cannot_evade_the_rule_by_losing_its_tag() {
    let mut specification = document();
    specification["paths"]["/manuscript_beat"] = json!({
        "get": operation("readManuscriptBeat", &[], &["200"])
    });

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
}

#[test]
fn a_patched_manuscript_operation_cannot_disappear() {
    let mut specification = document();
    let operation_id = "readManuscriptBeat";
    let index = MANUSCRIPT_OPERATION_IDS
        .iter()
        .position(|candidate| *candidate == operation_id)
        .expect("the operation should be part of the baseline");
    specification["paths"]
        .as_object_mut()
        .expect("the fixture paths should be an object")
        .remove(&format!("/baseline/{index}"));

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
    assert!(
        message.contains(operation_id),
        "diagnostic does not identify the deleted operation: {message}"
    );
}

#[test]
fn unrelated_tags_and_similar_prefixes_are_out_of_scope() {
    let mut specification = document();
    extend_paths(
        &mut specification,
        json!({
            "/article": {
                "get": operation("readArticle", &["Article"], &["200"])
            },
            "/manuscripture": {
                "get": operation("readManuscripture", &["Manuscripture"], &["200"])
            }
        }),
    );

    validate_value(&specification)
        .expect("only the Manuscript tag family is governed by patch 0005");
}

#[test]
fn diagnostics_are_aggregated_in_deterministic_order() {
    let first_paths = json!({
        "/zeta": {
            "get": operation("readZeta", &["Manuscript Zeta"], &["200"])
        },
        "/alpha": {
            "get": operation("readAlpha", &["Manuscript Alpha"], &["200"])
        }
    });
    let second_paths = json!({
        "/alpha": {
            "get": operation("readAlpha", &["Manuscript Alpha"], &["200"])
        },
        "/zeta": {
            "get": operation("readZeta", &["Manuscript Zeta"], &["200"])
        }
    });
    let mut first = document();
    extend_paths(&mut first, first_paths);
    let mut second = document();
    extend_paths(&mut second, second_paths);

    let first_message = validation_error(&first);
    let second_message = validation_error(&second);

    assert_eq!(first_message, second_message);
    assert_eq!(first_message.matches(RULE_ID).count(), 2, "{first_message}");
}
