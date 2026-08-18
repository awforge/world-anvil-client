use std::{collections::BTreeSet, fmt::Write as _};

use anyhow::{Result, bail};
use serde_json::Value;

const HTTP_METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const OPERATION_RULE: &str = "WA-OP-001";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Violation {
    rule: &'static str,
    location: String,
    message: String,
}

#[derive(Clone, Copy)]
pub(super) struct OperationView<'a> {
    pub(super) path: &'a str,
    pub(super) method: &'static str,
    pub(super) path_item: &'a Value,
    pub(super) operation: &'a Value,
}

impl<'a> OperationView<'a> {
    pub(super) fn operation_id(self) -> &'a str {
        self.operation
            .get("operationId")
            .and_then(Value::as_str)
            .unwrap_or("<missing operationId>")
    }

    pub(super) fn location(self) -> String {
        format!(
            "{} {} ({})",
            self.method.to_ascii_uppercase(),
            self.path,
            self.operation_id()
        )
    }
}

pub(super) struct Validator<'a> {
    pub(super) root: &'a Value,
    pub(super) operations: Vec<OperationView<'a>>,
    violations: Vec<Violation>,
}

impl<'a> Validator<'a> {
    pub(super) fn new(root: &'a Value, report_path_errors: bool) -> Self {
        let mut violations = Vec::new();
        let operations = collect_operations(root, report_path_errors, &mut violations);
        Self {
            root,
            operations,
            violations,
        }
    }

    pub(super) fn push(
        &mut self,
        rule: &'static str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.violations.push(Violation {
            rule,
            location: location.into(),
            message: message.into(),
        });
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.violations.sort();
        if self.violations.is_empty() {
            return Ok(());
        }

        let mut diagnostic = String::from("OpenAPI semantic invariants failed:\n");
        for violation in &self.violations {
            let _ = writeln!(
                diagnostic,
                "  [{}] {}: {}",
                violation.rule, violation.location, violation.message
            );
        }
        bail!(diagnostic.trim_end().to_owned())
    }
}

fn collect_operations<'a>(
    root: &'a Value,
    report_path_errors: bool,
    violations: &mut Vec<Violation>,
) -> Vec<OperationView<'a>> {
    let mut operations = Vec::new();
    let Some(paths) = root.get("paths").and_then(Value::as_object) else {
        if report_path_errors {
            violations.push(Violation {
                rule: OPERATION_RULE,
                location: "paths".to_owned(),
                message: "paths must be an object".to_owned(),
            });
        }
        return operations;
    };

    for (path, path_item) in paths {
        let path_item = match resolve_local(root, path_item) {
            Ok(path_item) => path_item,
            Err(error) => {
                if report_path_errors {
                    violations.push(Violation {
                        rule: OPERATION_RULE,
                        location: format!("path item {path}"),
                        message: error,
                    });
                }
                continue;
            }
        };
        let Some(path_item_object) = path_item.as_object() else {
            if report_path_errors {
                violations.push(Violation {
                    rule: OPERATION_RULE,
                    location: format!("path item {path}"),
                    message: "path item must be an object".to_owned(),
                });
            }
            continue;
        };
        for &method in HTTP_METHODS {
            let Some(operation) = path_item_object.get(method) else {
                continue;
            };
            if operation.is_object() {
                operations.push(OperationView {
                    path,
                    method,
                    path_item,
                    operation,
                });
            } else if report_path_errors {
                violations.push(Violation {
                    rule: OPERATION_RULE,
                    location: format!("{} {path}", method.to_ascii_uppercase()),
                    message: "operation must be an object".to_owned(),
                });
            }
        }
    }

    operations.sort_by(|left, right| {
        (left.path, left.method, left.operation_id()).cmp(&(
            right.path,
            right.method,
            right.operation_id(),
        ))
    });
    operations
}

pub(super) fn resolve_local<'a>(
    root: &'a Value,
    value: &'a Value,
) -> std::result::Result<&'a Value, String> {
    let mut current = value;
    let mut visited = BTreeSet::new();
    while let Some(reference) = current.get("$ref").and_then(Value::as_str) {
        if !visited.insert(reference.to_owned()) {
            return Err(format!("cyclic reference {reference}"));
        }
        current = resolve_reference_target(root, reference)?;
    }
    Ok(current)
}

pub(super) fn resolve_reference_target<'a>(
    root: &'a Value,
    reference: &str,
) -> std::result::Result<&'a Value, String> {
    let Some(pointer) = reference.strip_prefix('#') else {
        return Err(format!("external reference is not supported: {reference}"));
    };
    if !pointer.starts_with('/') {
        return Err(format!("invalid local reference: {reference}"));
    }
    root.pointer(pointer)
        .ok_or_else(|| format!("unresolved local reference: {reference}"))
}
