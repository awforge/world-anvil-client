use serde_json::Value;

use super::context::{Validator, resolve_local};

const REQUIRED_BODY_RULE: &str = "WA-BODY-001";

pub(super) fn validate(validator: &mut Validator<'_>) {
    for operation in validator.operations.clone() {
        let Some(body) = operation.operation.get("requestBody") else {
            continue;
        };
        match resolve_local(validator.root, body) {
            Ok(body) if body.get("required").and_then(Value::as_bool) == Some(true) => {}
            Ok(_) => validator.push(
                REQUIRED_BODY_RULE,
                operation.location(),
                "requestBody.required must be true",
            ),
            Err(error) => validator.push(REQUIRED_BODY_RULE, operation.location(), error),
        }
    }
}
