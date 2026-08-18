use anyhow::Result;
use serde_json::{Map, Value, json};

use super::validate_patch_0002;

const PARAMETER_NAME_RULE: &str = "[WA-PARAM-001]";
const FORBIDDEN_METHOD_BODY_RULE: &str = "[WA-BODY-002]";
const REQUEST_BODY_COMPATIBILITY_RULE: &str = "[WA-BODY-003]";
const REQUIRED_PROPERTY_RULE: &str = "[WA-SCHEMA-001]";
const BLOCK_TEMPLATE_CONTRACT_RULE: &str = "[WA-CONTRACT-BLOCK-TEMPLATE]";

fn document() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Invariant test",
            "version": "1.0.0"
        },
        "paths": {}
    })
}

fn operation(operation_id: &str) -> Value {
    json!({
        "operationId": operation_id,
        "responses": {
            "204": {
                "description": "Success"
            }
        }
    })
}

fn json_request_body() -> Value {
    json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": {
                    "type": "object",
                    "properties": {}
                }
            }
        }
    })
}

fn validate_value(specification: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(specification).expect("test specification must serialize");
    validate_patch_0002(&bytes)
}

fn assert_rule(specification: &Value, rule_id: &str) {
    let error = validate_value(specification).expect_err("invalid fixture unexpectedly passed");
    let message = format!("{error:#}");

    assert!(
        message.contains(rule_id),
        "expected diagnostic {rule_id}, got:\n{message}"
    );
}

#[test]
fn rejects_a_blank_operation_parameter_name() {
    // Arrange
    let mut specification = document();
    let mut create = operation("createWidget");
    create["parameters"] = json!([{
        "name": " \t",
        "in": "query",
        "schema": {
            "type": "string"
        }
    }]);
    specification["paths"]["/widgets"] = json!({ "post": create });

    // Act and assert
    assert_rule(&specification, PARAMETER_NAME_RULE);
}

#[test]
fn rejects_a_blank_path_parameter_name() {
    // Arrange
    let mut specification = document();
    specification["paths"]["/widgets"] = json!({
        "parameters": [{
            "name": "",
            "in": "query",
            "schema": {
                "type": "string"
            }
        }],
        "get": operation("listWidgets")
    });

    // Act and assert
    assert_rule(&specification, PARAMETER_NAME_RULE);
}

#[test]
fn accepts_nonblank_path_and_operation_parameter_names() {
    // Arrange
    let mut specification = document();
    let mut create = operation("createWidget");
    create["parameters"] = json!([{
        "name": "filter",
        "in": "query",
        "schema": {
            "type": "string"
        }
    }]);
    specification["paths"]["/widgets"] = json!({
        "parameters": [{
            "name": "api-version",
            "in": "header",
            "schema": {
                "type": "string"
            }
        }],
        "post": create
    });

    // Act
    let result = validate_value(&specification);

    // Assert
    result.unwrap();
}

#[test]
fn rejects_request_bodies_on_bodyless_methods() {
    for method in ["get", "head", "delete"] {
        // Arrange
        let mut specification = document();
        let mut bodyless_operation = operation(&format!("{method}Widget"));
        bodyless_operation["requestBody"] = json_request_body();
        let mut path_item = Map::new();
        path_item.insert(method.to_owned(), bodyless_operation);
        specification["paths"]["/widgets"] = Value::Object(path_item);

        // Act and assert
        assert_rule(&specification, FORBIDDEN_METHOD_BODY_RULE);
    }
}

#[test]
fn rejects_request_bodies_that_progenitor_cannot_consume() {
    let incompatible_bodies = [
        (
            "missing schema",
            json!({
                "required": true,
                "content": {
                    "text/plain": {}
                }
            }),
        ),
        (
            "unsupported multipart media type",
            json!({
                "required": true,
                "content": {
                    "multipart/form-data": {
                        "schema": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                }
            }),
        ),
        (
            "multiple media types",
            json!({
                "required": true,
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "properties": {}
                        }
                    },
                    "text/plain": {
                        "schema": {
                            "type": "string"
                        }
                    }
                }
            }),
        ),
    ];

    for (case, request_body) in incompatible_bodies {
        // Arrange
        let mut specification = document();
        let mut create = operation("createWidget");
        create["requestBody"] = request_body;
        specification["paths"]["/widgets"] = json!({ "post": create });

        // Act
        let error = match validate_value(&specification) {
            Ok(()) => panic!("{case} fixture unexpectedly passed"),
            Err(error) => error,
        };
        let message = format!("{error:#}");

        // Assert
        assert!(
            message.contains(REQUEST_BODY_COMPATIBILITY_RULE),
            "expected diagnostic {REQUEST_BODY_COMPATIBILITY_RULE} for {case}, got:\n{message}"
        );
    }
}

