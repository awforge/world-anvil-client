use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use tempfile::{Builder as TempDirBuilder, TempDir};

pub(super) const BUNDLE_NAME: &str = "world-anvil.openapi.json";

pub(super) struct RepositoryPaths {
    pub(super) root: PathBuf,
    pub(super) openapi: PathBuf,
    pub(super) upstream: PathBuf,
    pub(super) patches: PathBuf,
    pub(super) redocly_config: PathBuf,
    pub(super) redocly_cli: PathBuf,
}

impl RepositoryPaths {
    pub(super) fn discover() -> Result<Self> {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir
            .parent()
            .context("xtask must be located directly below the repository root")?
            .to_path_buf();
        let openapi = root.join("openapi");

        Ok(Self {
            upstream: openapi.join("upstream"),
            patches: openapi.join("patches"),
            redocly_config: openapi.join("redocly.yaml"),
            redocly_cli: root.join("node_modules/@redocly/cli/bin/cli.js"),
            root,
            openapi,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ExistingSnapshot {
    pub(super) source: Vec<u8>,
    checksums: Vec<u8>,
    pub(super) checked_files: BTreeSet<String>,
}

pub(super) fn validate_existing_snapshot(upstream: &Path) -> Result<ExistingSnapshot> {
    ensure_regular_directory(upstream)?;
    let source_path = upstream.join("SOURCE.md");
    let checksum_path = upstream.join("SHA256SUMS");
    ensure_regular_file(&source_path)?;
    ensure_regular_file(&checksum_path)?;

    let source =
        fs::read(&source_path).with_context(|| format!("cannot read {}", source_path.display()))?;
    let checksums = fs::read(&checksum_path)
        .with_context(|| format!("cannot read {}", checksum_path.display()))?;
    let checksum_text = std::str::from_utf8(&checksums)
        .with_context(|| format!("{} is not UTF-8", checksum_path.display()))?;
    let entries = parse_checksum_entries(checksum_text)?;
    ensure!(!entries.is_empty(), "upstream checksum list is empty");

    let checked_files = entries.keys().cloned().collect::<BTreeSet<_>>();
    let expected_files = checked_files
        .iter()
        .cloned()
        .chain(["SHA256SUMS".to_owned(), "SOURCE.md".to_owned()])
        .collect::<BTreeSet<_>>();

    let mut actual_files = BTreeSet::new();
    collect_files(upstream, "", &mut actual_files)?;
    ensure_same_files(&expected_files, &actual_files)?;

    for (relative, expected) in entries {
        let path = join_portable_path(upstream, &relative);
        let actual = sha256(&path)?;
        ensure!(
            actual.eq_ignore_ascii_case(&expected),
            "checksum mismatch for {relative}: expected {expected}, got {actual}"
        );
    }

    Ok(ExistingSnapshot {
        source,
        checksums,
        checked_files,
    })
}

fn ensure_regular_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("directory does not exist: {}", path.display()))?;
    ensure!(
        metadata.file_type().is_dir(),
        "path is not a regular directory: {}",
        path.display()
    );
    Ok(())
}

fn parse_checksum_entries(contents: &str) -> Result<BTreeMap<String, String>> {
    let mut entries = BTreeMap::new();
    let mut collision_keys = BTreeMap::<String, String>::new();

    for (index, line) in contents.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (expected, relative) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .with_context(|| format!("invalid checksum entry on line {}", index + 1))?;
        ensure!(
            expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid SHA-256 digest on line {}",
            index + 1
        );
        validate_portable_relative_path(relative)
            .with_context(|| format!("unsafe checksum path on line {}", index + 1))?;
        ensure!(
            !matches!(
                relative.to_ascii_lowercase().as_str(),
                "source.md" | "sha256sums"
            ),
            "checksum entry collides with repository metadata: {relative}"
        );

        ensure!(
            entries
                .insert(relative.to_owned(), expected.to_ascii_lowercase())
                .is_none(),
            "duplicate checksum entry: {relative}"
        );
        let collision_key = relative.to_lowercase();
        if let Some(existing) = collision_keys.insert(collision_key, relative.to_owned()) {
            bail!("case-insensitive checksum path collision: {existing} and {relative}");
        }
    }

    Ok(entries)
}

fn validate_portable_relative_path(relative: &str) -> Result<()> {
    ensure!(!relative.is_empty(), "path is empty");
    for segment in relative.split('/') {
        validate_portable_segment(segment)?;
    }
    Ok(())
}

fn collect_files(
    directory: &Path,
    relative_directory: &str,
    files: &mut BTreeSet<String>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("cannot read directory {}", directory.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let name = entry.file_name().into_string().map_err(|_| {
            anyhow::anyhow!("snapshot path is not UTF-8: {}", entry.path().display())
        })?;
        validate_portable_segment(&name)
            .with_context(|| format!("unsafe path in snapshot: {}", entry.path().display()))?;
        let relative = if relative_directory.is_empty() {
            name
        } else {
            format!("{relative_directory}/{name}")
        };
        let file_type = entry
            .file_type()
            .with_context(|| format!("cannot inspect {}", entry.path().display()))?;

        if file_type.is_dir() {
            collect_files(&entry.path(), &relative, files)?;
        } else if file_type.is_file() {
            files.extend([relative]);
        } else {
            bail!(
                "unsupported filesystem entry in upstream snapshot: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn ensure_same_files(expected: &BTreeSet<String>, actual: &BTreeSet<String>) -> Result<()> {
    if expected == actual {
        return Ok(());
    }

    let missing = expected
        .difference(actual)
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    let unexpected = actual
        .difference(expected)
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    bail!(
        "upstream snapshot files do not match SHA256SUMS; missing: {}; unexpected: {}",
        display_entries(&missing),
        display_entries(&unexpected)
    )
}

fn display_entries(entries: &[String]) -> String {
    if entries.is_empty() {
        "<none>".to_owned()
    } else {
        entries.join(", ")
    }
}

pub(super) fn join_portable_path(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}

pub(super) fn validate_portable_segment(segment: &str) -> Result<()> {
    ensure!(
        !segment.is_empty() && segment != "." && segment != "..",
        "empty or relative path segment"
    );
    ensure!(
        !segment
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character)),
        "path segment contains a character unsupported on Windows: {segment}"
    );
    ensure!(
        !segment.ends_with(['.', ' ']),
        "path segment ends with a dot or space: {segment}"
    );

    let stem = segment.split('.').next().unwrap_or_default();
    let stem = stem.to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    ensure!(!reserved, "reserved Windows filename: {segment}");
    Ok(())
}

pub(super) fn ensure_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("file does not exist: {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "path is not a regular file: {}",
        path.display()
    );
    Ok(())
}

pub(super) fn temporary_directory(prefix: &str) -> Result<TempDir> {
    TempDirBuilder::new()
        .prefix(prefix)
        .tempdir()
        .with_context(|| format!("cannot create temporary directory for {prefix}"))
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(super) fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::Path,
    };

