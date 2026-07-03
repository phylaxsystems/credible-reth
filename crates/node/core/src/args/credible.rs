use alloy_primitives::{Address, B256};
use clap::Args;
use reth_rpc_eth_types::{CredibleMarkerOverride, CredibleRpcConfig};

/// Parameters to configure Credible Layer RPC..
#[derive(Debug, Clone, Copy, Default, Args, PartialEq, Eq)]
#[command(next_help_heading = "Credible Layer")]
pub struct CredibleArgs {
    /// Contract address of the credible marker storage slot
    ///
    /// Must be set together with `--rpc.credible-marker-slot` and
    /// `--rpc.credible-marker-value` to enable marker-aware simulation.
    #[arg(long = "rpc.credible-marker-address", value_name = "ADDRESS")]
    pub marker_address: Option<Address>,

    /// Storage slot of the credible marker
    #[arg(long = "rpc.credible-marker-slot", value_name = "SLOT")]
    pub marker_slot: Option<B256>,

    /// Storage value of the credible marker
    #[arg(long = "rpc.credible-marker-value", value_name = "VALUE")]
    pub marker_value: Option<B256>,

    /// Retains transactions accepted by `--rpc.forwarder` as private pool transactions
    #[arg(long = "rpc.credible-retain-forwarded-private", default_value_t = false)]
    pub retain_forwarded_private: bool,
}

impl CredibleArgs {
    /// Returns a [`CredibleRpcConfig`] from the arguments.
    ///
    /// Marker-aware simulation is enabled only if the marker address, slot and value are all
    /// configured.
    pub const fn credible_config(&self) -> CredibleRpcConfig {
        let marker_override = match (self.marker_address, self.marker_slot, self.marker_value) {
            (Some(address), Some(slot), Some(value)) => {
                Some(CredibleMarkerOverride { address, slot, value })
            }
            _ => None,
        };

        CredibleRpcConfig {
            marker_override,
            retain_forwarded_txs_as_private: self.retain_forwarded_private,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// A helper type to parse Args more easily
    #[derive(Parser)]
    struct CommandParser<T: Args> {
        #[command(flatten)]
        args: T,
    }

    #[test]
    fn test_parse_credible_args_default() {
        let args = CommandParser::<CredibleArgs>::parse_from(["reth"]).args;
        assert_eq!(args, CredibleArgs::default());
    }

    #[test]
    fn test_parse_credible_args() {
        let address = Address::repeat_byte(0x11);
        let slot = B256::repeat_byte(0x22);
        let value = B256::repeat_byte(0x33);

        let args = CommandParser::<CredibleArgs>::parse_from([
            "reth",
            "--rpc.credible-marker-address",
            &address.to_string(),
            "--rpc.credible-marker-slot",
            &slot.to_string(),
            "--rpc.credible-marker-value",
            &value.to_string(),
            "--rpc.credible-retain-forwarded-private",
        ])
        .args;

        assert_eq!(
            args,
            CredibleArgs {
                marker_address: Some(address),
                marker_slot: Some(slot),
                marker_value: Some(value),
                retain_forwarded_private: true,
            }
        );
        assert_eq!(
            args.credible_config().marker_override,
            Some(CredibleMarkerOverride { address, slot, value })
        );
    }
}
