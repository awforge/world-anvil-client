use serde_json::{Map, Value, json};

use super::validate_patch_0001;

const AUTH_RULE: &str = "WA-AUTH-001";
const ARTICLE_RESPONSES_RULE: &str = "WA-CONTRACT-ARTICLE-RESPONSES";
const NOTESECTION_RESPONSES_RULE: &str = "WA-CONTRACT-NOTESECTION-RESPONSES";
const CANVAS_CREATE_RULE: &str = "WA-CONTRACT-CANVAS-CREATE";
const MARKER_READ_RULE: &str = "WA-CONTRACT-MARKER-READ";
const MARKER_TYPES_LIST_RULE: &str = "WA-CONTRACT-MARKER-TYPES-LIST";
const USER_UPDATE_RULE: &str = "WA-CONTRACT-USER-UPDATE";
const SCHEMAS_RULE: &str = "WA-CONTRACT-SCHEMAS";

const STANDARD_RESPONSES: &[&str] = &["200", "400", "401", "403", "404", "405", "422"];

fn response_map(statuses: &[&str]) -> Value {
    let responses = statuses
        .iter()
        .map(|status| {
            (
                (*status).to_owned(),
                json!({ "description": format!("HTTP {status}") }),
            )
        })
        .collect::<Map<_, _>>();
    Value::Object(responses)
}

fn operation(operation_id: &str, statuses: &[&str]) -> Value {
    json!({
        "operationId": operation_id,
        "responses": response_map(statuses),
    })
}

fn insert_operation(specification: &mut Value, path: &str, method: &str, operation: Value) {
    let paths = specification["paths"]
        .as_object_mut()
        .expect("fixture paths must be an object");
    let path_item = paths.entry(path).or_insert_with(|| json!({}));
    path_item
        .as_object_mut()
        .expect("fixture path item must be an object")
        .insert(method.to_owned(), operation);
}

fn valid_specification() -> Value {
    let mut specification = json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Patch 0001 invariant fixture",
            "version": "1.0.0"
        },
        "security": [{
            "ApplicationKey": [],
            "UserAuthenticationToken": []
        }],
        "paths": {},
        "components": {
            "securitySchemes": {
                "ApplicationKey": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "x-application-key"
                },
                "UserAuthenticationToken": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "x-auth-token"
                }
            },
            "schemas": {
                "General": { "type": "object" },
                "canvas.two": { "type": "object" },
                "marker.ref": { "type": "object" },
                "marker.zero": { "type": "object" },
                "marker.one": { "type": "object" },
                "marker.two": { "type": "object" },
                "user.ref": { "type": "object" },
                "user.two": {
                    "type": "object",
                    "properties": {
                        "onboardingProgress": {
                            "type": "integer",
                            "format": "int32"
                        }
                    }
                },
                "history.category.schema": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "format": "uuid"
                        },
                        "title": {},
                        "slug": {},
                        "state": {},
                        "isWip": {},
                        "isDraft": {},
                        "entityClass": {},
                        "icon": {},
                        "url": {},
                        "subscribergroups": {},
                        "folderId": {},
                        "tags": {},
                        "updateDate": {}
                    }
                },
                "markergroup.update": {
                    "type": "object",
                    "properties": {
                        "subscribergroups": uuid_object_array()
                    }
                },
                "subscribergroup.update": {
                    "type": "object",
                    "properties": {
                        "paidsubscribers": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {
                                        "type": "string",
                                        "format": "uuid"
                                    }
                                }
                            }
                        }
                    }
                },
                "timeline.create": timeline_schema(),
                "timeline.update": timeline_schema(),
                "VariableCollectionUpdate": { "type": "object" },
                "VariableCollectionCreate": {
                    "allOf": [
                        { "$ref": "#/components/schemas/VariableCollectionUpdate" },
                        {
                            "type": "object",
                            "required": ["title", "world"]
                        }
                    ]
                }
            }
        }
    });

    specification["components"]["schemas"]
        .as_object_mut()
        .expect("fixture schemas must be an object")
        .extend(patch_0001_schema_fixtures());

    for (method, operation_id) in [
        ("get", "readArticle"),
        ("put", "createArticle"),
        ("delete", "deleteArticle"),
        ("patch", "updateArticle"),
    ] {
        let mut article_operation = operation(operation_id, STANDARD_RESPONSES);
        if matches!(method, "put" | "patch") {
            article_operation["requestBody"] = required_json_body(json!({ "type": "object" }));
        }
        insert_operation(&mut specification, "/article", method, article_operation);
    }

    let mut list_notesections = operation("listNotesectionsByNotebook", STANDARD_RESPONSES);
    list_notesections["requestBody"] = required_json_body(json!({
        "$ref": "#/components/schemas/General"
    }));
    insert_operation(
        &mut specification,
        "/notebook/notesections",
        "post",
        list_notesections,
    );

    let mut create_canvas = operation("createCanvas", STANDARD_RESPONSES);
    create_canvas["requestBody"] = required_json_body(json!({
        "allOf": [
            { "$ref": "#/components/schemas/canvas.two" },
            {
                "type": "object",
                "required": ["title", "world"]
            }
        ]
    }));
    insert_operation(&mut specification, "/canvas", "put", create_canvas);

    let mut read_marker = operation("readMarker", STANDARD_RESPONSES);
    read_marker["responses"]["200"]["content"] = json!({
        "application/json": {
            "schema": {
                "oneOf": [
                    { "$ref": "#/components/schemas/marker.ref" },
                    { "$ref": "#/components/schemas/marker.zero" },
                    { "$ref": "#/components/schemas/marker.one" },
                    { "$ref": "#/components/schemas/marker.two" }
                ]
            }
        }
    });
    insert_operation(&mut specification, "/marker", "get", read_marker);

    let mut list_marker_types = operation("listMarkerTypes", STANDARD_RESPONSES);
    list_marker_types["requestBody"] = required_json_body(json!({
        "$ref": "#/components/schemas/General"
    }));
    insert_operation(
        &mut specification,
        "/markertypes",
        "post",
        list_marker_types,
    );

    let mut update_user = operation("updateUser", STANDARD_RESPONSES);
    update_user["requestBody"] = required_json_body(json!({
        "$ref": "#/components/schemas/user.two"
    }));
    update_user["responses"]["200"]["content"] = json!({
        "application/json": {
            "schema": { "$ref": "#/components/schemas/user.ref" }
        }
    });
    insert_operation(&mut specification, "/user", "patch", update_user);

    specification
}

