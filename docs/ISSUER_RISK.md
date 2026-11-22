# SAC Issuer-Risk Assumptions

Poker-table balances are backed by Stellar Asset Contract (SAC) token balances.
Issuers may freeze recipients or claw back issued assets. A table operator must
therefore treat transfer failure as an issuer-event risk, not an application
bankruptcy.

## Settlement behavior

- Settlement continues for every player whose SAC transfer succeeds.
- A failed transfer is retained as an internal claimable balance for the recipient.
- The aggregate untransferred amount is available from `settlement_deficit`.
- Players can retry a payout with `claim_settlement_balance`.
- A clawback or freeze must reduce the delivered asset balance; it does not
  reduce the table's recorded chip liability automatically.

## Operator obligations

- Monitor `settlement_deficit` and table-level settlement events.
- Suspend accepting deposits for a frozen or clawback-risk currency.
- Reconcile the SAC reserve, contract balance, and internal chip liabilities
  before accepting a replacement issuer or asset.
- Do not assume that a claimable balance is immediately redeemable if the issuer
  remains frozen or noncompliant.
