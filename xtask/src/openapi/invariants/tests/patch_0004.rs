use serde_json::{Value, json};

use super::validate_patch_0004;

const RULE_ID: &str = "[WA-BODY-001]";

fn specification(paths: Value, request_bodies: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "openapi": "3.0.3",
        "info": {
            "title": "required request body fixture",
            "version": "1.0.0"
        },
        "paths": paths,
        "components": {
            "requestBodies": request_bodies
        }
    }))
    .expect("the test fixture should serialize")
}

fn operation(operation_id: &str, request_body: Option<Value>) -> Value {
    let mut operation = json!({
        "operationId": operation_id,
        "responses": {
            "204": { "description": "success" }
        }
    });
    if let Some(request_body) = request_body {
        operation["requestBody"] = request_body;
    }
    operation
}

fn json_body(required: Option<bool>) -> Value {
    let mut body = json!({
        "content": {
            "application/json": {
                "schema": { "type": "object" }
            }
        }
    });
    if let Some(required) = required {
        body["required"] = json!(required);
    }
    body
}

fn validation_error(specification: &[u8]) -> String {
    let error = validate_patch_0004(specification).expect_err("the semantic invariant should fail");
    format!("{error:#}")
}

#[test]
fn operations_without_request_bodies_pass() {
    let specification = specification(
        json!({
            "/widgets": {
                "get": operation("readWidgets", None)
            }
        }),
        json!({}),
    );

    validate_patch_0004(&specification)
        .expect("operations without request bodies are out of scope");
}

#[test]
fn an_operation_without_a_request_body_does_not_stop_later_validation() {
    let specification = specification(
        json!({
            "/alpha": {
                "get": operation("readAlpha", None)
            },
            "/zeta": {
                "post": operation("createZeta", Some(json_body(None)))
            }
        }),
        json!({}),
    );

    let message = validation_error(&specification);
    assert_eq!(
        message,
        "OpenAPI semantic invariants failed:\n  [WA-BODY-001] POST /zeta (createZeta): requestBody.required must be true"
    );
}

#[test]
fn an_inline_required_request_body_passes() {
    let specification = specification(
        json!({
            "/widgets": {
                "post": operation("createWidget", Some(json_body(Some(true))))
            }
        }),
        json!({}),
    );

    validate_patch_0004(&specification).expect("an explicitly required request body should pass");
}

#[test]
fn referenced_and_chained_required_request_bodies_pass() {
    let specification = specification(
        json!({
            "/widgets": {
                "post": operation(
                    "createWidget",
                    Some(json!({ "$ref": "#/components/requestBodies/RequiredAlias" }))
                )
            }
        }),
        json!({
            "RequiredAlias": { "$ref": "#/components/requestBodies/Required" },
            "Required": json_body(Some(true))
        }),
    );

    validate_patch_0004(&specification)
        .expect("a chained reference to a required body should pass");
}

#[test]
fn omitted_and_explicitly_false_required_flags_fail() {
    for (name, body) in [
        ("omitted", json_body(None)),
        ("false", json_body(Some(false))),
    ] {
        let specification = specification(
            json!({
                "/widgets": {
                    "post": operation("createWidget", Some(body))
                }
            }),
            json!({}),
        );

        let message = validation_error(&specification);
        assert_eq!(
            message,
            "OpenAPI semantic invariants failed:\n  [WA-BODY-001] POST /widgets (createWidget): requestBody.required must be true",
            "unexpected diagnostic for {name} required flag"
        );
    }
}

#[test]
fn a_referenced_optional_request_body_fails() {
    let specification = specification(
        json!({
            "/widgets": {
                "post": operation(
                    "createWidget",
                    Some(json!({ "$ref": "#/components/requestBodies/OptionalAlias" }))
                )
            }
        }),
        json!({
            "OptionalAlias": { "$ref": "#/components/requestBodies/Optional" },
            "Optional": json_body(Some(false))
        }),
    );

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
}

#[test]
fn missing_external_and_cyclic_request_body_references_fail_closed() {
    let fixtures = [
        (
            "missing",
            json!({
                "operation": { "$ref": "#/components/requestBodies/Missing" },
                "components": {}
            }),
            "unresolved local reference: #/components/requestBodies/Missing",
        ),
        (
            "external",
            json!({
                "operation": {
                    "$ref": "https://example.invalid/openapi.json#/components/requestBodies/Body"
                },
                "components": {}
            }),
            "external reference is not supported: https://example.invalid/openapi.json#/components/requestBodies/Body",
        ),
        (
            "cyclic",
            json!({
                "operation": { "$ref": "#/components/requestBodies/A" },
                "components": {
                    "A": { "$ref": "#/components/requestBodies/B" },
                    "B": { "$ref": "#/components/requestBodies/A" }
                }
            }),
            "cyclic reference #/components/requestBodies/A",
        ),
    ];

    for (name, fixture, expected_error) in fixtures {
        let specification = specification(
            json!({
                "/widgets": {
                    "post": operation("createWidget", Some(fixture["operation"].clone()))
                }
            }),
            fixture["components"].clone(),
        );

        let message = validation_error(&specification);
        assert_eq!(
            message,
            format!(
                "OpenAPI semantic invariants failed:\n  [WA-BODY-001] POST /widgets (createWidget): {expected_error}"
            ),
            "unexpected diagnostic for {name} reference"
        );
    }
}

#[test]
fn diagnostics_are_aggregated_in_deterministic_order() {
    let first = specification(
        json!({
            "/zeta": {
                "post": operation("createZeta", Some(json_body(None)))
            },
            "/alpha": {
                "patch": operation("updateAlpha", Some(json_body(Some(false))))
            }
        }),
        json!({}),
    );
    let second = specification(
        json!({
            "/alpha": {
                "patch": operation("updateAlpha", Some(json_body(Some(false))))
            },
            "/zeta": {
                "post": operation("createZeta", Some(json_body(None)))
            }
        }),
        json!({}),
    );

    let first_message = validation_error(&first);
    let second_message = validation_error(&second);

    assert_eq!(first_message, second_message);
    assert_eq!(first_message.matches(RULE_ID).count(), 2, "{first_message}");
}
