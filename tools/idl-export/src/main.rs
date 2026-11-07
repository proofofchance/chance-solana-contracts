fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A shared Cargo target can reuse this executable across checkouts. Resolve
    // the source at invocation time so an export never reads the other tree.
    let root = std::env::current_dir()?.canonicalize()?;
    if !root.join("programs/giveaways-v1/Cargo.toml").is_file() {
        return Err("run the IDL exporter from the contract repository root".into());
    }
    // anchor-lang-idl 0.1.3 formats an inherited toolchain override literally.
    // The repository rust-toolchain.toml remains authoritative for child cargo.
    std::env::remove_var("RUSTUP_TOOLCHAIN");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let devnet = args == ["--devnet-v1"];
    let afresh = args == ["--staging-afresh-v1"];
    let brand_review = args == ["--brand-review-v1"];
    if !args.is_empty() && !devnet && !afresh && !brand_review {
        return Err(
            "expected no arguments, --devnet-v1, --staging-afresh-v1 or --brand-review-v1".into(),
        );
    }
    let mut cargo_args = vec!["--locked".into(), "--lib".into()];
    if devnet {
        cargo_args.extend(["--features".into(), "devnet-v1".into()]);
    }
    if afresh {
        cargo_args.extend(["--features".into(), "staging-afresh-v1".into()]);
    }
    if brand_review {
        cargo_args.extend(["--features".into(), "brand-review".into()]);
    }
    let idl = anchor_lang_idl::build::IdlBuilder::new()
        .program_path(root.join("programs/giveaways-v1"))
        .cargo_args(cargo_args)
        .build()?;
    let output = root.join(if brand_review {
        "audit/schemas/giveaways-brand-review-v1.idl.json"
    } else if afresh {
        "target/deploy/giveaways-staging-afresh-v1.idl.json"
    } else if devnet {
        "audit/schemas/giveaways-devnet-v1.idl.json"
    } else {
        "audit/schemas/giveaways-v1.idl.json"
    });
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_string_pretty(&idl)? + "\n")?;
    Ok(())
}
