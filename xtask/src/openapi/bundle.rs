use std::{
    borrow::Cow,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::shared::{
    BUNDLE_NAME, RepositoryPaths, display_path, ensure_regular_file, temporary_directory,
};
use anyhow::{Context, Result, bail, ensure};
use diffy::{
    apply_bytes,
    patch_set::{FileOperation, ParseOptions, PatchKind, PatchSet},
};

pub(super) fn run(output: Option<&Path>) -> Result<()> {
    let paths = RepositoryPaths::discover()?;
    let output = output
        .map(Cow::Borrowed)
        .unwrap_or_else(|| Cow::Owned(paths.openapi.join(BUNDLE_NAME)));

    build_bundle(&paths, &output)
}

pub(super) fn build_bundle(paths: &RepositoryPaths, output: &Path) -> Result<()> {
    ensure!(
        paths.redocly_config.is_file(),
        "missing Redocly configuration at {}",
        paths.redocly_config.display()
    );
    ensure!(
        paths.redocly_cli.is_file(),
        "Redocly CLI is not installed; run 'npm ci' first"
    );

    let work_dir = temporary_directory("world-anvil-openapi")?;
    materialize_sources(paths, work_dir.path(), None)?;

    let candidate = work_dir.path().join(BUNDLE_NAME);
    run_redocly(
        paths,
        [
            OsStr::new("bundle"),
            work_dir.path().join("openapi.yml").as_os_str(),
            OsStr::new("--config"),
            paths.redocly_config.as_os_str(),
            OsStr::new("--output"),
            candidate.as_os_str(),
            OsStr::new("--ext"),
            OsStr::new("json"),
            OsStr::new("--lint-config"),
            OsStr::new("error"),
        ],
    )?;

    ensure_trailing_newline(&candidate)?;

    run_redocly(
        paths,
        [
            OsStr::new("lint"),
            candidate.as_os_str(),
            OsStr::new("--config"),
            paths.redocly_config.as_os_str(),
            OsStr::new("--format"),
            OsStr::new("stylish"),
            OsStr::new("--max-problems"),
            OsStr::new("1000"),
        ],
    )?;

    let output_dir = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_dir)
        .with_context(|| format!("cannot create {}", output_dir.display()))?;
    fs::copy(&candidate, output)
        .with_context(|| format!("cannot write generated bundle to {}", output.display()))?;

    println!("Generated {}", display_path(&paths.root, output));
    Ok(())
}

pub(super) fn materialize_sources(
    paths: &RepositoryPaths,
    destination: &Path,
    before: Option<&OsStr>,
) -> Result<()> {
    ensure!(
        paths.upstream.join("openapi.yml").is_file(),
        "missing upstream snapshot; run 'cargo xtask openapi fetch' first"
    );

    let patch_files = selected_patch_files(paths, before)?;
    copy_directory(&paths.upstream, destination)?;
    apply_patch_files(paths, destination, &patch_files)
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("cannot create {}", destination.display()))?;

    let mut entries = fs::read_dir(source)
        .with_context(|| format!("cannot read directory {}", source.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("cannot inspect {}", source_path.display()))?;

        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "cannot copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        } else {
            bail!(
                "unsupported filesystem entry in upstream snapshot: {}",
                source_path.display()
            );
        }
    }

    Ok(())
}

fn selected_patch_files(paths: &RepositoryPaths, before: Option<&OsStr>) -> Result<Vec<PathBuf>> {
    let mut patch_files = fs::read_dir(&paths.patches)
        .with_context(|| format!("cannot read {}", paths.patches.display()))?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("patch")))
        .collect::<Vec<_>>();
    patch_files.sort();

    if let Some(before) = before {
        let before_path = Path::new(before);
        ensure!(
            before_path.components().count() == 1
                && before_path.file_name().is_some_and(|name| name == before),
            "--before must be a patch filename, not a path: {}",
            before_path.display()
        );
        ensure!(
            before_path.extension() == Some(OsStr::new("patch")),
            "--before must name a .patch file: {}",
            before_path.display()
        );

        let cutoff = patch_files
            .iter()
            .position(|path| path.file_name().is_some_and(|name| name == before))
            .with_context(|| {
                format!(
                    "--before patch does not exist in {}: {}",
                    paths.patches.display(),
                    before_path.display()
                )
            })?;
        patch_files.truncate(cutoff);
    }

    Ok(patch_files)
}

fn apply_patch_files(
    paths: &RepositoryPaths,
    work_root: &Path,
    patch_files: &[PathBuf],
) -> Result<()> {
    for patch_file in patch_files {
        println!("Applying {}", display_path(&paths.root, patch_file));
        apply_patch_file(patch_file, work_root)?;
    }

    Ok(())
}