fn required_json_body(schema: Value) -> Value {
    json!({
        "required": true,
        "content": {
            "application/json": { "schema": schema }
        }
    })
}

fn timeline_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subscribergroups": uuid_object_array(),
            "histories": uuid_object_array()
        }
    })
}

fn uuid_object_array() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "format": "uuid"
                }
            }
        }
    })
}

fn patch_0001_schema_fixtures() -> Map<String, Value> {
    let mut schemas = Map::new();
    schemas.insert(
        "ArticleGenericCore".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "userMetadata": {
                    "type": "object",
                    "nullable": true,
                    "readOnly": true
                },
                "articleMetadata": {
                    "type": "object",
                    "nullable": true,
                    "readOnly": true
                }
            }
        }),
    );
    schemas.insert(
        "ArticleGenericObjectProperties".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "cover": {
                    "allOf": [
                        { "$ref": "#/components/schemas/ImageRef" },
                        { "type": "object", "nullable": true }
                    ]
                }
            }
        }),
    );
    schemas.insert(
        "ArticleGenericFullPlus".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "ancestry": {
                    "type": "object",
                    "properties": {
                        "secondUp": ancestry_property(),
                        "thirdUp": ancestry_property()
                    }
                }
            }
        }),
    );
    schemas.insert(
        "Block".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "dataParser": { "type": "string", "format": "yaml" }
            }
        }),
    );
    schemas.insert("BlockTemplate".to_owned(), json!({ "type": "object" }));
    for (name, base) in [
        ("BlockTemplateExtended", "BlockTemplate"),
        ("BlockTemplateFull", "BlockTemplateExtended"),
        ("BlockTemplateUpdate", "BlockTemplate"),
    ] {
        schemas.insert(
            name.to_owned(),
            json!({
                "allOf": [
                    { "$ref": format!("#/components/schemas/{base}") },
                    { "type": "object" }
                ]
            }),
        );
    }
    schemas.insert(
        "BlockTemplateCreate".to_owned(),
        json!({
            "allOf": [
                { "$ref": "#/components/schemas/BlockTemplate" },
                {
                    "type": "object",
                    "properties": {
                        "RPGSRD": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer", "format": "int32" }
                            }
                        }
                    },
                    "required": ["title", "formSchemaParser", "formSchema", "RPGSRD"]
                }
            ]
        }),
    );

    let shape_names = [
        "canvas.shape.type.draw",
        "canvas.shape.type.arrow",
        "canvas.shape.type.sticky",
        "canvas.shape.type.text",
        "canvas.shape.type.block",
        "canvas.shape.type.rectangle",
    ];
    schemas.insert(
        "canvas.pages".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "page": {
                    "type": "object",
                    "properties": {
                        "shapes": {
                            "type": "object",
                            "additionalProperties": {
                                "oneOf": shape_names
                                    .iter()
                                    .map(|name| json!({
                                        "$ref": format!("#/components/schemas/{name}")
                                    }))
                                    .collect::<Vec<_>>()
                            }
                        }
                    }
                }
            }
        }),
    );
    for name in shape_names {
        schemas.insert(name.to_owned(), json!({ "type": "object" }));
    }
    schemas["canvas.shape.type.draw"] = json!({
        "type": "object",
        "properties": {
            "points": {
                "type": "array",
                "items": { "type": "array", "items": { "type": "number" } }
            }
        }
    });
    schemas["canvas.shape.type.text"] = json!({
        "type": "object",
        "properties": {
            "point": { "type": "array", "items": { "type": "number" } }
        }
    });
    schemas["canvas.shape.type.arrow"] = json!({
        "type": "object",
        "properties": {
            "handles": {
                "type": "object",
                "properties": {
                    "start": {
                        "type": "object",
                        "properties": {
                            "point": { "type": "array", "items": { "type": "number" } }
                        }
                    }
                }
            }
        }
    });

    schemas.insert(
        "ManuscriptRef".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "tags": { "type": "object", "additionalProperties": true }
            }
        }),
    );
    schemas.insert(
        "customarticletemplate.ref".to_owned(),
        read_only_schema(false, &["id", "title"]),
    );
    let reference_properties = [
        "id",
        "title",
        "slug",
        "state",
        "isWip",
        "isDraft",
        "entityClass",
        "icon",
        "url",
        "subscribergroups",
        "folderId",
        "tags",
    ];
    schemas.insert(
        "OrgChartRef".to_owned(),
        read_only_schema(true, &reference_properties),
    );
    schemas.insert(
        "PromptRef".to_owned(),
        read_only_schema(true, &reference_properties),
    );
    schemas
}

