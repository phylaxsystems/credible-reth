//! Generic extension hooks for RPC state overrides.

use alloy_rpc_types_eth::state::StateOverride;
use std::sync::Arc;

/// RPC execution paths which can install an additional state override.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateOverrideMethod {
    /// `eth_call`, `eth_callMany`, and `eth_createAccessList`.
    Call,
    /// `eth_estimateGas`.
    EstimateGas,
    /// `eth_simulateV1`.
    SimulateV1,
}

/// Context passed to a [`StateOverrideHook`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateOverrideContext {
    /// The RPC execution path being prepared.
    pub method: StateOverrideMethod,
    /// The block number in the final EVM environment, after block overrides are applied.
    pub block_number: u64,
}

/// Allows an RPC consumer to add state overrides after Reth resolves the execution environment.
pub trait StateOverrideHook: Send + Sync {
    /// Mutates the state overrides that will be applied to the EVM database.
    fn apply(&self, context: StateOverrideContext, overrides: &mut StateOverride);
}

impl<F> StateOverrideHook for F
where
    F: Fn(StateOverrideContext, &mut StateOverride) + Send + Sync,
{
    fn apply(&self, context: StateOverrideContext, overrides: &mut StateOverride) {
        self(context, overrides);
    }
}

/// Shared hook handle stored by the RPC API.
pub type StateOverrideHookRef = Arc<dyn StateOverrideHook>;
