# MPC Table Affinity

Table affinity is an operator opt-in for installations with more than three
healthy MPC nodes. Set `MPC_TABLE_AFFINITY_ENABLED=true` on the coordinator to
deterministically prefer the same three-node subset for a given public table
ID. The default is `false`.

The policy only changes committee ordering and selection among already-healthy
nodes. It always retains the full three-node REP3 committee requirement, uses
no player, card, deck, or wallet data, and does not share state between tables.
The table ID is already public and is the only affinity input. Consequently,
the policy does not weaken the privacy assumption that the coordinator never
sees private witnesses and that each MPC node receives only its own share.

Operators should disable the flag during committee churn or when they need
the broadest distribution of load. Affinity is not a security control and
must not be used to bypass node health, version negotiation, or committee
identity checks.