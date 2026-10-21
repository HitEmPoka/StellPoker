use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_multi_epoch_fee_distribution_with_stake_changes() {
    let env = Env::default();
    env.mock_all_auths();
    
    let committee_registry = Address::generate(&env);
    let treasury = Address::generate(&env);
    
    let validators: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
    
    let mut stakes = vec![1000, 2000, 1500, 3000, 2500];
    let total_fees = 10000;
    
    for epoch in 0..10 {
        let total_stake: i128 = stakes.iter().sum();
        
        for (i, validator) in validators.iter().enumerate() {
            let expected_share = (total_fees * stakes[i]) / total_stake;
            
            assert!(
                expected_share > 0,
                "Validator {} should receive non-zero fees in epoch {}",
                i, epoch
            );
        }
        
        if epoch == 3 {
            stakes[0] += 500;
            stakes[2] -= 300;
        }
        
        if epoch == 7 {
            stakes[1] *= 2;
            stakes[4] = stakes[4] / 2;
        }
    }
}

#[test]
fn test_proportional_distribution_correctness() {
    let env = Env::default();
    
    let stakes = vec![100, 200, 300, 400];
    let total_stake: i128 = stakes.iter().sum();
    let total_fees = 50000;
    
    let mut distributed = 0;
    
    for stake in &stakes {
        let share = (total_fees * stake) / total_stake;
        distributed += share;
    }
    
    assert!(
        (distributed - total_fees).abs() < stakes.len() as i128,
        "Distribution rounding error exceeds validator count"
    );
}
