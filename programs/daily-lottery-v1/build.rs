include!("../../crates/staging-build-input.rs");
include!("../../crates/non-production-controls-build.rs");

fn main() {
    generate_test_control_constants();
    generate_staging_constants("daily_lottery_program", false);
}
