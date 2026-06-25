/**
 * @name Panic-inducing unwrap / expect in Soroban contract
 * @description Calling `.unwrap()` or `.expect()` on a `Result` or `Option`
 *              inside a Soroban smart-contract function will cause a host-trap
 *              (contract panic) if the value is `Err` / `None`.  This can be
 *              exploited by an adversary to permanently freeze table state,
 *              deny service to players, or prevent fund withdrawals.
 *
 *              Prefer returning a typed `ContractError` variant via `?` or an
 *              explicit `match` / `.ok_or(...)` pattern.
 *
 * @kind        problem
 * @problem.severity warning
 * @precision   high
 * @id          stellpoker/rust/panic-unwrap
 * @tags        correctness
 *              reliability
 *              security
 *              soroban
 *              stellar
 */

import rust

/**
 * Matches method-call expressions whose method name is `unwrap` or `expect`.
 * We intentionally exclude test modules because `#[cfg(test)]` code is never
 * deployed on-chain and test panics are acceptable.
 */
from MethodCallExpr call, string methodName
where
  methodName = call.getIdentifier().getText() and
  (methodName = "unwrap" or methodName = "expect") and
  // Exclude test modules — look for the #[cfg(test)] attribute on an ancestor
  // module. CodeQL's Rust library exposes Module items as ancestors.
  not exists(Module m |
    m.getAnAttr().toString().matches("%cfg%test%") and
    m = call.getEnclosingItem*()
  )
select call,
  "Call to ." + methodName +
    "() can panic and trap the contract. Use `?` propagation or an explicit " +
    "error variant instead."
