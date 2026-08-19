use progenitor::{GenerationSettings, InterfaceStyle, TagStyle};

pub(crate) fn generation_settings() -> GenerationSettings {
    let mut settings = GenerationSettings::default();
    settings
        .with_interface(InterfaceStyle::Builder)
        .with_tag(TagStyle::Separate)
        .with_inner_type(
            "crate::Credentials"
                .parse()
                .expect("the credentials type path is valid Rust"),
        )
        .with_pre_hook_async(
            "crate::inject_credentials"
                .parse()
                .expect("the credential hook path is valid Rust"),
        );
    settings
}
