// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repo_root = manifest_dir.parent().expect("crate lives in workspace root");
    let proto_dir = repo_root.join("lore-proto").join("proto");

    println!("cargo:rerun-if-changed={}", proto_dir.join("auth_api.proto").display());
    println!("cargo:rerun-if-changed={}", proto_dir.join("rebac_api.proto").display());

    tonic_prost_build::configure()
        .build_client(false)
        .build_server(true)
        .compile_protos(
            &[proto_dir.join("auth_api.proto"), proto_dir.join("rebac_api.proto")],
            &[proto_dir],
        )?;

    Ok(())
}
