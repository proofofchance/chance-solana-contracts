#![allow(unexpected_cfgs)]

// Existing release builds retain their layout. Brand review is an explicit V1 schema.
#[cfg(not(feature = "brand-review"))]
include!("legacy.rs");
#[cfg(feature = "brand-review")]
include!("brand_review.rs");
