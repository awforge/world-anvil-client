use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    context::{Validator, resolve_local},
    schema::{component_schema, effective_properties, effective_shape},
};

const BODY_METHODS: &[&str] = &["post", "put", "patch"];

const PARAMETER_RULE: &str = "WA-PARAM-001";
const BODY_METHOD_RULE: &str = "WA-BODY-002";
const BODY_COMPATIBILITY_RULE: &str = "WA-BODY-003";
const REQUIRED_PROPERTY_RULE: &str = "WA-SCHEMA-001";
const BLOCK_TEMPLATE_RULE: &str = "WA-CONTRACT-BLOCK-TEMPLATE";

pub(super) fn validate(validator: &mut Validator<'_>) {
    validate_parameters(validator);
    validate_request_body_compatibility(validator);
    validate_required_properties(validator);
    validate_block_template_contract(validator);
}

fn validate_parameters(validator: &mut Validator<'_>) {
    let mut checked_paths = BTreeSet::new();
    for operation in validator.operations.clone() {
        if checked_paths.insert(operation.path) {
            validate_parameter_list(
                validator,
                operation.path_item.get("parameters"),
                format!("path item {}", operation.path),
            );
        }
        validate_parameter_list(
            validator,
            operation.operation.get("parameters"),
            operation.location(),
        );
    }
}

fn validate_parameter_list(
    validator: &mut Validator<'_>,
    parameters: Option<&Value>,
    location: String,
) {
    let Some(parameters) = parameters else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        validator.push(PARAMETER_RULE, location, "parameters must be an array");
        return;
    };

    for (index, parameter) in parameters.iter().enumerate() {
        let parameter = match resolve_local(validator.root, parameter) {
            Ok(parameter) => parameter,
            Err(error) => {
                validator.push(
                    PARAMETER_RULE,
                    format!("{location}, parameter {index}"),
                    error,
                );
                continue;
            }
        };
        if parameter
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| name.trim().is_empty())
        {
            validator.push(
                PARAMETER_RULE,
                format!("{location}, parameter {index}"),
                "parameter name must be nonblank",
            );
        }
    }
}

fn validate_request_body_compatibility(validator: &mut Validator<'_>) {
    for operation in validator.operations.clone() {
        let Some(body) = operation.operation.get("requestBody") else {
            continue;
        };

        if !BODY_METHODS.contains(&operation.method) {
            validator.push(
                BODY_METHOD_RULE,
                operation.location(),
                "request bodies are supported only on POST, PUT, and PATCH operations",
            );
        }

        let body = match resolve_local(validator.root, body) {
            Ok(body) => body,
            Err(error) => {
                validator.push(BODY_COMPATIBILITY_RULE, operation.location(), error);
                continue;
            }
        };
        let Some(content) = body.get("content").and_then(Value::as_object) else {
            validator.push(
                BODY_COMPATIBILITY_RULE,
                operation.location(),
                "request body content must be an object",
            );
            continue;
        };
        let Some(media_type) = content.get("application/json") else {
            validator.push(
                BODY_COMPATIBILITY_RULE,
                operation.location(),
                "the sole request media type must be application/json",
            );
            continue;
        };
        if content.len() != 1 {
            validator.push(
                BODY_COMPATIBILITY_RULE,
                operation.location(),
                "request body must declare exactly one media type",
            );
        }
        if media_type.get("schema").is_none() {
            validator.push(
                BODY_COMPATIBILITY_RULE,
                operation.location(),
                "application/json request body must declare a schema",
            );
        }
        if media_type
            .get("encoding")
            .and_then(Value::as_object)
            .is_some_and(|encoding| !encoding.is_empty())
        {
            validator.push(
                BODY_COMPATIBILITY_RULE,
                operation.location(),
                "request body encoding is not supported by the pinned generator",
            );
        }
    }
}

