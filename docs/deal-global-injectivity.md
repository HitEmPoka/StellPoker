# Deal global-injectivity constraints

The deal circuit now checks uniqueness at two independent layers:

1. `cards::assert_valid_deck` constrains all 52 private deck values to a valid
   permutation.
2. `assert_global_deal_injectivity` constrains the public dealt indices for
   active seats, independent of how those indices were generated.

For each active seat the gadget adds one within-seat inequality. For every
pair of active seats it adds four inequalities covering both hole-card
positions. With `n` players, that is `n + 4 * n * (n - 1) / 2` comparisons
(66 comparisons at six seats). Inactive padded seats are excluded.

The negative circuit test deliberately assigns seat 2 an index already held
by seat 0 and must fail with `card dealt to two seats`. This protects the
invariant if deterministic `2*p` assignment is replaced later.
