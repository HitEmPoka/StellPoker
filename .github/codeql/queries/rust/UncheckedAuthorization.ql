/**
 * @name Missing require_auth() before privileged operation
 * @description Every Soroban function that mutates privileged state — admin
 *              operations, token transfers, committee actions, proof submission
 *              — must call `<address>.require_auth()` (or `require_auth_for_args`)
 *              before any state-changing work.  If the call is absent, any
 *              account can impersonate the caller and, for example, pause a
 *              table, drain the rake balance, or submit fraudulent showdown
 *              proofs.
 *
 *              This query identifies public `#[contractimpl]` functions that
 *              receive an `Address` parameter but do not call `.require_auth()`
 *              on it anywhere in their body.  Results should be reviewed
 *              manually — some view functions intentionally skip auth.
 *
 * @kind        problem
 * @problem.severity error
 * @precision   medium
 * @id          stellpoker/rust/unchecked-authorization
 * @tags        security
 *              authorization
 *              access-control
 *              soroban
 *              stellar
 */

import rust

/**
 * A function that is part of a `#[contractimpl]` block.
 * We approximate this by looking for functions whose parent `impl` block
 * has the `contractimpl` attribute (proc-macro attribute).
 */
class ContractImplFunction extends Function {
  ContractImplFunction() {
    exists(Impl impl |
      impl.getAnAttr().toString().matches("%contractimpl%") and
      impl.getAFunction() = this
    )
  }
}

/**
 * A parameter whose type is or wraps `Address`.
 * We match parameter names conventionally used in Soroban contracts.
 */
class AddressParam extends Param {
  AddressParam() {
    // Pattern-match on type reference text containing "Address"
    this.getPatText().matches("%") and
    this.getTypeRef().toString().matches("%Address%")
  }
}

/**
 * A call expression that invokes `.require_auth()` or
 * `.require_auth_for_args()` on a receiver.
 */
class RequireAuthCall extends MethodCallExpr {
  RequireAuthCall() {
    this.getIdentifier().getText() = ["require_auth", "require_auth_for_args"]
  }
}

from ContractImplFunction f, AddressParam p
where
  p = f.getAParam() and
  // The function body never calls require_auth on any receiver
  not exists(RequireAuthCall call | call.getEnclosingFunction() = f) and
  // Exclude pure view/read functions by name convention
  not f.getName().getText() = ["get_table", "get_admin", "get_hub",
    "get_rake_balance", "is_paused", "get_member", "get_current_epoch",
    "get_timeout_config", "get_game_liveness", "is_proof_verified",
    "get_table"]
select f,
  "Function '" + f.getName().getText() +
    "' accepts an Address parameter '" + p.getPatText() +
    "' but does not call .require_auth(). Any caller can impersonate this address."