fn ancestry_property() -> Value {
    json!({
        "type": "object",
        "nullable": true,
        "oneOf": [
            { "$ref": "#/components/schemas/ArticleRef" },
            { "$ref": "#/components/schemas/CategoryRef" },
            { "$ref": "#/components/schemas/world.ref" }
        ]
    })
}

fn read_only_schema(root_read_only: bool, properties: &[&str]) -> Value {
    let properties = properties
        .iter()
        .map(|name| ((*name).to_owned(), json!({ "readOnly": true })))
        .collect::<Map<_, _>>();
    let mut schema = json!({ "type": "object", "properties": properties });
    if root_read_only {
        schema["readOnly"] = json!(true);
    }
    schema
}

fn validate_fixture(specification: &Value) -> anyhow::Result<()> {
    validate_patch_0001(&serde_json::to_vec(specification).expect("fixture must serialize"))
}

fn assert_rule_violation(specification: &Value, rule_id: &str) {
    let error = validate_fixture(specification).expect_err("mutated fixture must be rejected");
    let diagnostic = format!("{error:#}");
    assert!(
        diagnostic.contains(rule_id),
        "expected rule {rule_id} in validator diagnostic:\n{diagnostic}"
    );
}

#[test]
fn accepts_the_patch_0001_contract_fixture() {
    validate_fixture(&valid_specification()).expect("valid patch 0001 fixture must pass");
}

#[test]
fn authentication_is_an_and_requirement_not_two_or_alternatives() {
    let mut specification = valid_specification();
    specification["security"] = json!([
        { "ApplicationKey": [] },
        { "UserAuthenticationToken": [] }
    ]);

    assert_rule_violation(&specification, AUTH_RULE);
}

