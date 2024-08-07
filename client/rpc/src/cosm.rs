// This file is part of Horizon.

// Copyright (C) 2023 Haderech Pte. Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use crate::internal_err;
use futures::future::TryFutureExt;
use jsonrpsee::{
	core::{async_trait, RpcResult},
	proc_macros::rpc,
};
use sc_transaction_pool_api::TransactionPool;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::{sha2_256, Bytes, H256};
use sp_runtime::{traits::Block as BlockT, transaction_validity::TransactionSource};
use std::{marker::PhantomData, sync::Arc};

#[rpc(server)]
#[async_trait]
pub trait CosmApi {
	#[method(name = "cosm_broadcastTx")]
	async fn broadcast_tx(&self, tx_bytes: Bytes) -> RpcResult<H256>;
}

pub struct Cosm<B: BlockT, C, P> {
	pool: Arc<P>,
	client: Arc<C>,
	_marker: PhantomData<B>,
}

impl<B: BlockT, C, P> Cosm<B, C, P> {
	pub fn new(pool: Arc<P>, client: Arc<C>) -> Self {
		Self { pool, client, _marker: Default::default() }
	}
}

#[async_trait]
impl<B, C, P> CosmApiServer for Cosm<B, C, P>
where
	B: BlockT,
	C: Send + Sync + 'static,
	C: ProvideRuntimeApi<B>,
	C: HeaderBackend<B> + 'static,
	C::Api: hp_rpc::ConvertTxRuntimeApi<B>,
	P: TransactionPool<Block = B> + 'static,
{
	async fn broadcast_tx(&self, tx_bytes: Bytes) -> RpcResult<H256> {
		use hp_rpc::ConvertTxRuntimeApi;

		let block_hash = self.client.info().best_hash;
		let extrinsic = self
			.client
			.runtime_api()
			.convert_tx(block_hash, tx_bytes.to_vec())
			.map_err(|_| internal_err("cannot access runtime api"))?;
		let tx_hash = H256(sha2_256(&tx_bytes));
		self.pool
			.submit_one(block_hash, TransactionSource::Local, extrinsic)
			.map_ok(move |_| tx_hash)
			.map_err(|e| internal_err(e.to_string()))
			.await
	}
}
