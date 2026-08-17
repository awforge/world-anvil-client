use std::{
    env,
    error::Error,
    fs::{self, File},
    path::PathBuf,
};

use openapiv3::OpenAPI;
use progenitor::{GenerationSettings, Generator, InterfaceStyle, TagStyle};

const SPECIFICATION: &str = "openapi/world-anvil.openapi.json";
const OUTPUT: &str = "world_anvil.rs";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={SPECIFICATION}");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"));
    let specification_path = manifest_dir.join(SPECIFICATION);

    let specification: OpenAPI =
        serde_json::from_reader(File::open(&specification_path)?)?;

    let mut settings = GenerationSettings::default();
    settings
        .with_interface(InterfaceStyle::Builder)
        .with_tag(TagStyle::Separate);

    let mut generator = Generator::new(&settings);
    let tokens = generator.generate_tokens(&specification)?;
    let syntax: syn::File = syn::parse2(tokens)?;
    let source = prettyplease::unparse(&syntax);

    let output_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"));
    fs::write(output_dir.join(OUTPUT), source)?;

    Ok(())
}