#[test]
fn authentication_schemes_use_the_expected_headers() {
    for (pointer, replacement) in [
        (
            "/components/securitySchemes/ApplicationKey/name",
            json!("authorization"),
        ),
        (
            "/components/securitySchemes/ApplicationKey/in",
            json!("query"),
        ),
        (
            "/components/securitySchemes/UserAuthenticationToken/name",
            json!("authorization"),
        ),
        (
            "/components/securitySchemes/UserAuthenticationToken/in",
            json!("query"),
        ),
    ] {
        let mut specification = valid_specification();
        *specification
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture must contain {pointer}")) = replacement;

        assert_rule_violation(&specification, AUTH_RULE);
    }
}

#[test]
fn authentication_schemes_are_header_api_keys_and_must_exist() {
    for scheme_name in ["ApplicationKey", "UserAuthenticationToken"] {
        let mut wrong_kind = valid_specification();
        wrong_kind["components"]["securitySchemes"][scheme_name] = json!({
            "type": "http",
            "scheme": "bearer"
        });
        assert_rule_violation(&wrong_kind, AUTH_RULE);

        let mut missing = valid_specification();
        missing["components"]["securitySchemes"]
            .as_object_mut()
            .expect("fixture security schemes must be an object")
            .remove(scheme_name);
        assert_rule_violation(&missing, AUTH_RULE);
    }
}

#[test]
fn operation_security_cannot_weaken_global_authentication() {
    for security in [
        json!([{ "ApplicationKey": [] }]),
        json!([{ "UserAuthenticationToken": [] }]),
        json!([{}]),
        json!([]),
    ] {
        let mut specification = valid_specification();
        specification["paths"]["/marker"]["get"]["security"] = security;

        assert_rule_violation(&specification, AUTH_RULE);
    }
}

#[test]
fn every_article_operation_retains_the_standard_error_responses() {
    for method in ["get", "put", "delete", "patch"] {
        let mut specification = valid_specification();
        specification["paths"]["/article"][method]["responses"]
            .as_object_mut()
            .expect("fixture responses must be an object")
            .remove("422");

        assert_rule_violation(&specification, ARTICLE_RESPONSES_RULE);
    }
}

#[test]
fn article_operations_may_gain_additional_responses() {
    let mut specification = valid_specification();
    specification["paths"]["/article"]["get"]["responses"]["429"] =
        json!({ "description": "rate limited" });

    validate_fixture(&specification).expect("required response statuses are a subset contract");
}

#[test]
fn notesection_list_retains_the_standard_error_responses() {
    let mut specification = valid_specification();
    specification["paths"]["/notebook/notesections"]["post"]["responses"]
        .as_object_mut()
        .expect("fixture responses must be an object")
        .remove("422");

    assert_rule_violation(&specification, NOTESECTION_RESPONSES_RULE);
}

#[test]
fn canvas_create_keeps_its_base_schema_and_required_fields() {
    for (pointer, replacement) in [
        (
            "/paths/~1canvas/put/requestBody/content/application~1json/schema/allOf/0/$ref",
            json!("#/components/schemas/General"),
        ),
        (
            "/paths/~1canvas/put/requestBody/content/application~1json/schema/allOf/1/required",
            json!(["title"]),
        ),
    ] {
        let mut specification = valid_specification();
        *specification
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture must contain {pointer}")) = replacement;

        assert_rule_violation(&specification, CANVAS_CREATE_RULE);
    }
}

#[test]
fn composition_order_is_not_semantic() {
    let mut specification = valid_specification();
    specification["paths"]["/canvas"]["put"]["requestBody"]["content"]["application/json"]
        ["schema"]["allOf"]
        .as_array_mut()
        .expect("fixture canvas allOf must be an array")
        .reverse();
    specification["components"]["schemas"]["VariableCollectionCreate"]["allOf"]
        .as_array_mut()
        .expect("fixture variable collection allOf must be an array")
        .reverse();

    validate_fixture(&specification).expect("allOf member order must not affect the contract");
}

#[test]
fn marker_read_keeps_all_response_variants() {
    let mut specification = valid_specification();
    specification["paths"]["/marker"]["get"]["responses"]["200"]["content"]["application/json"]["schema"] =
        json!({ "$ref": "#/components/schemas/marker.ref" });

    assert_rule_violation(&specification, MARKER_READ_RULE);
}

