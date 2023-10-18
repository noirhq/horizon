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

use crate::error::DecodeTxError;
use cosmrs::{proto::cosmos::bank::v1beta1::MsgSend, tx::MessageExt};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Fee {
	pub amount: Vec<Coin>,
	pub gas: String,
}

impl From<cosmrs::tx::Fee> for Fee {
	fn from(fee: cosmrs::tx::Fee) -> Self {
		Self {
			amount: fee.amount.iter().map(|a| a.clone().into()).collect::<Vec<Coin>>(),
			gas: fee.gas_limit.to_string(),
		}
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Coin {
	pub amount: String,
	pub denom: String,
}

impl From<cosmrs::Coin> for Coin {
	fn from(coin: cosmrs::Coin) -> Self {
		Self { amount: coin.amount.to_string(), denom: coin.denom.to_string() }
	}
}

impl From<&cosmrs::proto::cosmos::base::v1beta1::Coin> for Coin {
	fn from(coin: &cosmrs::proto::cosmos::base::v1beta1::Coin) -> Self {
		Self { amount: coin.amount.to_string(), denom: coin.denom.to_string() }
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Msg {
	pub r#type: String,
	pub value: TypedMsg,
}

impl TryFrom<&cosmrs::Any> for Msg {
	type Error = DecodeTxError;

	fn try_from(msg: &cosmrs::Any) -> Result<Self, Self::Error> {
		if msg.type_url == "/cosmos.bank.v1beta1.MsgSend" {
			let msg_send = MsgSend::from_any(msg).map_err(|_| DecodeTxError::InvalidMsgData)?;
			let amount = msg_send.amount.iter().map(|c| c.into()).collect::<Vec<Coin>>();
			Ok(Self {
				r#type: "cosmos-sdk/MsgSend".to_string(),
				value: TypedMsg::MsgSend {
					amount,
					from_address: msg_send.from_address.into(),
					to_address: msg_send.to_address.into(),
				},
			})
		} else {
			Err(DecodeTxError::UnsupportedMsgType)
		}
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum TypedMsg {
	MsgSend { amount: Vec<Coin>, from_address: String, to_address: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StdSignDoc {
	pub chain_id: String,
	pub sequence: String,
	pub account_number: String,
	pub fee: Fee,
	pub memo: String,
	pub msgs: Vec<Msg>,
}

impl StdSignDoc {
	pub fn to_json(&self) -> Result<String, DecodeTxError> {
		Ok(serde_json::to_value(self)
			.map_err(|_| DecodeTxError::InvalidSignDoc)?
			.to_string())
	}

	pub fn to_bytes(&self) -> Result<Vec<u8>, DecodeTxError> {
		Ok(self.to_json()?.as_bytes().to_vec())
	}

	pub fn new(tx: &cosmrs::Tx, chain_id: &str) -> Result<Self, DecodeTxError> {
		if tx.auth_info.signer_infos.is_empty() {
			return Err(DecodeTxError::EmptySigners)
		}
		if let cosmrs::tx::ModeInfo::Single(_) = &tx.auth_info.signer_infos[0].mode_info {
			let mut msgs: Vec<Msg> = Vec::new();
			for msg in &tx.body.messages {
				msgs.push(msg.try_into()?);
			}
			return Ok(Self {
				account_number: "0".to_string(),
				chain_id: chain_id.to_string(),
				fee: tx.auth_info.fee.clone().into(),
				memo: tx.body.memo.clone(),
				msgs,
				sequence: tx.auth_info.signer_infos[0].sequence.to_string(),
			})
		} else {
			return Err(DecodeTxError::UnsupportedSignMode)
		}
	}
}
