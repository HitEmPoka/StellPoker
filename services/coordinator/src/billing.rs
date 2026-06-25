// src/billing.rs
//! Billing engine for session cost deductions

use std::sync::Arc;
use sqlx::{PgPool, Error};

/// Simple billing engine that deducts a fixed charge per transaction
pub struct BillingEngine {
    /// Optional DB pool for persisting balance changes
    pub db_pool: Option<Arc<PgPool>>,
    /// Fixed charge amount in smallest currency unit (e.g., lamports)
    pub fixed_charge: i64,
}

impl BillingEngine {
    /// Create a new billing engine. `fixed_charge` is the amount to charge per transaction.
    pub fn new(db_pool: Option<Arc<PgPool>>, fixed_charge: i64) -> Self {
        Self { db_pool, fixed_charge }
    }

    /// Apply billing for an operator. `operator_id` identifies the operator (e.g., DB row id).
    /// `tx_cost` is the number of Soroban transactions to bill for.
    /// Returns Ok(()) on success or a sqlx::Error.
    pub async fn apply_billing(
        &self,
        operator_id: i64,
        tx_cost: i64,
    ) -> Result<(), Error> {
        // Compute total charge
        let total = self.fixed_charge.saturating_mul(tx_cost);
        // If we have a DB pool, update the operator's balance; otherwise just log.
        if let Some(pool) = &self.db_pool {
            // Assume a table `operators` with columns `id` and `balance` exists.
            sqlx::query!(
                "UPDATE operators SET balance = balance - $1 WHERE id = $2",
                total,
                operator_id
            )
            .execute(pool.as_ref())
            .await
            .map(|_| ())
        } else {
            tracing::info!(
                "BillingEngine: would deduct {} from operator {} (no DB configured)",
                total,
                operator_id
            );
            Ok(())
        }
    }
}
