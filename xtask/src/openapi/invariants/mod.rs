use anyhow::{Context, Result};
use openapiv3::OpenAPI;
use serde_json::Value;

mod context;
mod patch_0001;
mod patch_0002;
mod patch_0003;
mod patch_0004;
mod patch_0005;
mod schema;

use context::Validator;

#[cfg(test)]
use patch_0005::MANUSCRIPT_OPERATION_IDS;

#[derive(Clone, Copy)]
struct RuleSelection {
    patch_0001: bool,
    patch_0002: bool,
    patch_0003: bool,
    patch_0004: bool,
    patch_0005: bool,
}

impl RuleSelection {
    const ALL: Self = Self {
        patch_0001: true,
        patch_0002: true,
        patch_0003: true,
        patch_0004: true,
        patch_0005: true,
    };

    #[cfg(test)]
    const PATCH_0001: Self = Self {
        patch_0001: true,
        patch_0002: false,
        patch_0003: false,
        patch_0004: false,
        patch_0005: false,
    };

    #[cfg(test)]
    const PATCH_0002: Self = Self {
        patch_0001: false,
        patch_0002: true,
        patch_0003: false,
        patch_0004: false,
        patch_0005: false,
    };

    #[cfg(test)]
    const PATCH_0003: Self = Self {
        patch_0001: false,
        patch_0002: false,
        patch_0003: true,
        patch_0004: false,
        patch_0005: false,
    };

    #[cfg(test)]
    const PATCH_0004: Self = Self {
        patch_0001: false,
        patch_0002: false,
        patch_0003: false,
        patch_0004: true,
        patch_0005: false,
    };

    #[cfg(test)]
    const PATCH_0005: Self = Self {
        patch_0001: false,
        patch_0002: false,
        patch_0003: false,
        patch_0004: false,
        patch_0005: true,
    };
}

pub(super) fn validate(specification: &[u8]) -> Result<()> {
    let raw: Value = serde_json::from_slice(specification)
        .context("cannot parse the bundled OpenAPI document as JSON")?;
    let _: OpenAPI = serde_json::from_slice(specification)
        .context("cannot parse the bundled document as OpenAPI 3")?;

    validate_value(&raw, RuleSelection::ALL)
}

#[cfg(test)]
fn validate_patch_0001(specification: &[u8]) -> Result<()> {
    validate_test_document(specification, RuleSelection::PATCH_0001)
}

#[cfg(test)]
fn validate_patch_0002(specification: &[u8]) -> Result<()> {
    validate_test_document(specification, RuleSelection::PATCH_0002)
}

#[cfg(test)]
fn validate_patch_0003(specification: &[u8]) -> Result<()> {
    validate_test_document(specification, RuleSelection::PATCH_0003)
}

#[cfg(test)]
fn validate_patch_0004(specification: &[u8]) -> Result<()> {
    validate_test_document(specification, RuleSelection::PATCH_0004)
}

#[cfg(test)]
fn validate_patch_0005(specification: &[u8]) -> Result<()> {
    validate_test_document(specification, RuleSelection::PATCH_0005)
}

#[cfg(test)]
fn validate_test_document(specification: &[u8], rules: RuleSelection) -> Result<()> {
    let raw: Value = serde_json::from_slice(specification).context("cannot parse test JSON")?;
    validate_value(&raw, rules)
}

fn validate_value(root: &Value, rules: RuleSelection) -> Result<()> {
    let mut validator = Validator::new(root, rules.patch_0001);

    if rules.patch_0001 {
        patch_0001::validate(&mut validator);
    }
    if rules.patch_0002 {
        patch_0002::validate(&mut validator);
    }
    if rules.patch_0003 {
        patch_0003::validate(&mut validator);
    }
    if rules.patch_0004 {
        patch_0004::validate(&mut validator);
    }
    if rules.patch_0005 {
        patch_0005::validate(&mut validator);
    }

    validator.finish()
}

#[cfg(test)]
#[path = "tests/patch_0001.rs"]
mod patch_0001_tests;

#[cfg(test)]
#[path = "tests/patch_0002.rs"]
mod patch_0002_tests;

#[cfg(test)]
#[path = "tests/patch_0003.rs"]
mod patch_0003_tests;

#[cfg(test)]
#[path = "tests/patch_0004.rs"]
mod patch_0004_tests;

#[cfg(test)]
#[path = "tests/patch_0005.rs"]
mod patch_0005_tests;

#[cfg(test)]
#[path = "tests/current_bundle.rs"]
mod current_bundle_tests;
