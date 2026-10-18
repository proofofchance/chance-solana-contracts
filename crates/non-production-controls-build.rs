// Host-only build policy. Neither an operator environment variable nor an
// authority transaction can enable a handler in a production artifact.
fn generate_test_control_constants() {
    let enabled = |name: &str| {
        println!("cargo:rerun-if-env-changed=CARGO_FEATURE_{name}");
        std::env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
    };
    let local = enabled("LOCAL_TEST_CONTROLS");
    let staging = enabled("STAGING_TEST_CONTROLS");
    let early = enabled("ALLOW_EARLY_UPLOAD");
    let production = enabled("PRODUCTION_RELEASE");
    let afresh = enabled("STAGING_AFRESH_V1");
    let charge = enabled("ALLOW_SERVICE_CHARGE_UPDATE");
    assert!(!(local && staging), "testing scopes are mutually exclusive");
    assert!(
        !early || local || staging,
        "early upload requires an explicit testing scope"
    );
    assert!(
        !local || !afresh,
        "local controls cannot use staging identities"
    );
    assert!(
        !staging || afresh,
        "staging controls require staging-afresh-v1 identities"
    );
    assert!(
        !production || !(local || staging || early || afresh || charge),
        "production release forbids testing features"
    );
    if !local && !staging {
        return;
    }
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let input = root.join(if local {
        "releases/local-test-controls-v1.inputs"
    } else {
        "releases/staging-afresh-v1.inputs"
    });
    println!("cargo:rerun-if-changed={}", input.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/non-production-controls-build.rs")
            .display()
    );
    let text =
        std::fs::read_to_string(input).expect("explicit testing identity inputs are required");
    assert!(
        text.len() < 1024 && text.ends_with('\n') && !text.contains('\r'),
        "noncanonical testing inputs"
    );
    let names: &[&str] = if local {
        &[
            "schema",
            "environment",
            "daily_lottery_program",
            "registry_program",
            "initialization_authority",
        ]
    } else {
        &STAGING_INPUT_KEYS
    };
    let rows: Vec<_> = text.lines().collect();
    assert_eq!(
        rows.len(),
        names.len(),
        "exact testing identity schema required"
    );
    let mut values = std::collections::BTreeMap::new();
    for (row, name) in rows.iter().zip(names) {
        let (key, value) = row
            .split_once('=')
            .expect("testing input key=value required");
        assert_eq!(
            key, *name,
            "testing input keys must be canonical and ordered"
        );
        assert!(
            !value.is_empty() && !value.contains('='),
            "invalid testing input"
        );
        values.insert(key, value);
    }
    assert_eq!(values["schema"], "1");
    assert_eq!(
        values["environment"],
        if local { "local" } else { "staging" }
    );
    let keys: Vec<_> = [
        "daily_lottery_program",
        "registry_program",
        "initialization_authority",
    ]
    .map(|key| decode_staging_key(values[key]))
    .into();
    assert!(
        keys[0] != keys[1] && keys[0] != keys[2] && keys[1] != keys[2],
        "testing identities must be distinct"
    );
    let output: String = [
        "TEST_CONTROL_PROGRAM_ID",
        "TEST_CONTROL_REGISTRY_ID",
        "TEST_CONTROL_AUTHORITY",
    ]
    .iter()
    .zip(keys)
    .map(|(name, key)| format!("pub const {name}: Pubkey = Pubkey::new_from_array({key:?});\n"))
    .collect();
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("test_controls.rs"),
        output,
    )
    .unwrap();
}
