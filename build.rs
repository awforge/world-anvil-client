use std::{
    env,
    error::Error,
    fs::{self, File},
    path::PathBuf,
};

mod codegen;

use openapiv3::OpenAPI;
use progenitor::Generator;

const SPECIFICATION: &str = "openapi/world-anvil.openapi.json";
const OUTPUT: &str = "world_anvil.rs";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={SPECIFICATION}");

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let spec_path = manifest_dir.join(SPECIFICATION);
    let spec: OpenAPI = serde_json::from_reader(File::open(&spec_path)?)?;

    let settings = codegen::generation_settings();
    let mut generator = Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec)?;
    let syntax: syn::File = syn::parse2(tokens)?;
    let source = prettyplease::unparse(&syntax);

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"));
    fs::write(output_dir.join(OUTPUT), source)?;

    Ok(())
}
