use std::collections::BTreeSet;

use serde_json::Value;

use super::context::{resolve_local, resolve_reference_target};

pub(super) fn component_schema<'a>(root: &'a Value, name: &str) -> Option<&'a Value> {
    root.pointer("/components/schemas")
        .and_then(Value::as_object)
        .and_then(|schemas| schemas.get(name))
        .and_then(|schema| resolve_local(root, schema).ok())
}

pub(super) fn effective_shape(
    root: &Value,
    schema: &Value,
) -> std::result::Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let mut properties = BTreeSet::new();
    let mut required = BTreeSet::new();
    let mut active_references = BTreeSet::new();
    collect_effective_shape(
        root,
        schema,
        &mut active_references,
        &mut properties,
        &mut required,
    )?;
    Ok((properties, required))
}

fn collect_effective_shape(
    root: &Value,
    schema: &Value,
    active_references: &mut BTreeSet<String>,
    properties: &mut BTreeSet<String>,
    required: &mut BTreeSet<String>,
) -> std::result::Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if !active_references.insert(reference.to_owned()) {
            return Err(format!("cyclic schema reference {reference}"));
        }
        let target = resolve_reference_target(root, reference)?;
        collect_effective_shape(root, target, active_references, properties, required)?;
        active_references.remove(reference);
        return Ok(());
    }

    let Some(schema) = schema.as_object() else {
        return Ok(());
    };
    if let Some(schema_properties) = schema.get("properties").and_then(Value::as_object) {
        properties.extend(schema_properties.keys().cloned());
    }
    if let Some(schema_required) = schema.get("required").and_then(Value::as_array) {
        required.extend(
            schema_required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            collect_effective_shape(root, branch, active_references, properties, required)?;
        }
    }
    Ok(())
}

pub(super) fn effective_properties<'a>(
    root: &'a Value,
    schema: &'a Value,
    name: &str,
) -> std::result::Result<Vec<&'a Value>, String> {
    let mut active_references = BTreeSet::new();
    let mut properties = Vec::new();
    collect_effective_properties(root, schema, name, &mut active_references, &mut properties)?;
    Ok(properties)
}

fn collect_effective_properties<'a>(
    root: &'a Value,
    schema: &'a Value,
    name: &str,
    active_references: &mut BTreeSet<String>,
    properties: &mut Vec<&'a Value>,
) -> std::result::Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if !active_references.insert(reference.to_owned()) {
            return Err(format!("cyclic schema reference {reference}"));
        }
        let target = resolve_reference_target(root, reference)?;
        collect_effective_properties(root, target, name, active_references, properties)?;
        active_references.remove(reference);
        return Ok(());
    }
    if let Some(property) = schema
        .pointer("/properties")
        .and_then(Value::as_object)
        .and_then(|p| p.get(name))
    {
        properties.push(resolve_local(root, property)?);
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            collect_effective_properties(root, branch, name, active_references, properties)?;
        }
    }
    Ok(())
}

pub(super) fn composition_refs<'a>(schema: &'a Value, keyword: &str) -> BTreeSet<&'a str> {
    schema
        .get(keyword)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|branch| branch.get("$ref").and_then(Value::as_str))
        .collect()
}

pub(super) fn request_json_schema<'a>(
    root: &'a Value,
    operation: &'a Value,
) -> std::result::Result<&'a Value, String> {
    let body = operation
        .get("requestBody")
        .ok_or_else(|| "requestBody is missing".to_owned())?;
    let body = resolve_local(root, body)?;
    body.pointer("/content/application~1json/schema")
        .ok_or_else(|| "application/json request schema is missing".to_owned())
}

pub(super) fn response_json_schema<'a>(
    root: &'a Value,
    operation: &'a Value,
    status: &str,
) -> std::result::Result<&'a Value, String> {
    let response = operation
        .get("responses")
        .and_then(Value::as_object)
        .and_then(|responses| responses.get(status))
        .ok_or_else(|| format!("response {status} is missing"))?;
    let response = resolve_local(root, response)?;
    response
        .pointer("/content/application~1json/schema")
        .ok_or_else(|| format!("response {status} application/json schema is missing"))
}

pub(super) fn is_uuid_string(schema: Option<&Value>) -> bool {
    schema.is_some_and(|schema| {
        schema.get("type").and_then(Value::as_str) == Some("string")
            && schema.get("format").and_then(Value::as_str) == Some("uuid")
    })
}
