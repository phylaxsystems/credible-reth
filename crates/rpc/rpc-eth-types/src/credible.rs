//! Credible Layer RPC integration hooks.

use alloy_primitives::{Address, B256};
use alloy_rpc_types_eth::state::EvmOverrides;
use reth_transaction_pool::TransactionOrigin;
use serde::{Deserialize, Serialize};

/// Credible Layer behavior toggles for the `eth` RPC namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredibleRpcConfig {
    /// Storage value that should be visible to marker-aware `eth_call` / `eth_estimateGas`.
    ///
    /// Marker-aware simulation is enabled.
    pub marker_override: Option<CredibleMarkerOverride>,
    /// Retains transactions accepted by a raw transaction forwarder as private pool transactions.
    pub retain_forwarded_txs_as_private: bool,
}

impl CredibleRpcConfig {
    /// Applies configured marker state to call-like EVM overrides.
    pub fn apply_marker_override(&self, overrides: EvmOverrides) -> EvmOverrides {
        match self.marker_override {
            Some(marker_override) => marker_override.apply_to(overrides),
            None => overrides,
        }
    }

    /// Returns the origin a successfully forwarded transaction should be retained under.
    ///
    /// Returns `Private` instead of the given origin when so configured, so the transaction
    /// never propagates to the network before the forwarder's remote node includes it on-chain.
    pub const fn resolve_forwarded_origin(&self, origin: TransactionOrigin) -> TransactionOrigin {
        if self.retain_forwarded_txs_as_private {
            TransactionOrigin::Private
        } else {
            origin
        }
    }
}

/// A single storage-slot marker override used for call-like RPC simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CredibleMarkerOverride {
    /// Contract/account that owns the marker storage.
    pub address: Address,
    /// Marker storage slot.
    pub slot: B256,
    /// Marker value to expose during simulation.
    pub value: B256,
}

impl CredibleMarkerOverride {
    /// Merges this marker storage value into existing EVM overrides.
    pub fn apply_to(self, mut overrides: EvmOverrides) -> EvmOverrides {
        let state = overrides.state.get_or_insert_with(Default::default);
        let account = state.entry(self.address).or_default();

        if let Some(state) = account.state.as_mut() {
            state.insert(self.slot, self.value);
        } else {
            account.state_diff.get_or_insert_with(Default::default).insert(self.slot, self.value);
        }

        overrides
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_rpc_types_eth::state::{AccountOverride, StateOverride};

    #[test]
    fn marker_override_adds_state_diff() {
        let address = Address::repeat_byte(0x11);
        let slot = B256::repeat_byte(0x22);
        let value = B256::repeat_byte(0x33);
        let marker = CredibleMarkerOverride { address, slot, value };

        let overrides = marker.apply_to(EvmOverrides::default());
        let state = overrides.state.expect("marker override should add state overrides");
        let account = state.get(&address).expect("marker account should be present");

        assert_eq!(account.state, None);
        assert_eq!(
            account.state_diff.as_ref().and_then(|state_diff| state_diff.get(&slot)),
            Some(&value)
        );
    }

    #[test]
    fn marker_override_merges_with_existing_full_state() {
        let address = Address::repeat_byte(0x11);
        let marker_slot = B256::repeat_byte(0x22);
        let marker_value = B256::repeat_byte(0x33);
        let existing_slot = B256::repeat_byte(0x44);
        let existing_value = B256::repeat_byte(0x55);

        let mut state_override = StateOverride::default();
        state_override.insert(
            address,
            AccountOverride::default().with_state([(existing_slot, existing_value)]),
        );

        let marker = CredibleMarkerOverride { address, slot: marker_slot, value: marker_value };
        let overrides = marker.apply_to(EvmOverrides::state(Some(state_override)));
        let state = overrides.state.expect("marker override should keep state overrides");
        let account = state.get(&address).expect("marker account should be present");
        let state = account.state.as_ref().expect("full state override should be preserved");

        assert_eq!(account.state_diff, None);
        assert_eq!(state.get(&existing_slot), Some(&existing_value));
        assert_eq!(state.get(&marker_slot), Some(&marker_value));
    }
}
