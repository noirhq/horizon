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

use crate::{error::DecodeMsgError, AccountId, Any, Coin};
#[cfg(feature = "std")]
use cosmrs::tx::Msg;
#[cfg(feature = "with-codec")]
use parity_scale_codec::{Decode, Encode};
#[cfg(feature = "with-codec")]
use scale_info::TypeInfo;
#[cfg(feature = "with-serde")]
use serde::{Deserialize, Serialize};
#[cfg(not(feature = "std"))]
use sp_std::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct MsgSend {
	pub from_address: AccountId,
	pub to_address: AccountId,
	pub amount: Vec<Coin>,
}

#[cfg(feature = "std")]
impl TryFrom<Any> for MsgSend {
	type Error = DecodeMsgError;

	fn try_from(msg: Any) -> Result<Self, Self::Error> {
		let type_url =
			String::from_utf8(msg.type_url).map_err(|_| DecodeMsgError::InvalidTypeUrl)?;
		if type_url != "/cosmos.bank.v1beta1.MsgSend" {
			return Err(DecodeMsgError::UnsupportedType);
		}

		let any = cosmrs::Any { type_url, value: msg.value };
		let msg_send =
			cosmrs::bank::MsgSend::from_any(&any).map_err(|_| DecodeMsgError::InvalidValue)?;

		let from_address = msg_send.from_address.into();
		let to_address = msg_send.to_address.into();
		let amount = msg_send.amount.iter().map(From::from).collect::<Vec<Coin>>();

		Ok(MsgSend { from_address, to_address, amount })
	}
}

#[cfg(feature = "std")]
pub fn to_scale(type_url: &[u8], value: &[u8]) -> Result<(Vec<u8>, Vec<u8>), DecodeMsgError> {
	match core::str::from_utf8(type_url).map_err(|_| DecodeMsgError::InvalidTypeUrl)? {
		"/cosmos.bank.v1beta1.MsgSend" => {
			let any = Any { type_url: type_url.to_vec(), value: value.to_vec() };
			let msg_send: MsgSend = any.try_into()?;
			Ok((type_url.to_vec(), msg_send.encode()))
		},
		_ => Err(DecodeMsgError::InvalidTypeUrl),
	}
}
