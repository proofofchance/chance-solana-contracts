// Check the actual rustc cfg, as well as the build script's Cargo feature input.
// This also rejects incompatible cfg flags injected outside Cargo's feature API.
#[cfg(all(
    feature = "production-release",
    any(
        feature = "local-test-controls",
        feature = "staging-test-controls",
        feature = "allow-early-upload",
        feature = "staging-afresh-v1",
        feature = "allow-service-charge-update"
    )
))]
compile_error!("production release forbids testing features");
#[cfg(all(feature = "local-test-controls", feature = "staging-test-controls"))]
compile_error!("testing scopes are mutually exclusive");
#[cfg(all(
    feature = "allow-early-upload",
    not(any(feature = "local-test-controls", feature = "staging-test-controls"))
))]
compile_error!("early upload requires an explicit testing scope");
#[cfg(all(feature = "local-test-controls", feature = "staging-afresh-v1"))]
compile_error!("local controls cannot use staging identities");
#[cfg(all(feature = "staging-test-controls", not(feature = "staging-afresh-v1")))]
compile_error!("staging controls require staging-afresh-v1 identities");
