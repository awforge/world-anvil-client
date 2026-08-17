use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use percent_encoding::percent_decode_str;
use reqwest::{StatusCode, Url, blocking::Client, header::ACCEPT_ENCODING, redirect::Policy};
use sha2::{Digest, Sha256};
use tempfile::{Builder as TempDirBuilder, TempDir};

use super::shared::{
    ExistingSnapshot, RepositoryPaths, display_path, join_portable_path,
    validate_existing_snapshot, validate_portable_segment,
};

const UPSTREAM_SOURCE_ROOT: &str =
    "https://wa-cdn.nyc3.cdn.digitaloceanspaces.com/assets/prod/boromir-documentation/swagger/";
const UPSTREAM_ENTRYPOINT: &str = "openapi.yml";
const FETCH_USER_AGENT: &str = "world-anvil-client OpenAPI snapshot fetcher";
const MAX_UPSTREAM_FILES: usize = 1_024;
const MAX_UPSTREAM_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_UPSTREAM_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_FETCH_DURATION: Duration = Duration::from_secs(5 * 60);

pub(super) fn run() -> Result<()> {
    let paths = RepositoryPaths::discover()?;
    let baseline = validate_existing_snapshot(&paths.upstream)?;
    let source_root =
        Url::parse(UPSTREAM_SOURCE_ROOT).context("invalid upstream source-root URL")?;
    let entrypoint = source_root
        .join(UPSTREAM_ENTRYPOINT)
        .context("invalid upstream entrypoint URL")?;
    ensure!(
        entrypoint.scheme() == "https" && source_root.scheme() == "https",
        "the upstream OpenAPI source must use HTTPS"
    );

    let client = Client::builder()
        .user_agent(FETCH_USER_AGENT)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate()
        .retry(reqwest::retry::never())
        .build()
        .context("cannot create the upstream HTTP client")?;

    let snapshot = crawl_snapshot(&entrypoint, &source_root, |url| {
        download_upstream_file(&client, url)
    })?;

    promote_snapshot(&paths, &snapshot, &baseline)?;
    println!(
        "Fetched {} OpenAPI files into {}",
        snapshot.len(),
        display_path(&paths.root, &paths.upstream)
    );
    println!(
        "Review the upstream diff and update retrieval metadata in openapi/upstream/SOURCE.md."
    );

    Ok(())
}

fn crawl_snapshot<F>(
    entrypoint: &Url,
    source_root: &Url,
    download: F,
) -> Result<BTreeMap<String, Vec<u8>>>
where
    F: FnMut(&Url) -> Result<Vec<u8>>,
{
    crawl_snapshot_with_limits(
        entrypoint,
        source_root,
        MAX_UPSTREAM_FILE_BYTES,
        MAX_UPSTREAM_TOTAL_BYTES,
        download,
    )
}

fn crawl_snapshot_with_limits<F>(
    entrypoint: &Url,
    source_root: &Url,
    max_file_bytes: usize,
    max_total_bytes: usize,
    mut download: F,
) -> Result<BTreeMap<String, Vec<u8>>>
where
    F: FnMut(&Url) -> Result<Vec<u8>>,
{
    ensure!(
        source_root.path().ends_with('/'),
        "upstream source-root URL must end with '/'"
    );
    validate_source_url(source_root, source_root)?;

    let mut pending = VecDeque::new();
    let mut scheduled = BTreeSet::new();
    schedule_upstream_url(
        entrypoint.clone(),
        source_root,
        &mut pending,
        &mut scheduled,
    )?;

    let mut files = BTreeMap::new();
    let mut total_bytes = 0_usize;
    let started = Instant::now();

    while let Some((source, relative)) = pending.pop_front() {
        ensure!(
            started.elapsed() <= MAX_FETCH_DURATION,
            "upstream fetch exceeded the {}-second overall time limit",
            MAX_FETCH_DURATION.as_secs()
        );
        let bytes = download(&source)
            .with_context(|| format!("cannot download upstream OpenAPI file {source}"))?;
        ensure!(
            bytes.len() <= max_file_bytes,
            "upstream file exceeds the {}-byte limit: {source}",
            max_file_bytes
        );
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .context("upstream snapshot byte count overflowed")?;
        ensure!(
            total_bytes <= max_total_bytes,
            "upstream snapshot exceeds the {}-byte limit",
            max_total_bytes
        );

        if source == *entrypoint {
            validate_entrypoint_document(&bytes)?;
        }

        let references = external_references(&bytes)
            .with_context(|| format!("cannot parse references in {source}"))?;
        ensure!(
            files.insert(relative, bytes).is_none(),
            "two upstream URLs resolved to the same destination"
        );

        for reference in references {
            let mut referenced_url = source
                .join(&reference)
                .with_context(|| format!("invalid $ref URL in {source}: {reference}"))?;
            referenced_url.set_fragment(None);
            schedule_upstream_url(referenced_url, source_root, &mut pending, &mut scheduled)?;
        }
    }

    Ok(files)
}

