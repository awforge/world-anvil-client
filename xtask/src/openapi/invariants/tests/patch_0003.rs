use serde_json::{Value, json};

use super::validate_patch_0003;

const RULE_ID: &str = "[WA-ERROR-001]";

fn specification(paths: Value, responses: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "openapi": "3.0.3",
        "info": {
            "title": "bodyless error response fixture",
            "version": "1.0.0"
        },
        "paths": paths,
        "components": {
            "responses": responses
        }
    }))
    .expect("the test fixture should serialize")
}

fn operation(operation_id: &str, responses: Value) -> Value {
    json!({
        "operationId": operation_id,
        "responses": responses
    })
}

fn bodyful_response(media_type: &str) -> Value {
    json!({
        "description": "error",
        "content": {
            (media_type): {}
        }
    })
}

fn validation_error(specification: &[u8]) -> String {
    let error = validate_patch_0003(specification).expect_err("the semantic invariant should fail");
    format!("{error:#}")
}

#[test]
fn inline_bodyless_error_responses_pass() {
    let specification = specification(
        json!({
            "/widgets": {
                "get": operation(
                    "readWidgets",
                    json!({
                        "404": { "description": "not found" },
                        "500": { "description": "server error", "content": {} },
                        "4XX": { "description": "other client error" },
                        "default": { "description": "other response" }
                    })
                )
            }
        }),
        json!({}),
    );

    validate_patch_0003(&specification)
        .expect("bodyless error responses should satisfy the invariant");
}

#[test]
fn referenced_and_chained_bodyless_error_responses_pass() {
    let specification = specification(
        json!({
            "/widgets/{id}": {
                "get": operation(
                    "readWidget",
                    json!({
                        "404": { "$ref": "#/components/responses/BodylessAlias" }
                    })
                )
            }
        }),
        json!({
            "BodylessAlias": { "$ref": "#/components/responses/Bodyless" },
            "Bodyless": { "description": "not found" }
        }),
    );

    validate_patch_0003(&specification)
        .expect("a chained reference to a bodyless response should pass");
}

#[test]
fn success_and_redirect_response_bodies_are_out_of_scope() {
    let specification = specification(
        json!({
            "/widgets": {
                "get": operation(
                    "readWidgets",
                    json!({
                        "200": bodyful_response("application/json"),
                        "302": bodyful_response("text/html"),
                        "404": { "description": "not found" }
                    })
                )
            }
        }),
        json!({}),
    );

    validate_patch_0003(&specification).expect("only error responses are required to be bodyless");
}

#[test]
fn bodyful_error_statuses_and_ranges_fail() {
    for (status, media_type) in [
        ("404", "application/json"),
        ("500", "text/plain"),
        ("4XX", "application/problem+json"),
        ("5XX", "application/octet-stream"),
    ] {
        let specification = specification(
            json!({
                "/widgets": {
                    "get": operation(
                        "readWidgets",
                        json!({
                            (status): bodyful_response(media_type)
                        })
                    )
                }
            }),
            json!({}),
        );

        let message = validation_error(&specification);
        assert!(
            message.contains(RULE_ID),
            "missing rule ID for status {status}: {message}"
        );
    }
}

#[test]
fn a_bodyful_default_response_fails() {
    let specification = specification(
        json!({
            "/widgets": {
                "get": operation(
                    "readWidgets",
                    json!({
                        "200": { "description": "success" },
                        "default": bodyful_response("application/json")
                    })
                )
            }
        }),
        json!({}),
    );

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
}

#[test]
fn a_referenced_bodyful_error_response_fails() {
    let specification = specification(
        json!({
            "/widgets/{id}": {
                "get": operation(
                    "readWidget",
                    json!({
                        "404": { "$ref": "#/components/responses/JsonErrorAlias" }
                    })
                )
            }
        }),
        json!({
            "JsonErrorAlias": { "$ref": "#/components/responses/JsonError" },
            "JsonError": bodyful_response("application/json")
        }),
    );

    let message = validation_error(&specification);
    assert!(message.contains(RULE_ID), "missing rule ID: {message}");
}

#[test]
fn missing_external_and_cyclic_response_references_fail_closed() {
    let fixtures = [
        (
            "missing",
            json!({
                "operation": { "$ref": "#/components/responses/Missing" },
                "components": {}
            }),
        ),
        (
            "external",
            json!({
                "operation": {
                    "$ref": "https://example.invalid/openapi.json#/components/responses/Error"
                },
                "components": {}
            }),
        ),
        (
            "cyclic",
            json!({
                "operation": { "$ref": "#/components/responses/A" },
                "components": {
                    "A": { "$ref": "#/components/responses/B" },
                    "B": { "$ref": "#/components/responses/A" }
                }
            }),
        ),
    ];

    for (name, fixture) in fixtures {
        let specification = specification(
            json!({
                "/widgets/{id}": {
                    "get": operation(
                        "readWidget",
                        json!({ "404": fixture["operation"].clone() })
                    )
                }
            }),
            fixture["components"].clone(),
        );

        let message = validation_error(&specification);
        assert!(
            message.contains(RULE_ID),
            "missing rule ID for {name} reference: {message}"
        );
    }
}

#[test]
fn diagnostics_are_aggregated_in_deterministic_order() {
    let first = specification(
        json!({
            "/zeta": {
                "get": operation(
                    "readZeta",
                    json!({ "500": bodyful_response("text/plain") })
                )
            },
            "/alpha": {
                "get": operation(
                    "readAlpha",
                    json!({ "404": bodyful_response("application/json") })
                )
            }
        }),
        json!({}),
    );
    let second = specification(
        json!({
            "/alpha": {
                "get": operation(
                    "readAlpha",
                    json!({ "404": bodyful_response("application/json") })
                )
            },
            "/zeta": {
                "get": operation(
                    "readZeta",
                    json!({ "500": bodyful_response("text/plain") })
                )
            }
        }),
        json!({}),
    );

    let first_message = validation_error(&first);
    let second_message = validation_error(&second);

    assert_eq!(first_message, second_message);
    assert_eq!(first_message.matches(RULE_ID).count(), 2, "{first_message}");
}
