/**
 * @name Unchecked integer arithmetic in Soroban contract
 * @description Plain `+`, `-`, `*` operators on integer types in a Soroban
 *              contract will panic in debug builds and **silently wrap or
 *              overflow** in release builds (where `overflow-checks = true`
 *              causes a trap).  Financial logic — pot accumulation, rake
 *              calculation, stack updates, side-pot math — must use the
 *              explicit `checked_*`, `saturating_*`, or `wrapping_*` families
 *              or validate bounds beforehand.
 *
 *              This query flags bare arithmetic binary expressions (`+`, `-`,
 *              `*`) on numeric types inside contract implementations.
 *              Division is excluded because divide-by-zero already produces an
 *              obvious panic; the risk here is *silent* value corruption.
 *
 * @kind        problem
 * @problem.severity warning
 * @precision   medium
 * @id          stellpoker/rust/arithmetic-overflow
 * @tags        correctness
 *              security
 *              financial
 *              soroban
 *              stellar
 */

import rust

/**
 * Integer arithmetic operators that can overflow on `i128 / u32` operands
 * commonly found in the poker-table and committee-registry contracts.
 */
class ArithOp extends string {
  ArithOp() { this = ["+", "-", "*"] }
}

from BinaryExpr expr, ArithOp op
where
  op = expr.getOperatorName() and
  // Restrict to contract source — exclude generated test code
  not exists(Module m |
    m.getAnAttr().toString().matches("%cfg%test%") and
    m = expr.getEnclosingItem*()
  )
select expr,
  "Unchecked '" + op +
    "' arithmetic may overflow or wrap. Use checked_" +
    (if op = "+" then "add" else if op = "-" then "sub" else "mul") +
    "(), saturating_*, or explicit bounds validation for financial values."
