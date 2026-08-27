fn main(bets: [Field; 6], side_pots: [Field; 6], rake: Field, pub_total_pot: pub Field) {
    let mut total = 0;
    for i in 0..6 {
        total += bets[i];
    }
    assert(total - rake == pub_total_pot);
}