#[test]
fn accepts_a_supported_json_request_body() {
    // Arrange
    let mut specification = document();
    let mut create = operation("createWidget");
    create["requestBody"] = json_request_body();
    specification["paths"]["/widgets"] = json!({ "post": create });

    // Act
    let result = validate_value(&specification);

    // Assert
    result.unwrap();
}

#[test]
fn rejects_an_inline_request_schema_with_an_undeclared_required_property() {
    let mut specification = document();
    let mut create = operation("createWidget");
    create["requestBody"] = json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": {
                    "type": "object",
                    "required": ["missing"]
                }
            }
        }
    });
    specification["paths"]["/widgets"] = json!({ "post": create });

    assert_rule(&specification, REQUIRED_PROPERTY_RULE);
}

#[test]
fn rejects_an_undeclared_required_property_across_all_of() {
    // Arrange
    let mut specification = document();
    specification["components"] = json!({
        "schemas": {
            "Base": {
                "type": "object",
                "properties": {
                    "declared": {
                        "type": "string"
                    }
                }
            },
            "Composed": {
                "allOf": [
                    {
                        "$ref": "#/components/schemas/Base"
                    },
                    {
                        "type": "object",
                        "required": ["declared", "missing"]
                    }
                ]
            }
        }
    });

    // Act and assert
    assert_rule(&specification, REQUIRED_PROPERTY_RULE);
}

#[test]
fn accepts_a_required_property_declared_by_an_all_of_sibling() {
    // Arrange
    let mut specification = document();
    specification["components"] = json!({
        "schemas": {
            "Base": {
                "type": "object",
                "properties": {
                    "declared": {
                        "type": "string"
                    }
                }
            },
            "Composed": {
                "allOf": [
                    {
                        "$ref": "#/components/schemas/Base"
                    },
                    {
                        "type": "object",
                        "required": ["declared"]
                    }
                ]
            }
        }
    });

    // Act
    let result = validate_value(&specification);

    // Assert
    result.unwrap();
}

#[test]
fn rejects_an_incorrect_block_template_rpgsrd_shape() {
    // Arrange
    let specification = block_template_specification(json!({ "type": "string" }));

    // Act and assert
    assert_rule(&specification, BLOCK_TEMPLATE_CONTRACT_RULE);
}

#[test]
fn accepts_the_block_template_rpgsrd_contract() {
    // Arrange
    let specification = block_template_specification(json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "integer",
                "format": "int32"
            }
        }
    }));

    // Act
    let result = validate_value(&specification);

    // Assert
    result.unwrap();
}

#[test]
fn rejects_a_conflicting_duplicate_block_template_rpgsrd_definition() {
    let mut specification = block_template_specification(json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer", "format": "int32" }
        }
    }));
    specification["components"]["schemas"]["BlockTemplateCreate"]["allOf"]
        .as_array_mut()
        .expect("fixture allOf must be an array")
        .push(json!({
            "type": "object",
            "properties": {
                "RPGSRD": { "type": "string" }
            }
        }));

    assert_rule(&specification, BLOCK_TEMPLATE_CONTRACT_RULE);
}

fn block_template_specification(rpgsrd: Value) -> Value {
    let mut specification = document();
    specification["components"] = json!({
        "schemas": {
            "BlockTemplate": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string"
                    },
                    "formSchemaParser": {
                        "type": "string"
                    },
                    "formSchema": {
                        "type": "string"
                    }
                }
            },
            "BlockTemplateCreate": {
                "allOf": [
                    {
                        "$ref": "#/components/schemas/BlockTemplate"
                    },
                    {
                        "type": "object",
                        "properties": {
                            "RPGSRD": rpgsrd
                        },
                        "required": [
                            "title",
                            "formSchemaParser",
                            "formSchema",
                            "RPGSRD"
                        ]
                    }
                ]
            }
        }
    });
    specification
}
