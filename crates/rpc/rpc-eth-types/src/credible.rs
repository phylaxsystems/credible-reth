//! Credible Layer RPC integration hooks.

use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_rpc_types_eth::{state::EvmOverrides, BlockOverrides};
use alloy_sol_types::SolValue;
use reth_transaction_pool::TransactionOrigin;
use serde::{Deserialize, Serialize};

/// Base storage slot of `CredibleRegistry`'s `_credibleBlocks` mapping, per its current
/// storage layout (`forge inspect CredibleRegistry storage-layout`).
const DEFAULT_CREDIBLE_BLOCKS_BASE_SLOT: u64 = 1;

/// Credible Layer behavior toggles for the `eth` RPC namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredibleRpcConfig {
    /// Address of the on-chain `CredibleRegistry` contract.
    ///
    /// When set, the credible block override is derived per-request from
    /// `_credibleBlocks[blockNumber]`.
    pub registry_address: Option<Address>,
    /// Retains transactions accepted by a raw transaction forwarder as private pool transactions.
    pub retain_forwarded_txs_as_private: bool,
}

impl CredibleRpcConfig {
    /// Applies the credible block override for a call simulated against `block_number` to
    /// call-like EVM overrides.
    ///
    /// A no-op if no registry is configured. Registry-backed resolution is a pure hash
    /// computation — no call into the registry contract, no EVM execution, no async lookup.
    pub fn apply_credible_block_override(
        &self,
        block_number: u64,
        overrides: EvmOverrides,
    ) -> EvmOverrides {
        let Some(registry) = self.registry_address else { return overrides };

        let block_override = CredibleBlockOverride {
            address: registry,
            slot: credible_block_slot(block_number, U256::from(DEFAULT_CREDIBLE_BLOCKS_BASE_SLOT)),
            value: B256::with_last_byte(1),
        };
        block_override.apply_to(overrides)
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

    /// Whether retained-private transactions must be hidden from public pool-reading RPCs.
    ///
    /// Enabled exactly when the node retains forwarded transactions as private.
    pub const fn hide_private_pool_txs(&self) -> bool {
        self.retain_forwarded_txs_as_private
    }
}

/// Computes the storage slot of `_credibleBlocks[block_number]`, matching Solidity's mapping
/// slot derivation: `keccak256(abi.encode(block_number, base_slot))`.
fn credible_block_slot(block_number: u64, base_slot: U256) -> B256 {
    keccak256((U256::from(block_number), base_slot).abi_encode())
}

/// Extracts the block number the EVM will use from `block_overrides.number`, if the caller set
/// one. This must take priority over resolving the request's block tag, since the override is
/// applied after tag resolution.
pub fn credible_block_number_override(block_overrides: Option<&BlockOverrides>) -> Option<u64> {
    block_overrides.and_then(|overrides| overrides.number).map(|number| number.saturating_to())
}

/// The storage override that makes `_credibleBlocks[blockNumber]` read as `true` for one
/// call-like RPC simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
struct CredibleBlockOverride {
    /// `CredibleRegistry` contract address.
    address: Address,
    /// `_credibleBlocks[blockNumber]` storage slot.
    slot: B256,
    /// Value to expose during simulation.
    value: B256,
}

impl CredibleBlockOverride {
    /// Merges this override into existing EVM overrides.
    fn apply_to(self, mut overrides: EvmOverrides) -> EvmOverrides {
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
    fn credible_block_override_adds_state_diff() {
        let address = Address::repeat_byte(0x11);
        let slot = B256::repeat_byte(0x22);
        let value = B256::repeat_byte(0x33);
        let block_override = CredibleBlockOverride { address, slot, value };

        let overrides = block_override.apply_to(EvmOverrides::default());
        let state = overrides.state.expect("override should add state overrides");
        let account = state.get(&address).expect("override account should be present");

        assert_eq!(account.state, None);
        assert_eq!(
            account.state_diff.as_ref().and_then(|state_diff| state_diff.get(&slot)),
            Some(&value)
        );
    }

    #[test]
    fn credible_block_override_merges_with_existing_full_state() {
        let address = Address::repeat_byte(0x11);
        let override_slot = B256::repeat_byte(0x22);
        let override_value = B256::repeat_byte(0x33);
        let existing_slot = B256::repeat_byte(0x44);
        let existing_value = B256::repeat_byte(0x55);

        let mut state_override = StateOverride::default();
        state_override.insert(
            address,
            AccountOverride::default().with_state([(existing_slot, existing_value)]),
        );

        let block_override =
            CredibleBlockOverride { address, slot: override_slot, value: override_value };
        let overrides = block_override.apply_to(EvmOverrides::state(Some(state_override)));
        let state = overrides.state.expect("override should keep state overrides");
        let account = state.get(&address).expect("override account should be present");
        let state = account.state.as_ref().expect("full state override should be preserved");

        assert_eq!(account.state_diff, None);
        assert_eq!(state.get(&existing_slot), Some(&existing_value));
        assert_eq!(state.get(&override_slot), Some(&override_value));
    }

    #[test]
    fn registry_slot_matches_solidity_mapping_derivation() {
        // keccak256(abi.encode(uint256(12345), uint256(1))), cross-checked with
        // `cast index uint256 12345 1`.
        let expected: B256 =
            "0x24689f9b6ba9bad3c49d2b1293bf33fa38d0c418c093b2b4bc23f5d18e11355e".parse().unwrap();
        assert_eq!(credible_block_slot(12345, U256::from(1)), expected);
    }

    #[test]
    fn no_override_without_registry() {
        let config = CredibleRpcConfig::default();
        let overrides = config.apply_credible_block_override(100, EvmOverrides::default());
        assert_eq!(overrides.state, None);
    }

    #[test]
    fn applies_registry_derived_override() {
        let registry = Address::repeat_byte(0xaa);
        let config = CredibleRpcConfig { registry_address: Some(registry), ..Default::default() };

        let overrides = config.apply_credible_block_override(12345, EvmOverrides::default());
        let state = overrides.state.expect("registry override should add state overrides");
        let account = state.get(&registry).expect("registry account should be present");
        let expected_slot: B256 =
            "0x24689f9b6ba9bad3c49d2b1293bf33fa38d0c418c093b2b4bc23f5d18e11355e".parse().unwrap();
        assert_eq!(
            account.state_diff.as_ref().and_then(|diff| diff.get(&expected_slot)),
            Some(&B256::with_last_byte(1))
        );
    }

    #[test]
    fn block_overrides_number_takes_priority() {
        let overrides = BlockOverrides { number: Some(U256::from(42)), ..Default::default() };
        assert_eq!(credible_block_number_override(Some(&overrides)), Some(42));
    }

    #[test]
    fn no_override_number_without_block_overrides() {
        assert_eq!(credible_block_number_override(None), None);

        let overrides = BlockOverrides::default();
        assert_eq!(credible_block_number_override(Some(&overrides)), None);
    }

    #[test]
    fn hide_private_pool_txs_tracks_retain_flag() {
        assert!(!CredibleRpcConfig::default().hide_private_pool_txs());
        let config =
            CredibleRpcConfig { retain_forwarded_txs_as_private: true, ..Default::default() };
        assert!(config.hide_private_pool_txs());
    }
}