#[test]
fn marker_type_list_uses_the_general_request_schema() {
    let mut specification = valid_specification();
    specification["paths"]["/markertypes"]["post"]["requestBody"]["content"]["application/json"]
        ["schema"] = json!({ "$ref": "#/components/schemas/user.ref" });

    assert_rule_violation(&specification, MARKER_TYPES_LIST_RULE);
}

#[test]
fn user_update_response_returns_the_user_reference_schema() {
    let mut specification = valid_specification();
    specification["paths"]["/user"]["patch"]["responses"]["200"]["content"]["application/json"]["schema"] =
        json!({ "$ref": "#/components/schemas/marker.ref" });

    assert_rule_violation(&specification, USER_UPDATE_RULE);
}

#[test]
fn history_category_fields_are_not_hidden_beneath_a_synthetic_type_property() {
    for properties in [
        json!({
            "type": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "format": "uuid" }
                }
            }
        }),
        json!({ "id": { "type": "string" } }),
    ] {
        let mut specification = valid_specification();
        specification["components"]["schemas"]["history.category.schema"]["properties"] =
            properties;

        assert_rule_violation(&specification, SCHEMAS_RULE);
    }
}

#[test]
fn relationship_array_items_are_uuid_bearing_objects() {
    for pointer in [
        "/components/schemas/timeline.create/properties/subscribergroups/items",
        "/components/schemas/timeline.create/properties/histories/items",
        "/components/schemas/timeline.update/properties/subscribergroups/items",
        "/components/schemas/timeline.update/properties/histories/items",
        "/components/schemas/subscribergroup.update/properties/paidsubscribers/items",
    ] {
        let mut specification = valid_specification();
        *specification
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture must contain {pointer}")) =
            json!({ "type": "string", "format": "uuid" });

        assert_rule_violation(&specification, SCHEMAS_RULE);
    }
}

#[test]
fn onboarding_progress_is_an_int32_integer() {
    for schema in [json!({ "type": "string" }), json!({ "type": "integer" })] {
        let mut specification = valid_specification();
        specification["components"]["schemas"]["user.two"]["properties"]["onboardingProgress"] =
            schema;

        assert_rule_violation(&specification, SCHEMAS_RULE);
    }
}

#[test]
fn variable_collection_create_keeps_its_composition_and_required_fields() {
    for schema in [
        json!({ "type": "object" }),
        json!({
            "allOf": [
                { "$ref": "#/components/schemas/VariableCollectionUpdate" },
                { "type": "object", "required": ["title"] }
            ]
        }),
    ] {
        let mut specification = valid_specification();
        specification["components"]["schemas"]["VariableCollectionCreate"] = schema;

        assert_rule_violation(&specification, SCHEMAS_RULE);
    }
}

#[test]
fn a_named_contract_cannot_disappear_silently() {
    let mut specification = valid_specification();
    specification["paths"]
        .as_object_mut()
        .expect("fixture paths must be an object")
        .remove("/canvas");

    assert_rule_violation(&specification, CANVAS_CREATE_RULE);
}

#[test]
fn a_named_schema_cannot_disappear_silently() {
    let mut specification = valid_specification();
    specification["components"]["schemas"]
        .as_object_mut()
        .expect("fixture schemas must be an object")
        .remove("ArticleGenericCore");

    assert_rule_violation(&specification, SCHEMAS_RULE);
}

#[test]
fn corrected_read_only_fields_remain_read_only() {
    let mut specification = valid_specification();
    specification["components"]["schemas"]["OrgChartRef"]["properties"]["title"]
        .as_object_mut()
        .expect("fixture property must be an object")
        .remove("readOnly");

    assert_rule_violation(&specification, SCHEMAS_RULE);
}

#[test]
fn corrected_arrow_points_remain_numeric_arrays() {
    let mut specification = valid_specification();
    specification["components"]["schemas"]["canvas.shape.type.arrow"]["properties"]["handles"]["properties"]
        ["start"]["properties"]["point"]["items"] = json!({ "type": "object" });

    assert_rule_violation(&specification, SCHEMAS_RULE);
}
