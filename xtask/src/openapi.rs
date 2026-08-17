use std::path::Path;

use anyhow::Result;

mod bundle;
mod check;
mod fetch;
mod shared;

pub(crate) fn fetch() -> Result<()> {
    fetch::run()
}

pub(crate) fn bundle(output: Option<&Path>) -> Result<()> {
    bundle::run(output)
}

pub(crate) fn check() -> Result<()> {
    check::run()
}
