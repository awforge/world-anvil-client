use serde_json::{Map, Value};

use super::context::{Validator, resolve_local};

const ERROR_RESPONSE_RULE: &str = "WA-ERROR-001";

pub(super) fn validate(validator: &mut Validator<'_>) {
    for operation in validator.operations.clone() {
        let Some(responses) = operation
            .operation
            .get("responses")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let mut statuses = responses
            .iter()
            .filter(|(status, _)| is_error_status(status))
            .collect::<Vec<_>>();
        statuses.sort_by_key(|(status, _)| *status);

        for (status, response) in statuses {
            let location = format!("{}, response {status}", operation.location());
            let response = match resolve_local(validator.root, response) {
                Ok(response) => response,
                Err(error) => {
                    validator.push(ERROR_RESPONSE_RULE, location, error);
                    continue;
                }
            };
            if let Some(content) = response.get("content")
                && !content.as_object().is_some_and(Map::is_empty)
            {
                let media_types = content
                    .as_object()
                    .map(|content| content.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "invalid content value".to_owned());
                validator.push(
                    ERROR_RESPONSE_RULE,
                    location,
                    format!("error response must be bodyless; found {media_types}"),
                );
            }
        }
    }
}

fn is_error_status(status: &str) -> bool {
    status == "default"
        || status.eq_ignore_ascii_case("4XX")
        || status.eq_ignore_ascii_case("5XX")
        || status
            .parse::<u16>()
            .is_ok_and(|status| (400..=599).contains(&status))
}
