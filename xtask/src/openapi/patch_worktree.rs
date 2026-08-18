use std::{
    env,
    ffi::OsStr,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use tempfile::{Builder as TempDirBuilder, TempDir};

use super::{
    bundle::materialize_sources,
    shared::{RepositoryPaths, display_path},
};

const GIT_REPOSITORY_ENVIRONMENT: [&str; 7] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_QUARANTINE_PATH",
    "GIT_WORK_TREE",
];

pub(super) fn run(output: &Path, before: Option<&OsStr>) -> Result<()> {
    let paths = RepositoryPaths::discover()?;
    create(&paths, output, before)
}

fn create(paths: &RepositoryPaths, output: &Path, before: Option<&OsStr>) -> Result<()> {
    let destination = validate_destination(paths, output)?;
    let staging = TempDirBuilder::new()
        .prefix(".world-anvil-openapi-worktree-")
        .tempdir_in(&destination.parent)
        .with_context(|| {
            format!(
                "cannot create a staging directory in {}",
                destination.parent.display()
            )
        })?;

    materialize_sources(paths, staging.path(), before)?;
    ensure!(
        !staging.path().join(".git").try_exists().with_context(|| {
            format!(
                "cannot inspect Git metadata in {}",
                staging.path().display()
            )
        })?,
        "the upstream snapshot contains a .git entry"
    );
    initialize_git(staging.path())?;
    install(staging, &destination)?;

    println!(
        "Created patch worktree at {}",
        display_path(&paths.root, &destination.path)
    );
    println!("The staged Git index is the patched-source baseline.");
    println!(
        "Edit the YAML files, then use the Git diff command documented in \
         openapi/patches/README.md."
    );

    Ok(())
}

struct Destination {
    path: PathBuf,
    parent: PathBuf,
    existed: bool,
}

fn validate_destination(paths: &RepositoryPaths, output: &Path) -> Result<Destination> {
    ensure!(!output.as_os_str().is_empty(), "output path is empty");

    let absolute = std::path::absolute(output)
        .with_context(|| format!("cannot resolve output path {}", output.display()))?;
    let file_name = absolute
        .file_name()
        .context("output must name a directory, not a filesystem root")?;
    let parent = absolute
        .parent()
        .context("output directory has no parent")?;
    let parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "output parent does not exist or cannot be resolved: {}",
            parent.display()
        )
    })?;
    let path = parent.join(file_name);

    let current_directory = env::current_dir()
        .context("cannot determine the current working directory")?
        .canonicalize()
        .context("cannot resolve the current working directory")?;
    ensure!(
        path != current_directory,
        "output must not be the current working directory: {}",
        path.display()
    );

    let upstream = fs::canonicalize(&paths.upstream).with_context(|| {
        format!(
            "cannot resolve upstream snapshot {}",
            paths.upstream.display()
        )
    })?;
    ensure!(
        !path.starts_with(&upstream),
        "output must not be inside the upstream snapshot: {}",
        path.display()
    );

    for git_metadata in repository_git_directories(&paths.root)? {
        ensure!(
            !path.starts_with(&git_metadata),
            "output must not be inside repository Git metadata: {}",
            path.display()
        );
    }

    let existed = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_dir(),
                "output exists and is not a regular directory: {}",
                path.display()
            );
            ensure_empty_directory(&path)?;
            true
        }
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect output {}", path.display()));
        }
    };

    Ok(Destination {
        path,
        parent,
        existed,
    })
}

fn repository_git_directories(root: &Path) -> Result<Vec<PathBuf>> {
    let dot_git = root.join(".git");
    let metadata = match fs::metadata(&dot_git) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("cannot inspect Git metadata {}", dot_git.display()));
        }
    };

    let git_directory = if metadata.is_dir() {
        fs::canonicalize(&dot_git)
            .with_context(|| format!("cannot resolve Git metadata {}", dot_git.display()))?
    } else {
        ensure!(
            metadata.is_file(),
            "repository Git metadata is not a file or directory: {}",
            dot_git.display()
        );
        let pointer = fs::read_to_string(&dot_git)
            .with_context(|| format!("cannot read Git metadata pointer {}", dot_git.display()))?;
        let relative = pointer
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .with_context(|| format!("invalid Git metadata pointer {}", dot_git.display()))?;
        let path = Path::new(relative);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        fs::canonicalize(&path)
            .with_context(|| format!("cannot resolve Git metadata {}", path.display()))?
    };
    ensure!(
        git_directory.is_dir(),
        "Git metadata path is not a directory: {}",
        git_directory.display()
    );

    let mut directories = vec![git_directory.clone()];
    let common_pointer = git_directory.join("commondir");
    match fs::read_to_string(&common_pointer) {
        Ok(pointer) => {
            let relative = pointer.trim();
            ensure!(
                !relative.is_empty(),
                "Git common-directory pointer is empty: {}",
                common_pointer.display()
            );
            let path = Path::new(relative);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                git_directory.join(path)
            };
            let common_directory = fs::canonicalize(&path)
                .with_context(|| format!("cannot resolve Git metadata {}", path.display()))?;
            ensure!(
                common_directory.is_dir(),
                "Git common metadata path is not a directory: {}",
                common_directory.display()
            );
            if common_directory != git_directory {
                directories.push(common_directory);
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "cannot read Git common-directory pointer {}",
                    common_pointer.display()
                )
            });
        }
    }

    Ok(directories)
}

