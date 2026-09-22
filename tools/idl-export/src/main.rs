use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    // anchor-lang-idl 0.1.3 formats an inherited toolchain override literally.
    // The repository rust-toolchain.toml remains authoritative for child cargo.
    std::env::remove_var("RUSTUP_TOOLCHAIN");
    let idl = anchor_lang_idl::build::IdlBuilder::new()
        .program_path(root.join("programs/giveaways-v1"))
        .cargo_args(vec!["--locked".into(), "--lib".into()])
        .build()?;
    let output = root.join("audit/schemas/giveaways-v1.idl.json");
    std::fs::write(output, serde_json::to_string_pretty(&idl)? + "\n")?;
    Ok(())
}