fn apply_patch_file(patch_file: &Path, work_root: &Path) -> Result<()> {
    let input = fs::read(patch_file)
        .with_context(|| format!("cannot read patch {}", patch_file.display()))?;
    let mut changed = false;

    for (index, parsed) in PatchSet::parse_bytes(&input, ParseOptions::gitdiff()).enumerate() {
        let file_patch = parsed.with_context(|| {
            format!(
                "cannot parse entry {} in {}",
                index + 1,
                patch_file.display()
            )
        })?;
        match file_patch.operation() {
            FileOperation::Modify { original, modified } => {
                ensure!(
                    original.as_ref().starts_with(b"a/"),
                    "original path lacks the required a/ prefix in {}",
                    patch_file.display()
                );
                ensure!(
                    modified.as_ref().starts_with(b"b/"),
                    "modified path lacks the required b/ prefix in {}",
                    patch_file.display()
                );
            }
            other => bail!(
                "unsupported patch operation in {}: {other:?}",
                patch_file.display()
            ),
        }

        let operation = file_patch.operation().strip_prefix(1);
        let (original, modified) = match operation {
            FileOperation::Modify { original, modified } => (original, modified),
            other => bail!(
                "unsupported patch operation in {}: {other:?}",
                patch_file.display()
            ),
        };
        ensure!(
            original.as_ref() == modified.as_ref(),
            "renaming through a modify patch is unsupported in {}",
            patch_file.display()
        );

        let relative = validate_relative_path(original.as_ref())?;
        let target = work_root.join(&relative);
        ensure_regular_file(&target)?;

        let text_patch = match file_patch.patch() {
            PatchKind::Text(patch) => patch,
            _ => bail!("binary patch is unsupported for {}", relative.display()),
        };
        let base = fs::read(&target)
            .with_context(|| format!("cannot read patch target {}", target.display()))?;
        let patched = apply_bytes(&base, text_patch).with_context(|| {
            format!(
                "cannot apply {} to {}",
                patch_file.display(),
                relative.display()
            )
        })?;
        fs::write(&target, patched)
            .with_context(|| format!("cannot update patch target {}", relative.display()))?;
        changed = true;
    }

    ensure!(
        changed,
        "patch contains no file changes: {}",
        patch_file.display()
    );

    Ok(())
}

fn validate_relative_path(path: &[u8]) -> Result<PathBuf> {
    let path = std::str::from_utf8(path).context("patch path is not UTF-8")?;
    ensure!(!path.is_empty(), "patch path is empty");
    ensure!(
        !path.contains(['\\', ':', '\0']),
        "unsafe patch path: {path}"
    );

    let mut result = PathBuf::new();
    for component in path.split('/') {
        ensure!(
            !component.is_empty() && component != "." && component != "..",
            "unsafe patch path: {path}"
        );
        result.push(component);
    }

    ensure!(result.is_relative(), "patch path is absolute: {path}");
    Ok(result)
}

fn run_redocly<'a>(
    paths: &RepositoryPaths,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<()> {
    let node = env::var_os("NODE").unwrap_or_else(|| OsString::from("node"));
    let mut command = Command::new(&node);
    command
        .arg(&paths.redocly_cli)
        .args(arguments)
        .current_dir(&paths.root);

    let status = command
        .status()
        .with_context(|| format!("cannot run Node.js executable {:?}", node))?;
    ensure!(status.success(), "Redocly exited with status {status}");
    Ok(())
}

fn ensure_trailing_newline(path: &Path) -> Result<()> {
    let mut output = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if !output.ends_with(b"\n") {
        output.push(b'\n');
        fs::write(path, output).with_context(|| format!("cannot update {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rstest::rstest;

    use super::validate_relative_path;

    #[test]
    fn validate_relative_path_accepts_portable_relative_paths() {
        // Arrange
        let path = b"parts/article/article.yml";

        // Act
        let result = validate_relative_path(path);

        // Assert
        assert_eq!(result.unwrap(), Path::new("parts/article/article.yml"));
    }

    #[rstest]
    #[case::empty(b"")]
    #[case::absolute(b"/absolute")]
    #[case::parent_traversal(b"../escape")]
    #[case::nested_parent_traversal(b"nested/../escape")]
    #[case::empty_segment(b"nested//file")]
    #[case::windows_drive_prefix(b"C:/windows")]
    #[case::windows_separator(b"windows\\path")]
    fn validate_relative_path_rejects_an_unsafe_path(#[case] path: &[u8]) {
        // Arrange is provided by the named case.

        // Act
        let result = validate_relative_path(path);

        // Assert
        assert!(result.is_err(), "accepted {path:?}");
    }
}
