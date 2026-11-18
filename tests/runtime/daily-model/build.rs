// Reuse the exact production source and its feature/identity build policy.
include!("../../../crates/staging-build-input.rs");
include!("../../../crates/non-production-controls-build.rs");

fn main() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../programs/daily-lottery-v1");
    std::env::set_var("CARGO_MANIFEST_DIR", source);
    generate_test_control_constants();
    generate_staging_constants("daily_lottery_program", false);
}
