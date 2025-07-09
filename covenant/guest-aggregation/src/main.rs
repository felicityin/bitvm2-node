#![no_main]
zkm_zkvm::entrypoint!(main);

use revm_primitives::B256;
use sha2::{Digest, Sha256};

use crate::io::ZKMPublicValues;

mod io;

pub fn main() {
    let vkey0 = zkm_zkvm::io::read::<[u32; 8]>();
    let public_values0 = zkm_zkvm::io::read::<Vec<u8>>();
    let states0: (B256, B256) = {
        let mut public_value = ZKMPublicValues::from(&public_values0);
        // (prev_state_root, cur_state_root)
        (public_value.read::<B256>(), public_value.read::<B256>())
    };

    let vkey1 = zkm_zkvm::io::read::<[u32; 8]>();
    let public_values1 = zkm_zkvm::io::read::<Vec<u8>>();
    let states1: (B256, B256) = {
        let mut public_value = ZKMPublicValues::from(&public_values1);
        // (prev_state_root, cur_state_root)
        (public_value.read::<B256>(), public_value.read::<B256>())
    };

    assert_eq!(states0.1, states1.0);

    // Verify the proofs.
    zkm_zkvm::lib::verify::verify_zkm_proof(&vkey0, &Sha256::digest(&public_values0).into());
    zkm_zkvm::lib::verify::verify_zkm_proof(&vkey1, &Sha256::digest(&public_values1).into());

    zkm_zkvm::io::commit(&states0.0); // prev state root
    zkm_zkvm::io::commit(&states1.1); // cur state root
}
