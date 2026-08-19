# World Anvil Client for Rust

An unofficial Rust client for the World Anvil API.

## Usage

```rust,ignore
use world_anvil_client::{Client, Credentials};
use world_anvil_client::api::ClientUserExt;

let credentials = Credentials::new(application_key, authentication_token)?;
let client = Client::world_anvil(credentials);
let identity = client.read_identity().send().await?.into_inner();
```

Use `Client::new(base_url, credentials)` for a custom endpoint. API methods are
provided by extension traits such as `ClientUserExt`.

## OpenAPI generation

The vendored upstream description, compatibility patches, and reproducible Rust code-generation commands are documented in [`openapi/README.md`](openapi/README.md).

## Licensing and service terms

The original source code in this repository is licensed under either the MIT
License or the Apache License 2.0, at your option.

This license does not grant rights to the World Anvil name, trademarks, service,
API access, documentation, or user content. Use of the World Anvil API remains
subject to World Anvil's applicable terms, including its restrictions on
commercial API projects.

This project is independent and is not affiliated with or endorsed by World
Anvil.
