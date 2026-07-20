use core::fmt;
use std::collections::BTreeMap;

use alloy_consensus::Transaction;
use alloy_primitives::Address;
use alloy_rpc_types_txpool::{
    TxpoolContent, TxpoolContentFrom, TxpoolInspect, TxpoolInspectSummary, TxpoolStatus,
};
use async_trait::async_trait;
use jsonrpsee::core::RpcResult;
use reth_primitives_traits::NodePrimitives;
use reth_rpc_api::TxPoolApiServer;
use reth_rpc_convert::{RpcConvert, RpcTypes};
use reth_rpc_eth_api::RpcTransaction;
use reth_rpc_eth_types::CredibleRpcConfig;
use reth_transaction_pool::{
    AllPoolTransactions, PoolConsensusTx, PoolTransaction, TransactionPool,
};
use tracing::trace;

/// `txpool` API implementation.
///
/// This type provides the functionality for handling `txpool` related requests.
#[derive(Clone)]
pub struct TxPoolApi<Pool, Eth> {
    /// An interface to interact with the pool
    pool: Pool,
    converter: Eth,
    credible_config: CredibleRpcConfig,
}

impl<Pool, Eth> TxPoolApi<Pool, Eth> {
    /// Creates a new instance of `TxpoolApi`.
    pub const fn new(pool: Pool, converter: Eth, credible_config: CredibleRpcConfig) -> Self {
        Self { pool, converter, credible_config }
    }
}

impl<Pool, Eth> TxPoolApi<Pool, Eth>
where
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus: Transaction>> + 'static,
    Eth: RpcConvert<Primitives: NodePrimitives<SignedTx = PoolConsensusTx<Pool>>>,
{
    fn content(&self) -> Result<TxpoolContent<RpcTransaction<Eth::Network>>, Eth::Error> {
        #[inline]
        fn insert<Tx, RpcTxB>(
            tx: &Tx,
            content: &mut BTreeMap<
                Address,
                BTreeMap<String, <RpcTxB::Network as RpcTypes>::TransactionResponse>,
            >,
            resp_builder: &RpcTxB,
        ) -> Result<(), RpcTxB::Error>
        where
            Tx: PoolTransaction,
            RpcTxB: RpcConvert<Primitives: NodePrimitives<SignedTx = Tx::Consensus>>,
        {
            content.entry(tx.sender()).or_default().insert(
                tx.nonce().to_string(),
                resp_builder.fill_pending(tx.clone_into_consensus())?,
            );

            Ok(())
        }

        // With Credible Layer retention, private-origin pool txs must not leak before inclusion.
        let hide_private = self.credible_config.hide_private_pool_txs();
        let AllPoolTransactions { pending, queued } = self.pool.all_transactions();

        let mut content = TxpoolContent::default();
        for pending in pending {
            if hide_private && pending.origin.is_private() {
                continue;
            }
            insert::<_, Eth>(&pending.transaction, &mut content.pending, &self.converter)?;
        }
        for queued in queued {
            if hide_private && queued.origin.is_private() {
                continue;
            }
            insert::<_, Eth>(&queued.transaction, &mut content.queued, &self.converter)?;
        }

        Ok(content)
    }
}

