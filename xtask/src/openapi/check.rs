use std::fs;

use anyhow::{Context, Result, bail, ensure};
use diffy::DiffOptions;
use openapiv3::OpenAPI;
use progenitor::{GenerationSettings, Generator, InterfaceStyle, TagStyle};

use super::{
    bundle::build_bundle,
    shared::{
        BUNDLE_NAME, RepositoryPaths, display_path, temporary_directory, validate_existing_snapshot,
    },
};

pub(super) fn run() -> Result<()> {
    let paths = RepositoryPaths::discover()?;
    let checksums = paths.upstream.join("SHA256SUMS");
    let committed_bundle = paths.openapi.join(BUNDLE_NAME);

    ensure!(
        checksums.is_file(),
        "missing {}; refresh the upstream snapshot first",
        display_path(&paths.root, &checksums)
    );
    ensure!(
        committed_bundle.is_file(),
        "missing {}; run 'cargo xtask openapi bundle' first",
        display_path(&paths.root, &committed_bundle)
    );

    let snapshot = validate_existing_snapshot(&paths.upstream)?;
    for relative in snapshot.checked_files {
        println!("{relative}: OK");
    }

    let check_dir = temporary_directory("world-anvil-openapi-check")?;
    let rebuilt_bundle = check_dir.path().join(BUNDLE_NAME);
    build_bundle(&paths, &rebuilt_bundle)?;

    let committed = fs::read(&committed_bundle)
        .with_context(|| format!("cannot read {}", committed_bundle.display()))?;
    let rebuilt = fs::read(&rebuilt_bundle)
        .with_context(|| format!("cannot read {}", rebuilt_bundle.display()))?;

    if committed != rebuilt {
        let committed_text =
            String::from_utf8(committed).context("the committed OpenAPI bundle is not UTF-8")?;
        let rebuilt_text =
            String::from_utf8(rebuilt).context("the rebuilt OpenAPI bundle is not UTF-8")?;
        let diff = DiffOptions::new()
            .set_original_filename("committed")
            .set_modified_filename("rebuilt")
            .create_patch(&committed_text, &rebuilt_text);

        eprintln!(
            "openapi check: committed bundle is stale; rebuild it with \
             'cargo xtask openapi bundle'"
        );
        eprintln!("{diff}");
        bail!("OpenAPI bundle is not reproducible");
    }

    println!("OpenAPI bundle is reproducible.");
    smoke_generate(&rebuilt)?;
    println!("Progenitor smoke generation succeeded.");

    Ok(())
}

fn smoke_generate(specification: &[u8]) -> Result<()> {
    let api: OpenAPI = serde_json::from_slice(specification)
        .context("cannot parse the rebuilt bundle as OpenAPI 3")?;

    let mut settings = GenerationSettings::default();
    settings
        .with_interface(InterfaceStyle::Builder)
        .with_tag(TagStyle::Separate);
    let mut generator = Generator::new(&settings);
    let tokens = generator
        .generate_tokens(&api)
        .context("Progenitor could not generate Rust bindings")?;
    syn::parse2::<syn::File>(tokens).context("Progenitor produced invalid Rust syntax")?;

    Ok(())
}