fn schedule_upstream_url(
    mut url: Url,
    source_root: &Url,
    pending: &mut VecDeque<(Url, String)>,
    scheduled: &mut BTreeSet<String>,
) -> Result<()> {
    url.set_fragment(None);
    validate_source_url(&url, source_root)?;
    let relative = source_path(&url, source_root)?;
    let url_key = url.as_str().to_owned();

    if scheduled.contains(&url_key) {
        return Ok(());
    }

    ensure!(
        scheduled.len() < MAX_UPSTREAM_FILES,
        "upstream reference graph exceeds the {MAX_UPSTREAM_FILES}-file limit"
    );

    scheduled.extend([url_key]);
    pending.push_back((url, relative));
    Ok(())
}

fn validate_source_url(url: &Url, source_root: &Url) -> Result<()> {
    ensure!(
        url.origin() == source_root.origin(),
        "refusing to fetch reference outside {source_root}: {url}"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "refusing URL containing credentials: {url}"
    );
    ensure!(
        url.query().is_none(),
        "refusing URL containing a query: {url}"
    );
    ensure!(
        url.fragment().is_none(),
        "upstream URL fragment was not removed: {url}"
    );
    ensure!(
        url.path().starts_with(source_root.path()),
        "refusing to fetch reference outside {source_root}: {url}"
    );
    Ok(())
}

fn source_path(url: &Url, source_root: &Url) -> Result<String> {
    let encoded = url
        .path()
        .strip_prefix(source_root.path())
        .with_context(|| format!("URL is outside the source root: {url}"))?;
    ensure!(
        !encoded.is_empty(),
        "upstream URL does not name a file: {url}"
    );

    let mut segments = Vec::new();
    for encoded_segment in encoded.split('/') {
        let segment = percent_decode_str(encoded_segment)
            .decode_utf8()
            .with_context(|| format!("upstream path is not UTF-8: {url}"))?;
        validate_portable_segment(&segment)
            .with_context(|| format!("unsafe upstream path in {url}"))?;
        segments.push(segment.into_owned());
    }

    let relative = segments.join("/");
    let first_segment = segments.first().context("upstream URL path is empty")?;
    ensure!(
        !matches!(
            first_segment.to_ascii_lowercase().as_str(),
            "source.md" | "sha256sums"
        ),
        "upstream URL collides with repository metadata: {url}"
    );
    Ok(relative)
}

fn external_references(bytes: &[u8]) -> Result<Vec<String>> {
    let document = std::str::from_utf8(bytes).context("upstream YAML is not UTF-8")?;
    ensure!(
        !document.trim().is_empty(),
        "upstream YAML document is empty"
    );
    let mut references = Vec::new();
    for line in document.lines() {
        let line = line.trim_start();
        let line = line
            .strip_prefix('-')
            .map_or(line, |remainder| remainder.trim_start());
        let Some(value) = line.strip_prefix("$ref:") else {
            continue;
        };
        let mut value = value.trim();

        if ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
            && value.len() >= 2
        {
            value = &value[1..value.len() - 1];
        } else if let Some(comment) = yaml_comment_start(value) {
            value = value[..comment].trim_end();
        }

        let file = value.split('#').next().unwrap_or_default();
        if !file.is_empty() {
            references.push(file.to_owned());
        }
    }
    Ok(references)
}

fn validate_entrypoint_document(bytes: &[u8]) -> Result<()> {
    let document = std::str::from_utf8(bytes).context("upstream entrypoint is not UTF-8")?;
    let mut top_level = BTreeMap::new();
    for line in document.lines() {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            top_level.extend([(key.trim(), value.trim())]);
        }
    }

    let version = top_level
        .get("openapi")
        .context("upstream entrypoint has no top-level openapi version")?;
    let version = version.trim_matches(['\'', '"']);
    ensure!(
        version.starts_with("3.0."),
        "unsupported upstream OpenAPI version: {version}"
    );
    for required in ["info", "paths", "components"] {
        ensure!(
            top_level.contains_key(required),
            "upstream entrypoint has no top-level {required} section"
        );
    }
    Ok(())
}

