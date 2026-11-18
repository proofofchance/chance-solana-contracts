#![allow(unexpected_cfgs)]

/// Borsh body encoding shared by account hash and audit records. This excludes
/// Anchor discriminators, preserving the existing commitment/account wire format.
trait BorshBytes: anchor_lang::AnchorSerialize {
    fn borsh_bytes(&self) -> std::io::Result<Vec<u8>> {
        borsh::to_vec(self)
    }
}
impl<T: anchor_lang::AnchorSerialize + ?Sized> BorshBytes for T {}

// Existing release builds retain their layout. Brand review is an explicit V1 schema.
#[cfg(not(feature = "brand-review"))]
include!("legacy.rs");
#[cfg(feature = "brand-review")]
include!("brand_review.rs");