#[async_trait]
impl<Pool, Eth> TxPoolApiServer<RpcTransaction<Eth::Network>> for TxPoolApi<Pool, Eth>
where
    Pool: TransactionPool<Transaction: PoolTransaction<Consensus: Transaction>> + 'static,
    Eth: RpcConvert<Primitives: NodePrimitives<SignedTx = PoolConsensusTx<Pool>>> + 'static,
{
    /// Returns the number of transactions currently pending for inclusion in the next block(s), as
    /// well as the ones that are being scheduled for future execution only.
    /// Ref: [Here](https://geth.ethereum.org/docs/rpc/ns-txpool#txpool_status)
    ///
    /// Handler for `txpool_status`
    async fn txpool_status(&self) -> RpcResult<TxpoolStatus> {
        trace!(target: "rpc::eth", "Serving txpool_status");
        let (pending, queued) = self.pool.pending_and_queued_txn_count();
        // With Credible Layer retention, exclude private-origin txs from the public counts.
        if self.credible_config.hide_private_pool_txs() {
            let (private_pending, private_queued) =
                self.pool.private_pending_and_queued_txn_count();
            return Ok(TxpoolStatus {
                pending: pending.saturating_sub(private_pending) as u64,
                queued: queued.saturating_sub(private_queued) as u64,
            });
        }
        Ok(TxpoolStatus { pending: pending as u64, queued: queued as u64 })
    }

    /// Returns a summary of all the transactions currently pending for inclusion in the next
    /// block(s), as well as the ones that are being scheduled for future execution only.
    ///
    /// See [here](https://geth.ethereum.org/docs/rpc/ns-txpool#txpool_inspect) for more details
    ///
    /// Handler for `txpool_inspect`
    async fn txpool_inspect(&self) -> RpcResult<TxpoolInspect> {
        trace!(target: "rpc::eth", "Serving txpool_inspect");

        #[inline]
        fn insert<T: PoolTransaction<Consensus: Transaction>>(
            tx: &T,
            inspect: &mut BTreeMap<Address, BTreeMap<String, TxpoolInspectSummary>>,
        ) {
            let entry = inspect.entry(tx.sender()).or_default();
            let tx = tx.clone_into_consensus();
            entry.insert(tx.nonce().to_string(), tx.into_inner().into());
        }

        // With Credible Layer retention, private-origin pool txs must not leak before inclusion.
        let hide_private = self.credible_config.hide_private_pool_txs();
        let AllPoolTransactions { pending, queued } = self.pool.all_transactions();

        Ok(TxpoolInspect {
            pending: pending.iter().filter(|tx| !hide_private || !tx.origin.is_private()).fold(
                Default::default(),
                |mut acc, tx| {
                    insert(&tx.transaction, &mut acc);
                    acc
                },
            ),
            queued: queued.iter().filter(|tx| !hide_private || !tx.origin.is_private()).fold(
                Default::default(),
                |mut acc, tx| {
                    insert(&tx.transaction, &mut acc);
                    acc
                },
            ),
        })
    }

    /// Retrieves the transactions contained within the txpool, returning pending as well as queued
    /// transactions of this address, grouped by nonce.
    ///
    /// See [here](https://geth.ethereum.org/docs/rpc/ns-txpool#txpool_contentFrom) for more details
    /// Handler for `txpool_contentFrom`
    async fn txpool_content_from(
        &self,
        from: Address,
    ) -> RpcResult<TxpoolContentFrom<RpcTransaction<Eth::Network>>> {
        trace!(target: "rpc::eth", ?from, "Serving txpool_contentFrom");
        Ok(self.content().map_err(Into::into)?.remove_from(&from))
    }

    /// Returns the details of all transactions currently pending for inclusion in the next
    /// block(s), as well as the ones that are being scheduled for future execution only.
    ///
    /// See [here](https://geth.ethereum.org/docs/rpc/ns-txpool#txpool_content) for more details
    /// Handler for `txpool_content`
    async fn txpool_content(&self) -> RpcResult<TxpoolContent<RpcTransaction<Eth::Network>>> {
        trace!(target: "rpc::eth", "Serving txpool_content");
        Ok(self.content().map_err(Into::into)?)
    }
}

impl<Pool, Eth> fmt::Debug for TxPoolApi<Pool, Eth> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TxpoolApi").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::helpers::types::EthRpcConverter;
    use reth_chainspec::MAINNET;
    use reth_rpc_eth_types::receipt::EthReceiptConverter;
    use reth_transaction_pool::{
        test_utils::{testing_pool, MockTransaction},
        TransactionOrigin,
    };

    #[tokio::test]
    async fn txpool_hides_retained_private_txs() {
        let public_sender = Address::repeat_byte(0x11);
        let private_sender = Address::repeat_byte(0x22);

        let pool = testing_pool();
        pool.add_transaction(
            TransactionOrigin::External,
            MockTransaction::eip1559().with_nonce(0).with_sender(public_sender),
        )
        .await
        .unwrap();
        pool.add_transaction(
            TransactionOrigin::Private,
            MockTransaction::eip1559().with_nonce(0).with_sender(private_sender),
        )
        .await
        .unwrap();

        let converter = EthRpcConverter::new(EthReceiptConverter::new(MAINNET.clone()));

        // Retention enabled: the private-origin tx is hidden, the public one remains.
        let api = TxPoolApi::new(
            pool.clone(),
            converter.clone(),
            CredibleRpcConfig { retain_forwarded_txs_as_private: true, ..Default::default() },
        );
        let content = api.txpool_content().await.unwrap();
        assert!(
            content.pending.contains_key(&public_sender) ||
                content.queued.contains_key(&public_sender)
        );
        assert!(
            !content.pending.contains_key(&private_sender) &&
                !content.queued.contains_key(&private_sender)
        );
        let status = api.txpool_status().await.unwrap();
        assert_eq!(status.pending + status.queued, 1);

        // Retention disabled: both transactions are visible.
        let api = TxPoolApi::new(pool, converter, CredibleRpcConfig::default());
        let status = api.txpool_status().await.unwrap();
        assert_eq!(status.pending + status.queued, 2);
    }
}