    use rstest::rstest;
    use tempfile::TempDir;

    use super::{
        RepositoryPaths, display_entries, display_path, ensure_regular_file, ensure_same_files,
        join_portable_path, parse_checksum_entries, sha256, temporary_directory,
        validate_existing_snapshot, validate_portable_segment,
    };

    const EXAMPLE_SCHEMA: &str = "type: object\n";
    const EXAMPLE_SCHEMA_SHA256: &str =
        "34ea4d90d89b8e77e1a6c1c6c5e16b5fc7b08ad25b5d244a4a3822c27e7c740d";

    fn create_valid_snapshot() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        let upstream = directory.path();
        fs::create_dir(upstream.join("schemas")).unwrap();
        fs::write(upstream.join("SOURCE.md"), "source metadata\n").unwrap();

        let schema = upstream.join("schemas/example.yml");
        fs::write(schema, EXAMPLE_SCHEMA).unwrap();
        fs::write(
            upstream.join("SHA256SUMS"),
            format!("{EXAMPLE_SCHEMA_SHA256}  schemas/example.yml\n"),
        )
        .unwrap();
        directory
    }

    fn checksum_entry(relative: &str) -> String {
        format!("{EXAMPLE_SCHEMA_SHA256}  {relative}\n")
    }

    #[test]
    fn discover_returns_all_repository_paths() {
        // Arrange
        let expected_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();

        // Act
        let paths = RepositoryPaths::discover().unwrap();

        // Assert
        assert_eq!(paths.root, expected_root);
        assert_eq!(paths.openapi, expected_root.join("openapi"));
        assert_eq!(paths.upstream, expected_root.join("openapi/upstream"));
        assert_eq!(paths.patches, expected_root.join("openapi/patches"));
        assert_eq!(
            paths.redocly_config,
            expected_root.join("openapi/redocly.yaml")
        );
        assert_eq!(
            paths.redocly_cli,
            expected_root.join("node_modules/@redocly/cli/bin/cli.js")
        );
    }

    #[test]
    fn parse_checksum_entries_skips_blank_lines() {
        // Arrange
        let contents = format!("\n{}\n", checksum_entry("schemas/example.yml"));

        // Act
        let entries = parse_checksum_entries(&contents).unwrap();

        // Assert
        assert_eq!(
            entries,
            BTreeMap::from([(
                "schemas/example.yml".to_owned(),
                EXAMPLE_SCHEMA_SHA256.to_owned()
            )])
        );
    }

    #[test]
    fn parse_checksum_entries_accepts_binary_markers_and_normalizes_digests() {
        // Arrange
        let uppercase_digest = EXAMPLE_SCHEMA_SHA256.to_ascii_uppercase();
        let contents = format!("{uppercase_digest} *schemas/example.yml\n");

        // Act
        let entries = parse_checksum_entries(&contents).unwrap();

        // Assert
        assert_eq!(
            entries.get("schemas/example.yml").map(String::as_str),
            Some(EXAMPLE_SCHEMA_SHA256)
        );
    }

    #[test]
    fn parse_checksum_entries_rejects_case_insensitive_path_collisions() {
        // Arrange
        let contents = format!(
            "{}{}",
            checksum_entry("schemas/example.yml"),
            checksum_entry("SCHEMAS/EXAMPLE.YML")
        );

        // Act
        let error = parse_checksum_entries(&contents).unwrap_err().to_string();

        // Assert
        assert_eq!(
            error,
            "case-insensitive checksum path collision: schemas/example.yml and SCHEMAS/EXAMPLE.YML"
        );
    }

    #[test]
    fn parse_checksum_entries_accepts_distinct_paths() {
        // Arrange
        let contents = format!(
            "{}{}",
            checksum_entry("schemas/first.yml"),
            checksum_entry("schemas/second.yml")
        );

        // Act
        let entries = parse_checksum_entries(&contents).unwrap();

        // Assert
        assert_eq!(
            entries.keys().map(String::as_str).collect::<Vec<_>>(),
            ["schemas/first.yml", "schemas/second.yml"]
        );
    }

    #[test]
    fn parse_checksum_entries_rejects_duplicate_paths() {
        // Arrange
        let contents = checksum_entry("schemas/example.yml").repeat(2);

        // Act
        let error = parse_checksum_entries(&contents).unwrap_err().to_string();

        // Assert
        assert_eq!(error, "duplicate checksum entry: schemas/example.yml");
    }

    #[test]
    fn parse_checksum_entries_rejects_repository_metadata() {
        // Arrange
        let contents = checksum_entry("source.MD");

        // Act
        let error = parse_checksum_entries(&contents).unwrap_err().to_string();

        // Assert
        assert_eq!(
            error,
            "checksum entry collides with repository metadata: source.MD"
        );
    }

    #[rstest]
    #[case::empty("")]
    #[case::current_directory(".")]
    #[case::parent_directory("..")]
    #[case::reserved_con("CON")]
    #[case::reserved_con_with_extension("con.txt")]
    #[case::reserved_conin_dollar("CONIN$")]
    #[case::reserved_conout_dollar_with_extension("conout$.txt")]
    #[case::reserved_prn_with_extension("PRN.yml")]
    #[case::reserved_com_ascii_digit("COM1")]
    #[case::reserved_com_superscript_digit("COM¹.log")]
    #[case::reserved_lpt_ascii_digit("LPT9.log")]
    #[case::reserved_lpt_superscript_digit("LPT³")]
    #[case::colon("bad:name")]
    #[case::backslash("bad\\name")]
    #[case::question_mark("bad?name")]
    #[case::control_character("bad\nname")]
    #[case::trailing_dot("trailing.")]
    #[case::trailing_space("trailing ")]
    fn validate_portable_segment_rejects_an_incompatible_segment(#[case] segment: &str) {
        // Arrange is provided by the named case.

        // Act
        let result = validate_portable_segment(segment);

        // Assert
        assert!(result.is_err(), "accepted {segment:?}");
    }

    #[rstest]
    #[case::file_with_extension("openapi.yml")]
    #[case::embedded_spaces("manuscript-stats by version.yml")]
    #[case::extensionless("complete")]
    fn validate_portable_segment_accepts_a_portable_segment(#[case] segment: &str) {
        // Arrange is provided by the named case.

        // Act
        let result = validate_portable_segment(segment);

        // Assert
        assert!(result.is_ok(), "rejected {segment:?}: {result:?}");
    }

    #[test]
    fn validate_existing_snapshot_ignores_empty_directories() {
        // Arrange
        let snapshot = create_valid_snapshot();
        fs::create_dir_all(snapshot.path().join("unused/empty")).unwrap();

        // Act
        let result = validate_existing_snapshot(snapshot.path());

        // Assert
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn validate_existing_snapshot_rejects_unlisted_regular_files() {
        // Arrange
        let snapshot = create_valid_snapshot();
        fs::write(snapshot.path().join("unlisted.yml"), "type: string\n").unwrap();

        // Act
        let result = validate_existing_snapshot(snapshot.path());

        // Assert
        let error = result.unwrap_err().to_string();
        assert!(error.contains("unexpected: unlisted.yml"), "{error}");
    }

    #[test]
    fn validate_existing_snapshot_returns_snapshot_contents() {
        // Arrange
        let snapshot = create_valid_snapshot();
        let expected_checksums = checksum_entry("schemas/example.yml").into_bytes();

        // Act
        let result = validate_existing_snapshot(snapshot.path()).unwrap();

        // Assert
        assert_eq!(result.source, b"source metadata\n");
        assert_eq!(result.checksums, expected_checksums);
        assert_eq!(
            result.checked_files,
            BTreeSet::from(["schemas/example.yml".to_owned()])
        );
    }

    #[test]
    fn validate_existing_snapshot_rejects_missing_checked_files() {
        // Arrange
        let snapshot = create_valid_snapshot();
        fs::remove_file(snapshot.path().join("schemas/example.yml")).unwrap();

        // Act
        let error = validate_existing_snapshot(snapshot.path())
            .unwrap_err()
            .to_string();

        // Assert
        assert_eq!(
            error,
            "upstream snapshot files do not match SHA256SUMS; missing: schemas/example.yml; unexpected: <none>"
        );
    }

    #[test]
    fn validate_existing_snapshot_rejects_checksum_mismatches() {
        // Arrange
        let snapshot = create_valid_snapshot();
        fs::write(
            snapshot.path().join("schemas/example.yml"),
            "type: string\n",
        )
        .unwrap();

        // Act
        let error = validate_existing_snapshot(snapshot.path())
            .unwrap_err()
            .to_string();

        // Assert
        assert!(error.starts_with("checksum mismatch for schemas/example.yml:"));
        assert!(error.contains(EXAMPLE_SCHEMA_SHA256));
    }

    #[test]
    fn ensure_same_files_reports_missing_and_unexpected_files() {
        // Arrange
        let expected = BTreeSet::from(["missing.yml".to_owned(), "shared.yml".to_owned()]);
        let actual = BTreeSet::from(["shared.yml".to_owned(), "unexpected.yml".to_owned()]);

        // Act
        let error = ensure_same_files(&expected, &actual)
            .unwrap_err()
            .to_string();

        // Assert
        assert_eq!(
            error,
            "upstream snapshot files do not match SHA256SUMS; missing: missing.yml; unexpected: unexpected.yml"
        );
    }

    #[test]
    fn display_entries_renders_none_for_an_empty_slice() {
        // Arrange
        let entries = Vec::new();

        // Act
        let display = display_entries(&entries);

        // Assert
        assert_eq!(display, "<none>");
    }

    #[test]
    fn display_entries_joins_multiple_entries() {
        // Arrange
        let entries = vec!["one.yml".to_owned(), "two.yml".to_owned()];

        // Act
        let display = display_entries(&entries);

        // Assert
        assert_eq!(display, "one.yml, two.yml");
    }

    #[test]
    fn join_portable_path_appends_forward_slash_separated_segments() {
        // Arrange
        let root = Path::new("root");

        // Act
        let path = join_portable_path(root, "schemas/nested/example.yml");

        // Assert
        assert_eq!(path, root.join("schemas/nested/example.yml"));
    }

    #[test]
    fn ensure_regular_file_accepts_a_regular_file() {
        // Arrange
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("example.yml");
        fs::write(&file, EXAMPLE_SCHEMA).unwrap();

        // Act
        let result = ensure_regular_file(&file);

        // Assert
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn ensure_regular_file_rejects_a_directory() {
        // Arrange
        let directory = tempfile::tempdir().unwrap();

        // Act
        let error = ensure_regular_file(directory.path())
            .unwrap_err()
            .to_string();

        // Assert
        assert!(error.starts_with("path is not a regular file:"));
    }

    #[test]
    fn temporary_directory_creates_a_prefixed_directory() {
        // Arrange
        let prefix = "shared-test";

        // Act
        let directory = temporary_directory(prefix).unwrap();

        // Assert
        assert!(directory.path().is_dir());
        assert!(
            directory
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(prefix)
        );
    }

    #[test]
    fn sha256_returns_the_file_digest() {
        // Arrange
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("example.yml");
        fs::write(&file, EXAMPLE_SCHEMA).unwrap();

        // Act
        let digest = sha256(&file).unwrap();

        // Assert
        assert_eq!(digest, EXAMPLE_SCHEMA_SHA256);
    }

    #[test]
    fn display_path_returns_a_relative_path_below_the_root() {
        // Arrange
        let root = Path::new("repository");
        let path = root.join("openapi/upstream");

        // Act
        let display = display_path(root, &path);

        // Assert
        assert_eq!(display, "openapi/upstream");
    }

    #[test]
    fn display_path_keeps_a_path_outside_the_root() {
        // Arrange
        let root = Path::new("repository");
        let path = Path::new("elsewhere/openapi.yml");

        // Act
        let display = display_path(root, path);

        // Assert
        assert_eq!(display, "elsewhere/openapi.yml");
    }
}
