use std::{ffi::OsStr, path::Path};

use anyhow::Result;

mod bundle;
mod check;
mod fetch;
mod invariants;
mod patch_worktree;
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

pub(crate) fn patch_worktree(output: &Path, before: Option<&OsStr>) -> Result<()> {
    patch_worktree::run(output, before)
}