fn ensure_empty_directory(path: &Path) -> Result<()> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("cannot read output directory {}", path.display()))?;
    ensure!(
        entries.next().transpose()?.is_none(),
        "output directory is not empty: {}",
        path.display()
    );
    Ok(())
}

fn initialize_git(worktree: &Path) -> Result<()> {
    run_git(worktree, ["init", "--quiet"])?;
    run_git(worktree, ["config", "--local", "core.autocrlf", "false"])?;
    run_git(worktree, ["config", "--local", "core.filemode", "false"])?;
    run_git(worktree, ["add", "--all", "--force", "--", "."])?;
    run_git(worktree, ["diff", "--quiet", "--", "."])
}

fn run_git<const N: usize>(worktree: &Path, arguments: [&str; N]) -> Result<()> {
    let invocation = format!("git {}", arguments.join(" "));
    let mut command = Command::new("git");
    command.args(arguments).current_dir(worktree);
    for variable in GIT_REPOSITORY_ENVIRONMENT {
        command.env_remove(variable);
    }

    let status = command
        .status()
        .with_context(|| format!("cannot run `{invocation}`; ensure Git is available on PATH"))?;
    ensure!(
        status.success(),
        "`{invocation}` exited with status {status}"
    );
    Ok(())
}

fn install(staging: TempDir, destination: &Destination) -> Result<()> {
    if destination.existed {
        ensure_empty_directory(&destination.path)?;
        fs::remove_dir(&destination.path).with_context(|| {
            format!(
                "cannot replace empty output directory {}",
                destination.path.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(staging.path(), &destination.path) {
        if destination.existed {
            let _ = fs::create_dir(&destination.path);
        }
        bail!(
            "cannot install patch worktree at {}: {error}",
            destination.path.display()
        );
    }
    let _ = staging.keep();

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, fs, path::Path, process::Command};

    use tempfile::TempDir;

    use super::create;
    use crate::openapi::shared::RepositoryPaths;

    const FIRST_PATCH: &str = "\
diff --git a/parts/example.yml b/parts/example.yml
--- a/parts/example.yml
+++ b/parts/example.yml
@@ -1 +1 @@
-value: upstream
+value: first
";
    const SECOND_PATCH: &str = "\
diff --git a/parts/example.yml b/parts/example.yml
--- a/parts/example.yml
+++ b/parts/example.yml
@@ -1 +1 @@
-value: first
+value: second
";

    struct Fixture {
        directory: TempDir,
        paths: RepositoryPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            let openapi = root.join("openapi");
            let upstream = openapi.join("upstream");
            let patches = openapi.join("patches");
            fs::create_dir_all(upstream.join("parts")).unwrap();
            fs::create_dir_all(&patches).unwrap();
            fs::write(upstream.join("openapi.yml"), "openapi: 3.0.3\n").unwrap();
            fs::write(upstream.join("parts/example.yml"), "value: upstream\n").unwrap();
            fs::write(patches.join("0001-first.patch"), FIRST_PATCH).unwrap();
            fs::write(patches.join("0002-second.patch"), SECOND_PATCH).unwrap();
            fs::write(patches.join("README.md"), "not a patch\n").unwrap();

            let paths = RepositoryPaths {
                root: root.clone(),
                openapi: openapi.clone(),
                upstream,
                patches,
                redocly_config: openapi.join("redocly.yaml"),
                redocly_cli: root.join("node_modules/redocly.js"),
            };

            Self { directory, paths }
        }

        fn output(&self, name: &str) -> std::path::PathBuf {
            self.directory.path().join(name)
        }
    }

    fn example_value(worktree: &Path) -> String {
        fs::read_to_string(worktree.join("parts/example.yml")).unwrap()
    }

    fn git_diff(worktree: &Path) -> Vec<u8> {
        let output = Command::new("git")
            .args([
                "diff",
                "--binary",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--",
                ".",
            ])
            .current_dir(worktree)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.status);
        output.stdout
    }

    #[test]
    fn create_applies_all_patches_and_stages_the_result() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");

        // Act
        create(&fixture.paths, &output, None).unwrap();

        // Assert
        assert_eq!(example_value(&output), "value: second\n");
        assert!(output.join(".git").is_dir());
        assert!(git_diff(&output).is_empty());
    }

    #[test]
    fn create_stops_before_the_named_patch() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");

        // Act
        create(
            &fixture.paths,
            &output,
            Some(OsStr::new("0002-second.patch")),
        )
        .unwrap();

        // Assert
        assert_eq!(example_value(&output), "value: first\n");
        assert!(git_diff(&output).is_empty());
    }

    #[test]
    fn create_before_the_first_patch_materializes_the_raw_snapshot() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");

        // Act
        create(
            &fixture.paths,
            &output,
            Some(OsStr::new("0001-first.patch")),
        )
        .unwrap();

        // Assert
        assert_eq!(example_value(&output), "value: upstream\n");
    }

    #[test]
    fn create_rejects_an_unknown_cutoff_without_creating_the_output() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");

        // Act
        let error = create(
            &fixture.paths,
            &output,
            Some(OsStr::new("0003-missing.patch")),
        )
        .unwrap_err()
        .to_string();

        // Assert
        assert!(error.contains("--before patch does not exist"), "{error}");
        assert!(!output.exists());
    }

    #[test]
    fn create_accepts_an_existing_empty_output_directory() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");
        fs::create_dir(&output).unwrap();

        // Act
        create(&fixture.paths, &output, None).unwrap();

        // Assert
        assert_eq!(example_value(&output), "value: second\n");
        assert!(output.join(".git").is_dir());
    }

    #[test]
    fn create_rejects_a_nonempty_output_without_modifying_it() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");
        fs::create_dir(&output).unwrap();
        let existing = output.join("keep.txt");
        fs::write(&existing, "keep me\n").unwrap();

        // Act
        let error = create(&fixture.paths, &output, None)
            .unwrap_err()
            .to_string();

        // Assert
        assert!(error.contains("output directory is not empty"), "{error}");
        assert_eq!(fs::read_to_string(existing).unwrap(), "keep me\n");
    }

    #[test]
    fn create_rejects_an_output_file_without_modifying_it() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");
        fs::write(&output, "keep me\n").unwrap();

        // Act
        let error = create(&fixture.paths, &output, None)
            .unwrap_err()
            .to_string();

        // Assert
        assert!(
            error.contains("output exists and is not a regular directory"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(output).unwrap(), "keep me\n");
    }

    #[test]
    fn create_rejects_a_cutoff_path_without_creating_the_output() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");

        // Act
        let error = create(
            &fixture.paths,
            &output,
            Some(OsStr::new("openapi/patches/0002-second.patch")),
        )
        .unwrap_err()
        .to_string();

        // Assert
        assert!(
            error.contains("--before must be a patch filename, not a path"),
            "{error}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn create_rejects_an_output_nested_in_the_upstream_snapshot() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.paths.upstream.join("worktree");

        // Act
        let error = create(&fixture.paths, &output, None)
            .unwrap_err()
            .to_string();

        // Assert
        assert!(
            error.contains("output must not be inside the upstream snapshot"),
            "{error}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn create_rejects_output_in_linked_worktree_git_metadata() {
        // Arrange
        let fixture = Fixture::new();
        let git_directory = fixture.directory.path().join("worktree-git-directory");
        fs::create_dir(&git_directory).unwrap();
        fs::write(
            fixture.directory.path().join(".git"),
            format!("gitdir: {}\n", git_directory.display()),
        )
        .unwrap();
        let output = git_directory.join("openapi-edit");

        // Act
        let error = create(&fixture.paths, &output, None)
            .unwrap_err()
            .to_string();

        // Assert
        assert!(
            error.contains("output must not be inside repository Git metadata"),
            "{error}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn create_rejects_output_in_linked_worktree_common_git_metadata() {
        // Arrange
        let fixture = Fixture::new();
        let git_directory = fixture.directory.path().join("worktree-git-directory");
        let common_directory = fixture.directory.path().join("common-git-directory");
        fs::create_dir(&git_directory).unwrap();
        fs::create_dir(&common_directory).unwrap();
        fs::write(git_directory.join("commondir"), "../common-git-directory\n").unwrap();
        fs::write(
            fixture.directory.path().join(".git"),
            format!("gitdir: {}\n", git_directory.display()),
        )
        .unwrap();
        let output = common_directory.join("openapi-edit");

        // Act
        let error = create(&fixture.paths, &output, None)
            .unwrap_err()
            .to_string();

        // Assert
        assert!(
            error.contains("output must not be inside repository Git metadata"),
            "{error}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn edited_worktree_emits_a_patch_rooted_at_the_materialized_source() {
        // Arrange
        let fixture = Fixture::new();
        let output = fixture.output("worktree");
        create(&fixture.paths, &output, None).unwrap();

        // Act
        fs::write(output.join("parts/example.yml"), "value: edited\n").unwrap();
        let patch = String::from_utf8(git_diff(&output)).unwrap();

        // Assert
        assert!(
            patch.contains("diff --git a/parts/example.yml b/parts/example.yml"),
            "{patch}"
        );
        assert!(patch.contains("--- a/parts/example.yml"), "{patch}");
        assert!(patch.contains("+++ b/parts/example.yml"), "{patch}");
        assert!(!patch.contains("openapi/upstream"), "{patch}");
    }
}