fn validate_required_properties(validator: &mut Validator<'_>) {
    if let Some(schemas) = validator
        .root
        .pointer("/components/schemas")
        .and_then(Value::as_object)
    {
        for (name, schema) in schemas {
            validate_schema_tree(
                validator,
                schema,
                &format!("components.schemas.{name}"),
                false,
            );
        }
    }

    for operation in validator.operations.clone() {
        if let Some(body) = operation.operation.get("requestBody")
            && let Ok(body) = resolve_local(validator.root, body)
        {
            validate_media_type_schemas(
                validator,
                body.get("content"),
                &format!("{}.requestBody", operation.location()),
            );
        }

        if let Some(responses) = operation
            .operation
            .get("responses")
            .and_then(Value::as_object)
        {
            let mut statuses = responses.iter().collect::<Vec<_>>();
            statuses.sort_by_key(|(status, _)| *status);
            for (status, response) in statuses {
                if let Ok(response) = resolve_local(validator.root, response) {
                    validate_media_type_schemas(
                        validator,
                        response.get("content"),
                        &format!("{}.responses.{status}", operation.location()),
                    );
                }
            }
        }
    }
}

fn validate_media_type_schemas(
    validator: &mut Validator<'_>,
    content: Option<&Value>,
    location: &str,
) {
    let Some(content) = content.and_then(Value::as_object) else {
        return;
    };
    let mut media_types = content.iter().collect::<Vec<_>>();
    media_types.sort_by_key(|(media_type, _)| *media_type);
    for (media_type, definition) in media_types {
        if let Some(schema) = definition.get("schema") {
            validate_schema_tree(
                validator,
                schema,
                &format!("{location}.content.{media_type}.schema"),
                false,
            );
        }
    }
}

fn validate_schema_tree(
    validator: &mut Validator<'_>,
    schema: &Value,
    location: &str,
    inside_all_of: bool,
) {
    let Some(object) = schema.as_object() else {
        return;
    };

    if !inside_all_of && (object.contains_key("required") || object.contains_key("allOf")) {
        match effective_shape(validator.root, schema) {
            Ok((properties, required)) => {
                for missing in required.difference(&properties) {
                    validator.push(
                        REQUIRED_PROPERTY_RULE,
                        location,
                        format!("required property {missing:?} has no declared schema"),
                    );
                }
            }
            Err(error) => validator.push(REQUIRED_PROPERTY_RULE, location, error),
        }
    }

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            validate_schema_tree(
                validator,
                property,
                &format!("{location}.properties.{name}"),
                false,
            );
        }
    }
    if let Some(items) = object.get("items") {
        validate_schema_tree(validator, items, &format!("{location}.items"), false);
    }
    if let Some(additional) = object.get("additionalProperties")
        && additional.is_object()
    {
        validate_schema_tree(
            validator,
            additional,
            &format!("{location}.additionalProperties"),
            false,
        );
    }
    for keyword in ["allOf", "oneOf", "anyOf"] {
        if let Some(branches) = object.get(keyword).and_then(Value::as_array) {
            for (index, branch) in branches.iter().enumerate() {
                validate_schema_tree(
                    validator,
                    branch,
                    &format!("{location}.{keyword}[{index}]"),
                    keyword == "allOf",
                );
            }
        }
    }
    if let Some(not) = object.get("not") {
        validate_schema_tree(validator, not, &format!("{location}.not"), false);
    }
}

fn validate_block_template_contract(validator: &mut Validator<'_>) {
    let Some(schema) = component_schema(validator.root, "BlockTemplateCreate") else {
        return;
    };
    let location = "components.schemas.BlockTemplateCreate.RPGSRD";
    let rpgsrd = effective_properties(validator.root, schema, "RPGSRD").unwrap_or_default();
    if rpgsrd.is_empty() {
        validator.push(
            BLOCK_TEMPLATE_RULE,
            location,
            "required RPGSRD property is missing",
        );
        return;
    }
    if rpgsrd.iter().any(|rpgsrd| {
        let id = rpgsrd.pointer("/properties/id");
        rpgsrd.get("type").and_then(Value::as_str) != Some("object")
            || id.and_then(|id| id.get("type")).and_then(Value::as_str) != Some("integer")
            || id.and_then(|id| id.get("format")).and_then(Value::as_str) != Some("int32")
    }) {
        validator.push(
            BLOCK_TEMPLATE_RULE,
            location,
            "expected an object containing id: integer/int32",
        );
    }
}
