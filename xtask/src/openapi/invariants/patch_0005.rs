use std::collections::BTreeSet;

use serde_json::Value;

use super::context::Validator;

const MANUSCRIPT_RESPONSES_RULE: &str = "WA-CONTRACT-MANUSCRIPT-RESPONSES";

pub(super) const MANUSCRIPT_OPERATION_IDS: &[&str] = &[
    "createManuscript",
    "createManuscriptBeat",
    "createManuscriptBookmark",
    "createManuscriptLabel",
    "createManuscriptPart",
    "createManuscriptPlot",
    "createManuscriptStat",
    "createManuscriptTag",
    "createManuscriptVersion",
    "deleteManuscript",
    "deleteManuscriptBeat",
    "deleteManuscriptBookmark",
    "deleteManuscriptLabel",
    "deleteManuscriptPart",
    "deleteManuscriptPlot",
    "deleteManuscriptStat",
    "deleteManuscriptTag",
    "deleteManuscriptVersion",
    "listManuscriptBeatsByManuscriptPart",
    "listManuscriptBookmarksByManuscript",
    "listManuscriptLabelsByManuscript",
    "listManuscriptPartsByManuscriptVersion",
    "listManuscriptPlotsByManuscriptVersion",
    "listManuscriptStatsByManuscriptVersion",
    "listManuscriptTagsByManuscript",
    "listManuscriptVersionsByManuscript",
    "listManuscriptsByWorld",
    "readManuscript",
    "readManuscriptBeat",
    "readManuscriptBookmark",
    "readManuscriptLabel",
    "readManuscriptPart",
    "readManuscriptPlot",
    "readManuscriptStat",
    "readManuscriptTag",
    "readManuscriptVersion",
    "updateManuscript",
    "updateManuscriptBeat",
    "updateManuscriptBookmark",
    "updateManuscriptLabel",
    "updateManuscriptPart",
    "updateManuscriptPlot",
    "updateManuscriptStat",
    "updateManuscriptTag",
    "updateManuscriptVersion",
];

pub(super) fn validate(validator: &mut Validator<'_>) {
    const EXPECTED: &[&str] = &["400", "401", "403", "404", "405", "422"];

    let present_operation_ids = validator
        .operations
        .iter()
        .map(|operation| operation.operation_id())
        .collect::<BTreeSet<_>>();
    for operation_id in MANUSCRIPT_OPERATION_IDS {
        if !present_operation_ids.contains(operation_id) {
            validator.push(
                MANUSCRIPT_RESPONSES_RULE,
                format!("operationId {operation_id}"),
                "required patched manuscript operation is missing",
            );
        }
    }

    for operation in validator.operations.clone() {
        let is_manuscript = operation
            .operation
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .any(|tag| tag == "Manuscript" || tag.starts_with("Manuscript "))
            })
            || is_manuscript_operation_id(operation.operation_id());
        if !is_manuscript {
            continue;
        }

        let Some(responses) = operation
            .operation
            .get("responses")
            .and_then(Value::as_object)
        else {
            validator.push(
                MANUSCRIPT_RESPONSES_RULE,
                operation.location(),
                "responses must be an object",
            );
            continue;
        };
        let missing = EXPECTED
            .iter()
            .copied()
            .filter(|status| !responses.contains_key(*status))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            validator.push(
                MANUSCRIPT_RESPONSES_RULE,
                operation.location(),
                format!(
                    "missing explicit client-error response statuses {}",
                    missing.join(", ")
                ),
            );
        }
    }
}

fn is_manuscript_operation_id(operation_id: &str) -> bool {
    [
        "readManuscript",
        "createManuscript",
        "deleteManuscript",
        "updateManuscript",
        "listManuscripts",
        "listManuscript",
    ]
    .iter()
    .any(|prefix| {
        operation_id.strip_prefix(prefix).is_some_and(|suffix| {
            suffix.is_empty()
                || suffix.starts_with(|character: char| character.is_ascii_uppercase())
        })
    })
}
