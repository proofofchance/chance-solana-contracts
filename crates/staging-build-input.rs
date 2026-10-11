// Host-only deterministic build input parser; no keypair or runtime override.
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const STAGING_INPUT_KEYS: [&str; 7] = [
    "schema",
    "environment",
    "genesis_hash",
    "registry_program",
    "daily_lottery_program",
    "giveaways_program",
    "initialization_authority",
];

fn decode_staging_key(value: &str) -> [u8; 32] {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    assert!((32..=44).contains(&value.len()), "staging identity length");
    let leading = value.bytes().take_while(|b| *b == b'1').count();
    let mut bytes = vec![0u8];
    for character in value.bytes() {
        let mut carry = ALPHABET
            .iter()
            .position(|b| *b == character)
            .expect("staging identities must be canonical base58") as u32;
        for byte in bytes.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = (carry & 255) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 255) as u8);
            carry >>= 8;
        }
    }
    while bytes.first() == Some(&0) {
        bytes.remove(0);
    }
    assert_eq!(
        leading + bytes.len(),
        32,
        "staging identities must decode to 32 bytes"
    );
    let mut result = [0u8; 32];
    result[leading..].copy_from_slice(&bytes);
    assert_ne!(result, [0; 32], "system/zero identity is forbidden");
    result
}

fn generate_staging_constants(program_key: &str, anchor: bool) {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_STAGING_AFRESH_V1");
    if env::var_os("CARGO_FEATURE_STAGING_AFRESH_V1").is_none() {
        return;
    }
    assert!(
        env::var_os("CARGO_FEATURE_DEVNET_V1").is_none(),
        "staging-afresh-v1 and retained devnet-v1 are mutually exclusive"
    );
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let input = root.join("releases/staging-afresh-v1.inputs");
    println!("cargo:rerun-if-changed={}", input.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/staging-build-input.rs").display()
    );
    let text =
        fs::read_to_string(input).expect("explicit public staging-afresh-v1.inputs is required");
    assert!(
        text.len() < 1024 && text.ends_with('\n') && !text.contains('\r'),
        "noncanonical staging inputs"
    );
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(
        lines.len(),
        STAGING_INPUT_KEYS.len(),
        "exact staging input schema required"
    );
    let mut values = BTreeMap::new();
    for (line, expected) in lines.iter().zip(STAGING_INPUT_KEYS) {
        let (key, value) = line
            .split_once('=')
            .expect("staging input key=value required");
        assert_eq!(
            key, expected,
            "staging input fields must be in canonical order"
        );
        assert!(
            !value.is_empty() && !value.contains('='),
            "invalid staging input value"
        );
        values.insert(key, value);
    }
    assert_eq!(values["schema"], "1");
    assert_eq!(
        values["environment"], "staging",
        "production is never an afresh target"
    );
    assert_eq!(
        values["genesis_hash"],
        "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG"
    );
    let mut keys = BTreeMap::new();
    for key in &STAGING_INPUT_KEYS[3..] {
        keys.insert(*key, decode_staging_key(values[key]));
    }
    let distinct: std::collections::BTreeSet<_> = keys.values().collect();
    assert_eq!(
        distinct.len(),
        keys.len(),
        "program and bootstrap identities must be distinct"
    );
    let program = keys[program_key];
    let authority = keys["initialization_authority"];
    let mut output = format!(
        "pub const STAGING_PROGRAM_ID: Pubkey = Pubkey::new_from_array({program:?});\n\
         pub const INITIALIZATION_AUTHORITY: Pubkey = Pubkey::new_from_array({authority:?});\n"
    );
    if anchor {
        output.push_str(&format!("declare_id!(\"{}\");\n", values[program_key]));
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("staging_afresh.rs"),
        output,
    )
    .unwrap();
}
