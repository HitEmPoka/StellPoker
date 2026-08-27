fn main(proofs: [Field; 5], valid_flags: [bool; 5], pub_result: pub bool) {
    let mut all_valid = true;
    for i in 0..5 {
        if !valid_flags[i] {
            all_valid = false;
        }
    }
    assert(all_valid == pub_result);
}
