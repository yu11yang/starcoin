use diem_types::account_config::CORE_CODE_ADDRESS;
use move_stdlib::natives::{bcs, event, hash, signer, vector};
use move_vm_runtime::native_functions::{NativeFunction, NativeFunctionTable};
use starcoin_vm_types::identifier::Identifier;
use starcoin_vm_types::language_storage::CORE_CODE_ADDRESS;

pub fn diem_natives() -> NativeFunctionTable {
    const NATIVES: &[(&str, &str, NativeFunction)] = &[
        ("Hash", "sha2_256", hash::native_sha2_256),
        ("Hash", "sha3_256", hash::native_sha3_256),
        (
            "Hash",
            "keccak_256",
            starcoin_natives::hash::native_keccak_256,
        ),
        ("BCS", "to_bytes", bcs::native_to_bytes),
        (
            "BCS",
            "to_address",
            starcoin_natives::bcs::native_to_address,
        ),
        (
            "Signature",
            "ed25519_validate_pubkey",
            signature::native_ed25519_publickey_validation,
        ),
        (
            "Signature",
            "ed25519_verify",
            signature::native_ed25519_signature_verification,
        ),
        ("Vector", "length", vector::native_length),
        ("Vector", "empty", vector::native_empty),
        ("Vector", "borrow", vector::native_borrow),
        ("Vector", "borrow_mut", vector::native_borrow),
        ("Vector", "push_back", vector::native_push_back),
        ("Vector", "pop_back", vector::native_pop),
        ("Vector", "destroy_empty", vector::native_destroy_empty),
        ("Vector", "swap", vector::native_swap),
        (
            "Event",
            "write_to_event_store",
            event::native_write_to_event_store,
        ),
        ("Account", "create_signer", account::native_create_signer),
        ("Account", "destroy_signer", account::native_destroy_signer),
        ("Signer", "borrow_address", signer::native_borrow_address),
        (
            "Token",
            "name_of",
            starcoin_natives::token::native_token_name_of,
        ),
        #[cfg(feature = "testing")]
        ("Debug", "print", move_stdlib::natives::debug::native_print),
        #[cfg(feature = "testing")]
        (
            "Debug",
            "print_stack_trace",
            move_stdlib::natives::debug::native_print_stack_trace,
        ),
        #[cfg(feature = "testing")]
        (
            "UnitTest",
            "create_signers_for_testing",
            move_stdlib::natives::unit_test::native_create_signers_for_testing,
        ),
    ];
    NATIVES
        .iter()
        .cloned()
        .map(|(module_name, func_name, func)| {
            (
                CORE_CODE_ADDRESS,
                Identifier::new(module_name).unwrap(),
                Identifier::new(func_name).unwrap(),
                func,
            )
        })
        .collect()
}
