/**
 * @name Potential storage key collision in Soroban contract
 * @description In Soroban, every storage entry is addressed by a key that is
 *              XDR-serialized at runtime.  Two different logical values that
 *              serialize to the same byte sequence will silently overwrite each
 *              other.  Common collision risks:
 *
 *              1. Using raw string literals (Symbol::new / symbol_short) as
 *                 storage keys instead of a typed `#[contracttype]` enum —
 *                 a typo or copy-paste produces a silent collision.
 *              2. Wrapping different data in a single-variant enum where the
 *                 variant tag is not included in the key.
 *              3. Reusing the same `Symbol` literal for logically distinct
 *                 entries (e.g., using "admin" for both per-table and global
 *                 admin).
 *
 *              This query flags `env.storage().*().set(...)` and
 *              `env.storage().*().get(...)` calls where the first key argument
 *              is a plain `Symbol::new(...)` or `symbol_short!(...)` literal
 *              rather than a typed `DataKey` / `StorageKey` enum variant.
 *              Results should be reviewed to confirm the key uniquely
 *              identifies the intended storage slot.
 *
 * @kind        problem
 * @problem.severity warning
 * @precision   medium
 * @id          stellpoker/rust/storage-key-collision
 * @tags        correctness
 *              security
 *              storage
 *              soroban
 *              stellar
 */

import rust

/**
 * A call to `env.storage().instance().set(...)`,
 * `.persistent().set(...)`, or `.temporary().set(...)`.
 */
class StorageSetCall extends MethodCallExpr {
  StorageSetCall() {
    this.getIdentifier().getText() = "set" and
    // Receiver chain ends with `.instance()`, `.persistent()`, or `.temporary()`
    exists(MethodCallExpr storageType |
      storageType.getIdentifier().getText() = ["instance", "persistent", "temporary"] and
      storageType = this.getReceiver().(MethodCallExpr)
    )
  }
}

/**
 * A call to the corresponding `.get(...)` variants.
 */
class StorageGetCall extends MethodCallExpr {
  StorageGetCall() {
    this.getIdentifier().getText() = ["get", "has", "remove"] and
    exists(MethodCallExpr storageType |
      storageType.getIdentifier().getText() = ["instance", "persistent", "temporary"] and
      storageType = this.getReceiver().(MethodCallExpr)
    )
  }
}

/**
 * A key expression that is a plain `Symbol::new(...)` call — not wrapped in a
 * typed enum variant.  Using raw Symbol keys is fragile: there is no compiler
 * enforcement that two callers use the same string, and a typo creates a
 * distinct storage slot that is silently never read.
 */
class RawSymbolKey extends CallExpr {
  RawSymbolKey() {
    // Matches `Symbol::new(&env, "some_key")` path-call expressions
    this.getFunction().(PathExpr).toString().matches("Symbol::new")
  }
}

from StorageSetCall setCall, RawSymbolKey rawKey
where
  // The first argument to .set() is the key
  rawKey = setCall.getArgList().getArg(0)
select setCall,
  "Storage .set() uses a raw Symbol key " + rawKey.toString() +
    " instead of a typed DataKey/StorageKey enum variant. " +
    "This is prone to collision if the same string is used elsewhere. " +
    "Wrap the key in a #[contracttype] enum variant."
