use alloy_primitives::Address;
use clap::Args;
use reth_rpc_eth_types::CredibleRpcConfig;

/// Parameters to configure Credible Layer RPC.
#[derive(Debug, Clone, Copy, Default, Args, PartialEq, Eq)]
#[command(next_help_heading = "Credible Layer")]
pub struct CredibleArgs {
    /// Address of the on-chain `CredibleRegistry` contract.
    ///
    /// When set, the marker override for call-like RPC methods is derived per-request from the
    /// registry's `_credibleBlocks` mapping instead of a static override.
    #[arg(long = "rpc.credible-registry-address", value_name = "ADDRESS")]
    pub registry_address: Option<Address>,

    /// Retains transactions accepted by `--rpc.forwarder` as private pool transactions
    #[arg(long = "rpc.credible-retain-forwarded-private", default_value_t = false)]
    pub retain_forwarded_private: bool,
}

impl From<CredibleArgs> for CredibleRpcConfig {
    fn from(args: CredibleArgs) -> Self {
        Self {
            registry_address: args.registry_address,
            retain_forwarded_txs_as_private: args.retain_forwarded_private,
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

        let args = CommandParser::<CredibleArgs>::parse_from([
            "reth",
            "--rpc.credible-registry-address",
            &address.to_string(),
            "--rpc.credible-retain-forwarded-private",
        ])
        .args;

        assert_eq!(
            args,
            CredibleArgs { registry_address: Some(address), retain_forwarded_private: true }
        );
        assert_eq!(CredibleRpcConfig::from(args).registry_address, Some(address));
    }
}
