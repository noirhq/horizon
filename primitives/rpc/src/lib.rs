// This file is part of Horizon.

// Copyright (C) 2023 Haderech Pte. Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::too_many_arguments)]
#![deny(unused_crate_dependencies)]

use primitive_types::H256;
use sp_runtime::traits::Block as BlockT;

sp_api::decl_runtime_apis! {
	pub trait CosmosRuntimeRPCApi {
		fn broadcast_tx(tx: hp_cosmos::Tx) -> H256;
	}

	pub trait ConvertTransactionRuntimeApi {
		fn convert_transaction(tx: hp_cosmos::Tx) -> <Block as BlockT>::Extrinsic;
	}
}

pub trait ConvertTransaction<E> {
	fn convert_transaction(&self, tx: hp_cosmos::Tx) -> E;
}