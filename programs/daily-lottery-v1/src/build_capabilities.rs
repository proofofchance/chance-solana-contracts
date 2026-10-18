//! Read-only, fixed-width facts from the executing program, never caller metadata.
use solana_program::pubkey::Pubkey;

pub fn encode(program_id: &Pubkey) -> [u8; 110] {
    let mut bytes = [0; 110];
    bytes[..8].copy_from_slice(b"CHCTL001");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes()); // capability schema
    bytes[10..12].copy_from_slice(&1u16.to_le_bytes()); // business contract V1
    bytes[14..46].copy_from_slice(program_id.as_ref());
    #[cfg(any(feature = "local-test-controls", feature = "staging-test-controls"))]
    {
        bytes[14..46].copy_from_slice(crate::TEST_CONTROL_PROGRAM_ID.as_ref());
        bytes[12] = if cfg!(feature = "local-test-controls") {
            1
        } else {
            2
        };
        bytes[46..78].copy_from_slice(crate::TEST_CONTROL_REGISTRY_ID.as_ref());
        bytes[78..110].copy_from_slice(crate::TEST_CONTROL_AUTHORITY.as_ref());
    }
    bytes[13] = u8::from(cfg!(feature = "allow-early-upload"))
        | (u8::from(cfg!(feature = "allow-service-charge-update")) << 1);
    bytes
}
