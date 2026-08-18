mod openapi;

use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "Repository maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Build and validate the World Anvil OpenAPI description.
    Openapi(OpenapiArgs),
}

#[derive(Debug, Args)]
struct OpenapiArgs {
    #[command(subcommand)]
    task: OpenapiTask,
}

#[derive(Debug, Subcommand)]
enum OpenapiTask {
    /// Refresh the vendored OpenAPI snapshot from World Anvil.
    Fetch,
    /// Rebuild the canonical bundled OpenAPI document.
    Bundle {
        /// Write the bundle to this path instead of the canonical location.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
    /// Create an editable, Git-backed tree of the patched OpenAPI source.
    PatchWorktree {
        /// Create the patch worktree at this path.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
        /// Stop before applying this existing patch filename.
        #[arg(long, value_name = "PATCH")]
        before: Option<OsString>,
    },
    /// Verify the snapshot, reproduce the bundle, and smoke-test generation.
    Check,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    match Cli::parse().task {
        Task::Openapi(args) => match args.task {
            OpenapiTask::Fetch => openapi::fetch(),
            OpenapiTask::Bundle { output } => openapi::bundle(output.as_deref()),
            OpenapiTask::PatchWorktree { output, before } => {
                openapi::patch_worktree(&output, before.as_deref())
            }
            OpenapiTask::Check => openapi::check(),
        },
    }
}
