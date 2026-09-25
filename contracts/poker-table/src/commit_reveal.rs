use crate::types::*;
use soroban_sdk::{Bytes, BytesN, Env};

pub fn compute_action_hash(
    env: &Env,
    action: &Action,
    amount: i128,
    nonce: &Bytes,
) -> BytesN<32> {
    let mut preimage = Bytes::new(env);

    match action {
        Action::Fold => {
            preimage.push_back(0u8);
        }
        Action::Check => {
            preimage.push_back(1u8);
        }
        Action::Call => {
            preimage.push_back(2u8);
        }
        Action::Bet(_) => {
            preimage.push_back(3u8);
            let amount_bytes = amount.to_be_bytes();
            for byte in amount_bytes {
                preimage.push_back(byte);
            }
        }
        Action::Raise(_) => {
            preimage.push_back(4u8);
            let amount_bytes = amount.to_be_bytes();
            for byte in amount_bytes {
                preimage.push_back(byte);
            }
        }
        Action::AllIn => {
            preimage.push_back(5u8);
        }
    }

    for i in 0..nonce.len() {
        preimage.push_back(nonce.get(i).unwrap_or(0u8));
    }

    env.crypto().keccak256(&preimage).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_hash_fold() {
        let env = soroban_sdk::Env::default();
        let nonce = Bytes::from_slice(&env, &[1u8; 32]);
        let hash = compute_action_hash(&env, &Action::Fold, 0, &nonce);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_action_hash_consistency() {
        let env = soroban_sdk::Env::default();
        let nonce = Bytes::from_slice(&env, &[2u8; 32]);
        let hash1 = compute_action_hash(&env, &Action::Check, 0, &nonce);
        let hash2 = compute_action_hash(&env, &Action::Check, 0, &nonce);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_action_hash_different_actions() {
        let env = soroban_sdk::Env::default();
        let nonce = Bytes::from_slice(&env, &[3u8; 32]);
        let hash_fold = compute_action_hash(&env, &Action::Fold, 0, &nonce);
        let hash_check = compute_action_hash(&env, &Action::Check, 0, &nonce);
        assert_ne!(hash_fold, hash_check);
    }
}