fn yaml_comment_start(value: &str) -> Option<usize> {
    value.char_indices().find_map(|(index, character)| {
        (character == '#'
            && value[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace))
        .then_some(index)
    })
}

fn download_upstream_file(client: &Client, url: &Url) -> Result<Vec<u8>> {
    let response = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .with_context(|| format!("request failed for {url}"))?;
    let status = response.status();
    read_upstream_response(response, status, url, MAX_UPSTREAM_FILE_BYTES)
}

fn read_upstream_response<R>(
    response: R,
    status: StatusCode,
    url: &Url,
    max_file_bytes: usize,
) -> Result<Vec<u8>>
where
    R: Read,
{
    ensure!(
        status == StatusCode::OK,
        "failed to fetch {url}: expected HTTP 200, got HTTP {status}"
    );

    let mut bytes = Vec::new();
    response
        .take(max_file_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read response body from {url}"))?;
    ensure!(
        bytes.len() <= max_file_bytes,
        "upstream file exceeds the {}-byte limit: {url}",
        max_file_bytes
    );
    Ok(bytes)
}

fn checksum_document(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut document = String::new();
    let mut entries = files.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    for (relative, bytes) in entries {
        document.push_str(&sha256_bytes(bytes));
        document.push_str("  ");
        document.push_str(relative);
        document.push('\n');
    }
    document
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn promote_snapshot(
    paths: &RepositoryPaths,
    files: &BTreeMap<String, Vec<u8>>,
    baseline: &ExistingSnapshot,
) -> Result<()> {
    let staging = TempDirBuilder::new()
        .prefix(".upstream-fetch-")
        .tempdir_in(&paths.openapi)
        .context("cannot create an OpenAPI staging directory")?;
    let stage = staging.path().join("stage");
    fs::create_dir(&stage)
        .with_context(|| format!("cannot create staged snapshot at {}", stage.display()))?;

    for (relative, bytes) in files {
        let destination = join_portable_path(&stage, relative);
        let parent = destination
            .parent()
            .context("staged upstream file has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
        fs::write(&destination, bytes)
            .with_context(|| format!("cannot stage {}", destination.display()))?;
    }
    fs::write(stage.join("SOURCE.md"), &baseline.source)
        .context("cannot preserve upstream SOURCE.md")?;
    fs::write(stage.join("SHA256SUMS"), checksum_document(files))
        .context("cannot stage upstream SHA256SUMS")?;
    validate_existing_snapshot(&stage).context("staged upstream snapshot is invalid")?;

    let current = validate_existing_snapshot(&paths.upstream)?;
    ensure!(
        current == *baseline,
        "the vendored upstream snapshot changed while it was being refreshed; retry after reviewing those changes"
    );

    let backup = staging.path().join("backup");

    if let Err(error) = fs::rename(&paths.upstream, &backup) {
        return Err(error)
            .with_context(|| format!("cannot move the existing snapshot to {}", backup.display()));
    }

    if let Err(promote_error) = fs::rename(&stage, &paths.upstream) {
        match fs::rename(&backup, &paths.upstream) {
            Ok(()) => {
                return Err(promote_error)
                    .context("cannot promote the staged snapshot; restored the previous snapshot");
            }
            Err(rollback_error) => {
                return Err(preserve_failed_snapshot(
                    staging,
                    promote_error,
                    rollback_error,
                ));
            }
        }
    }

    Ok(())
}

fn preserve_failed_snapshot(
    staging: TempDir,
    promote_error: std::io::Error,
    rollback_error: std::io::Error,
) -> anyhow::Error {
    let staging_path = staging.keep();
    anyhow::anyhow!(
        "cannot promote staged snapshot ({promote_error}) and cannot restore the old snapshot \
         ({rollback_error}); recovery data remains at {}",
        staging_path.display()
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, io::Cursor, path::Path};

    use rstest::rstest;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use super::*;

    fn test_paths(root: &Path) -> RepositoryPaths {
        let root = root.to_path_buf();
        let openapi = root.join("openapi");
        RepositoryPaths {
            upstream: openapi.join("upstream"),
            patches: openapi.join("patches"),
            redocly_config: openapi.join("redocly.yaml"),
            redocly_cli: root.join("node_modules/@redocly/cli/bin/cli.js"),
            root,
            openapi,
        }
    }

    fn fixture_checksum_document(files: &BTreeMap<String, Vec<u8>>) -> String {
        files
            .iter()
            .map(|(relative, bytes)| {
                format!("{}  {relative}\n", hex::encode(Sha256::digest(bytes)))
            })
            .collect()
    }

    fn write_snapshot(upstream: &Path, source: &[u8], files: &BTreeMap<String, Vec<u8>>) {
        fs::create_dir_all(upstream).unwrap();
        fs::write(upstream.join("SOURCE.md"), source).unwrap();
        fs::write(
            upstream.join("SHA256SUMS"),
            fixture_checksum_document(files),
        )
        .unwrap();
        for (relative, bytes) in files {
            let path = upstream.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn external_references_discovers_yaml_references_recursively() {
        // Arrange
        let document = br#"
paths:
  article:
    $ref: 'parts/article.yml#/Article'
items:
  - $ref: "parts/item.yml"
internal:
  $ref: '#/components/schemas/Internal'
literal: '$ref: ignored.yml'
"#;

        // Act
        let result = external_references(document);

        // Assert
        let references = result.unwrap();
        assert_eq!(references, ["parts/article.yml", "parts/item.yml"]);
    }

    #[test]
    fn external_references_preserves_unquoted_and_unbalanced_values() {
        // Arrange
        let document = br#"
plain:
  $ref: plain.yml
single:
  $ref: 'single.yml
double:
  $ref: "double.yml
"#;

        // Act
        let result = external_references(document);

        // Assert
        assert_eq!(
            result.unwrap(),
            ["plain.yml", "'single.yml", "\"double.yml"]
        );
    }

    #[test]
    fn external_references_ignores_an_empty_quoted_value() {
        // Arrange
        let document = b"empty:\n  $ref: ''\nvalid:\n  $ref: valid.yml\n";

        // Act
        let result = external_references(document);

        // Assert
        assert_eq!(result.unwrap(), ["valid.yml"]);
    }

    #[test]
    fn external_references_removes_an_inline_yaml_comment() {
        // Arrange
        let document = b"value:\n  $ref: parts/item.yml   # explanation\n";

        // Act
        let result = external_references(document);

        // Assert
        assert_eq!(result.unwrap(), ["parts/item.yml"]);
    }

    #[test]
    fn yaml_comment_start_finds_a_whitespace_delimited_comment() {
        // Arrange
        let value = "schema.yml # explanation";

        // Act
        let result = yaml_comment_start(value);

        // Assert
        assert_eq!(result, Some(11));
    }

    #[test]
    fn validate_entrypoint_document_rejects_a_missing_openapi_version() {
        // Arrange
        let document = b"<html>temporary error</html>\n";

        // Act
        let result = validate_entrypoint_document(document);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn validate_entrypoint_document_rejects_an_unsupported_openapi_version() {
        // Arrange
        let document = b"openapi: 3.1.0\ninfo: {}\npaths: {}\ncomponents: {}\n";

        // Act
        let result = validate_entrypoint_document(document);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn validate_entrypoint_document_rejects_indented_top_level_sections() {
        // Arrange
        let document = b"  openapi: 3.0.3\n  info: {}\n  paths: {}\n  components: {}\n";

        // Act
        let result = validate_entrypoint_document(document);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn validate_entrypoint_document_skips_nested_lines_before_top_level_sections() {
        // Arrange
        let document = b"  nested: true\nopenapi: 3.0.3\ninfo: {}\npaths: {}\ncomponents: {}\n";

        // Act
        let result = validate_entrypoint_document(document);

        // Assert
        result.unwrap();
    }

    #[test]
    fn crawl_snapshot_visits_cycles_once_and_preserves_downloaded_bytes() {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let entrypoint = root.join("openapi.yml").unwrap();
        let article = root.join("parts/article.yml").unwrap();
        let entrypoint_bytes = b"openapi: 3.0.3\ninfo: {}\npaths: {}\ncomponents: {}\none:\n  $ref: 'parts/article.yml#/One'\ntwo:\n  $ref: 'parts/article.yml#/Two'\n".to_vec();
        let article_bytes = b"root:\n  $ref: '../openapi.yml#/Root'\n".to_vec();
        let responses = BTreeMap::from([
            (entrypoint.to_string(), entrypoint_bytes.clone()),
            (article.to_string(), article_bytes.clone()),
        ]);
        let mut requests = BTreeMap::<String, usize>::new();

        // Act
        let result = crawl_snapshot(&entrypoint, &root, |url| {
            *requests.entry(url.to_string()).or_default() += 1;
            Ok(responses.get(url.as_str()).unwrap().clone())
        });

        // Assert
        let snapshot = result.unwrap();
        assert_eq!(snapshot.get("openapi.yml"), Some(&entrypoint_bytes));
        assert_eq!(snapshot.get("parts/article.yml"), Some(&article_bytes));
        assert_eq!(requests.values().copied().collect::<Vec<_>>(), [1, 1]);
    }

    #[test]
    fn crawl_snapshot_rejects_a_snapshot_over_the_total_byte_limit() {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let entrypoint = root.join("openapi.yml").unwrap();
        let child = root.join("child.yml").unwrap();
        let entrypoint_bytes =
            b"openapi: 3.0.3\ninfo: {}\npaths: {}\ncomponents: {}\nchild:\n  $ref: child.yml\n"
                .to_vec();
        let child_bytes = b"value: true\n".to_vec();
        let total_limit = entrypoint_bytes.len() + child_bytes.len() - 1;
        let responses = BTreeMap::from([
            (entrypoint.to_string(), entrypoint_bytes),
            (child.to_string(), child_bytes),
        ]);

        // Act
        let result =
            crawl_snapshot_with_limits(&entrypoint, &root, usize::MAX, total_limit, |url| {
                Ok(responses.get(url.as_str()).unwrap().clone())
            });

        // Assert
        assert!(result.unwrap_err().to_string().contains("snapshot exceeds"));
    }

    #[test]
    fn source_path_normalizes_paths_within_the_source_root() {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let nested = root.join("parts/nested/article.yml").unwrap();
        let inside = nested.join("../../schemas/date.yml").unwrap();

        // Act
        let result = source_path(&inside, &root);

        // Assert
        assert_eq!(result.unwrap(), "schemas/date.yml");
    }

    #[rstest]
    #[case::parent_path("../outside.yml")]
    #[case::different_origin("https://other.test/swagger/file.yml")]
    fn validate_source_url_rejects_a_url_outside_the_source_root(#[case] reference: &str) {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let url = root.join(reference).unwrap();

        // Act
        let result = validate_source_url(&url, &root);

        // Assert
        assert!(result.is_err(), "accepted {url}");
    }

    #[rstest]
    #[case::encoded_path_separator("parts%2Ffile.yml")]
    #[case::windows_reserved_name("CON.yml")]
    fn source_path_rejects_a_nonportable_url_segment(#[case] relative: &str) {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let url = root.join(relative).unwrap();

        // Act
        let result = source_path(&url, &root);

        // Assert
        assert!(result.is_err(), "accepted {url}");
    }

    #[test]
    fn validate_source_url_rejects_urls_with_queries() {
        // Arrange
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let url = Url::parse("https://example.test/swagger/file.yml?version=1").unwrap();

        // Act
        let result = validate_source_url(&url, &root);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn promote_snapshot_promotes_a_complete_snapshot_and_removes_stale_files() {
        // Arrange
        let temporary = tempdir().unwrap();
        let paths = test_paths(temporary.path());
        let source = b"provenance\n";
        let old_files = BTreeMap::from([("stale.yml".to_owned(), b"old: true\n".to_vec())]);
        write_snapshot(&paths.upstream, source, &old_files);

        let new_files = BTreeMap::from([
            ("openapi.yml".to_owned(), b"openapi: 3.0.3\n".to_vec()),
            (
                "parts/article.yml".to_owned(),
                b"Article: { type: object }\n".to_vec(),
            ),
        ]);
        let baseline = validate_existing_snapshot(&paths.upstream).unwrap();

        // Act
        let result = promote_snapshot(&paths, &new_files, &baseline);

        // Assert
        result.unwrap();
        validate_existing_snapshot(&paths.upstream).unwrap();
        assert_eq!(fs::read(paths.upstream.join("SOURCE.md")).unwrap(), source);
        assert!(!paths.upstream.join("stale.yml").exists());
        for (relative, expected) in new_files {
            assert_eq!(fs::read(paths.upstream.join(relative)).unwrap(), expected);
        }
        assert_eq!(fs::read_dir(&paths.openapi).unwrap().count(), 1);
    }

    #[test]
    fn promote_snapshot_refuses_a_snapshot_changed_after_validation() {
        // Arrange
        let temporary = tempdir().unwrap();
        let paths = test_paths(temporary.path());
        let source = b"provenance\n";
        let old_files = BTreeMap::from([("openapi.yml".to_owned(), b"old: true\n".to_vec())]);
        write_snapshot(&paths.upstream, source, &old_files);
        let baseline = validate_existing_snapshot(&paths.upstream).unwrap();

        let local_change = b"locally modified\n";
        let local_files = BTreeMap::from([("openapi.yml".to_owned(), local_change.to_vec())]);
        write_snapshot(&paths.upstream, source, &local_files);
        let new_files = BTreeMap::from([("openapi.yml".to_owned(), b"new: true\n".to_vec())]);

        // Act
        let result = promote_snapshot(&paths, &new_files, &baseline);

        // Assert
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("changed while it was being refreshed")
        );
        assert_eq!(
            fs::read(paths.upstream.join("openapi.yml")).unwrap(),
            local_change
        );
        assert_eq!(fs::read_dir(&paths.openapi).unwrap().count(), 1);
    }

    #[test]
    fn preserve_failed_snapshot_keeps_recovery_data_and_reports_its_path() {
        // Arrange
        let temporary = tempdir().unwrap();
        let staging = TempDirBuilder::new()
            .prefix("recovery-")
            .tempdir_in(temporary.path())
            .unwrap();
        let recovery_path = staging.path().to_path_buf();
        fs::write(staging.path().join("snapshot.yml"), b"recovery data\n").unwrap();
        let promote_error = std::io::Error::other("promotion failed");
        let rollback_error = std::io::Error::other("rollback failed");

        // Act
        let error = preserve_failed_snapshot(staging, promote_error, rollback_error);

        // Assert
        assert!(recovery_path.join("snapshot.yml").is_file());
        assert!(
            error
                .to_string()
                .contains(&recovery_path.display().to_string())
        );
    }

    #[test]
    fn read_upstream_response_rejects_a_non_success_status() {
        // Arrange
        let response = Cursor::new(b"redirect body");
        let url = Url::parse("https://example.test/openapi.yml").unwrap();

        // Act
        let result = read_upstream_response(response, StatusCode::FOUND, &url, 16);

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected HTTP 200")
        );
    }

    #[test]
    fn read_upstream_response_rejects_an_oversized_body() {
        // Arrange
        let response = Cursor::new(b"12345678901234567");
        let url = Url::parse("https://example.test/openapi.yml").unwrap();

        // Act
        let result = read_upstream_response(response, StatusCode::OK, &url, 16);

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("upstream file exceeds")
        );
    }

    #[test]
    fn read_upstream_response_returns_a_body_at_the_size_limit() {
        // Arrange
        let expected = b"1234567890123456";
        let response = Cursor::new(expected);
        let url = Url::parse("https://example.test/openapi.yml").unwrap();

        // Act
        let result = read_upstream_response(response, StatusCode::OK, &url, expected.len());

        // Assert
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn checksum_document_sorts_paths_case_insensitively_and_hashes_contents() {
        // Arrange
        let files = BTreeMap::from([
            ("B.yml".to_owned(), b"second\n".to_vec()),
            ("a.yml".to_owned(), b"first\n".to_vec()),
        ]);
        let expected = concat!(
            "b640e840b19d378660b32fb51ae18d67dccb4a8596a29e7bd72c1b2ae5928f41  a.yml\n",
            "480c2336b410f1ad5f8bf1b28944490255804b65350c527787e74ebdd511e3a4  B.yml\n",
        );

        // Act
        let result = checksum_document(&files);

        // Assert
        assert_eq!(result, expected);
    }

    #[test]
    fn crawl_snapshot_reconstructs_a_disk_backed_reference_graph() {
        // Arrange
        let temporary = tempdir().unwrap();
        let paths = test_paths(temporary.path());
        let root = Url::parse("https://example.test/swagger/").unwrap();
        let entrypoint = root.join("openapi.yml").unwrap();
        let files = BTreeMap::from([
            (
                "openapi.yml".to_owned(),
                b"openapi: 3.0.3\ninfo: {}\npaths: {}\ncomponents: {}\narticle:\n  $ref: 'parts/article.yml'\n"
                    .to_vec(),
            ),
            (
                "parts/article.yml".to_owned(),
                b"Article: { type: object }\n".to_vec(),
            ),
        ]);
        write_snapshot(&paths.upstream, b"source metadata\n", &files);

        // Act
        let result = crawl_snapshot(&entrypoint, &root, |url| {
            let relative = source_path(url, &root)?;
            Ok(fs::read(paths.upstream.join(relative))?)
        });

        // Assert
        assert_eq!(result.unwrap(), files);
    }
}
