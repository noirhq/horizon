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

pub mod error;
#[cfg(feature = "std")]
mod legacy;

#[cfg(feature = "std")]
use core::str::FromStr;
#[cfg(feature = "std")]
use cosmrs::tendermint::chain;
#[cfg(feature = "std")]
use cosmrs::tx::SignMode;
#[cfg(feature = "std")]
use cosmrs::{self, tx::MessageExt};
#[cfg(feature = "std")]
use error::DecodeTxError;
#[cfg(feature = "std")]
use legacy::StdSignDoc;
#[cfg(feature = "with-codec")]
use parity_scale_codec::{Decode, Encode};
#[cfg(feature = "with-codec")]
use scale_info::TypeInfo;
#[cfg(feature = "with-serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use sp_core::hashing::sha2_256;
#[cfg(feature = "std")]
use sp_core::Bytes;
use sp_core::{H160, H256};
use sp_std::vec::Vec;

pub type SequenceNumber = u64;
pub type SignatureBytes = Vec<u8>;
pub type Gas = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Tx {
	pub body: Body,
	pub auth_info: AuthInfo,
	pub signatures: Vec<SignatureBytes>,
	pub hash: H256,
	pub len: u32,
}

impl Tx {
	pub fn is_valid(&self) -> bool {
		return self.validate_basic() && self.validate_extras()
	}

	fn validate_basic(&self) -> bool {
		if self.auth_info.signer_infos.is_empty() {
			return false
		}
		if self.body.messages.is_empty() {
			return false
		}
		if self.signatures.is_empty() {
			return false
		}
		if self.auth_info.fee.amount.is_empty() {
			return false
		}
		return true
	}

	fn validate_extras(&self) -> bool {
		if self.auth_info.signer_infos.len() > 1 {
			return false
		}
		if self.body.messages.len() > 1 {
			return false
		}
		if self.signatures.len() > 1 {
			return false
		}
		if self.auth_info.fee.amount.len() > 1 {
			return false
		}
		return true
	}
}

#[cfg(feature = "std")]
impl Tx {
	pub fn decode(tx_bytes: &Bytes, chain_id: &str) -> Result<Self, DecodeTxError> {
		if tx_bytes.is_empty() {
			return Err(DecodeTxError::EmptyTxBytes)
		}

		let tx_origin =
			cosmrs::Tx::from_bytes(tx_bytes).map_err(|_| DecodeTxError::InvalidTxData)?;
		let _ = validate_basic(&tx_origin)?;
		let _ = validate_extras(&tx_origin)?;

		let signatures =
			tx_origin.signatures.iter().map(|s| s.clone()).collect::<Vec<SignatureBytes>>();
		let sign_doc = match tx_origin.auth_info.signer_infos[0].mode_info {
			cosmrs::tx::ModeInfo::Single(single) => match single.mode {
				SignMode::Direct => {
					let chain_id = chain::Id::from_str(chain_id).unwrap();
					let sign_doc = cosmrs::tx::SignDoc::new(
						&tx_origin.body,
						&tx_origin.auth_info,
						&chain_id,
						0u64,
					)
					.map_err(|_| DecodeTxError::InvalidTxData)?;
					sign_doc.into_bytes().map_err(|_| DecodeTxError::InvalidSignDoc)?
				},
				SignMode::LegacyAminoJson => StdSignDoc::new(&tx_origin, chain_id)?.to_bytes()?,
				_ => return Err(DecodeTxError::UnsupportedSignMode),
			},
			_ => return Err(DecodeTxError::UnsupportedSignMode),
		};
		let len = tx_bytes.len().try_into().map_err(|_| DecodeTxError::TooLongTxBytes)?;
		Ok(Self {
			body: tx_origin.body.try_into()?,
			auth_info: tx_origin.auth_info.try_into()?,
			signatures,
			hash: sha2_256(&sign_doc).into(),
			len,
		})
	}
}

#[cfg(feature = "std")]
fn validate_basic(tx: &cosmrs::Tx) -> Result<(), DecodeTxError> {
	if tx.auth_info.signer_infos.is_empty() {
		return Err(DecodeTxError::EmptySigners)
	}
	if tx.body.messages.is_empty() {
		return Err(DecodeTxError::EmptyMessages)
	}
	if tx.signatures.is_empty() {
		return Err(DecodeTxError::EmptySignatures)
	}
	if tx.auth_info.fee.amount.is_empty() {
		return Err(DecodeTxError::EmptyFeeAmount)
	}
	Ok(())
}

#[cfg(feature = "std")]
fn validate_extras(tx: &cosmrs::Tx) -> Result<(), DecodeTxError> {
	if tx.auth_info.signer_infos.len() > 1 {
		return Err(DecodeTxError::TooManySigners)
	}
	if tx.body.messages.len() > 1 {
		return Err(DecodeTxError::TooManyMessages)
	}
	if tx.signatures.len() > 1 {
		return Err(DecodeTxError::TooManySignatures)
	}
	if tx.auth_info.fee.amount.len() > 1 {
		return Err(DecodeTxError::TooManyFeeAmount)
	}
	Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Body {
	pub messages: Vec<Msg>,
}

#[cfg(feature = "std")]
impl TryFrom<cosmrs::tx::Body> for Body {
	type Error = DecodeTxError;

	fn try_from(body: cosmrs::tx::Body) -> Result<Self, Self::Error> {
		let mut messages: Vec<Msg> = Vec::new();
		for msg in body.messages {
			messages.push(msg.try_into()?);
		}
		Ok(Self { messages })
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub enum Msg {
	MsgSend { from_address: H160, to_address: H160, amount: Vec<Coin> },
}

#[cfg(feature = "std")]
impl TryFrom<cosmrs::Any> for Msg {
	type Error = DecodeTxError;

	fn try_from(any: cosmrs::Any) -> Result<Self, Self::Error> {
		Self::try_from(&any)
	}
}

#[cfg(feature = "std")]
impl TryFrom<&cosmrs::Any> for Msg {
	type Error = DecodeTxError;

	fn try_from(any: &cosmrs::Any) -> Result<Self, Self::Error> {
		if any.type_url == "/cosmos.bank.v1beta1.MsgSend" {
			let typed_msg = cosmrs::proto::cosmos::bank::v1beta1::MsgSend::from_any(any)
				.map_err(|_| DecodeTxError::InvalidMsgData)?;
			let typed_msg: cosmrs::bank::MsgSend =
				typed_msg.try_into().map_err(|_| DecodeTxError::InvalidMsgData)?;
			if typed_msg.amount.is_empty() {
				return Err(DecodeTxError::EmptyMsgSendAmount)
			}
			if typed_msg.amount.len() > 1 {
				return Err(DecodeTxError::TooManyMsgSendAmount)
			}
			let amount = typed_msg.amount.iter().map(|coin| coin.into()).collect::<Vec<Coin>>();
			let mut from_address: [u8; 20] = [0u8; 20];
			from_address.copy_from_slice(&typed_msg.from_address.to_bytes()[..]);
			let mut to_address: [u8; 20] = [0u8; 20];
			to_address.copy_from_slice(&typed_msg.to_address.to_bytes()[..]);

			Ok(Msg::MsgSend {
				from_address: from_address.into(),
				to_address: to_address.into(),
				amount,
			})
		} else {
			Err(DecodeTxError::UnsupportedMsgType)
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct AuthInfo {
	pub signer_infos: Vec<SignerInfo>,
	pub fee: Fee,
}

#[cfg(feature = "std")]
impl TryFrom<cosmrs::tx::AuthInfo> for AuthInfo {
	type Error = DecodeTxError;

	fn try_from(auth_info: cosmrs::tx::AuthInfo) -> Result<Self, Self::Error> {
		let mut signer_infos: Vec<SignerInfo> = Vec::new();
		for signer_info in auth_info.signer_infos {
			signer_infos.push(signer_info.try_into()?);
		}
		Ok(Self { signer_infos, fee: auth_info.fee.try_into()? })
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct SignerInfo {
	pub public_key: Option<SignerPublicKey>,
	pub sequence: SequenceNumber,
}

#[cfg(feature = "std")]
impl TryFrom<cosmrs::tx::SignerInfo> for SignerInfo {
	type Error = DecodeTxError;

	fn try_from(signer_info: cosmrs::tx::SignerInfo) -> Result<Self, Self::Error> {
		let public_key = match signer_info.public_key {
			Some(pubkey) => match pubkey {
				cosmrs::tx::SignerPublicKey::Single(pk) => match pk.type_url() {
					cosmrs::crypto::PublicKey::ED25519_TYPE_URL => {
						let mut raw_bytes: [u8; 32] = [0u8; 32];
						raw_bytes.copy_from_slice(&pk.to_bytes()[..]);
						Some(SignerPublicKey::Single(PublicKey::Ed25519(raw_bytes)))
					},
					cosmrs::crypto::PublicKey::SECP256K1_TYPE_URL => {
						let mut raw_bytes: [u8; 33] = [0u8; 33];
						raw_bytes.copy_from_slice(&pk.to_bytes()[..]);
						Some(SignerPublicKey::Single(PublicKey::Secp256k1(raw_bytes)))
					},
					_ => return Err(DecodeTxError::UnsupportedSignerType),
				},
				_ => return Err(DecodeTxError::UnsupportedSignerType),
			},
			None => None,
		};
		Ok(Self { public_key, sequence: signer_info.sequence })
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub enum SignerPublicKey {
	/// Single singer.
	Single(PublicKey),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub enum PublicKey {
	Ed25519([u8; 32]),
	Secp256k1([u8; 33]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Fee {
	pub amount: Vec<Coin>,
	pub gas_limit: Gas,
}

#[cfg(feature = "std")]
impl TryFrom<cosmrs::tx::Fee> for Fee {
	type Error = DecodeTxError;

	fn try_from(fee: cosmrs::tx::Fee) -> Result<Self, Self::Error> {
		if fee.amount.is_empty() {
			return Err(DecodeTxError::EmptyFeeAmount)
		}
		let amount = fee.amount.iter().map(|coin| coin.into()).collect::<Vec<Coin>>();
		Ok(Self { amount, gas_limit: fee.gas_limit })
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Account {
	pub sequence: SequenceNumber,
	pub amount: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "with-codec", derive(Encode, Decode, TypeInfo))]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Coin {
	pub denom: Vec<u8>,
	pub amount: u128,
}

#[cfg(feature = "std")]
impl From<&cosmrs::Coin> for Coin {
	fn from(coin: &cosmrs::Coin) -> Self {
		let denom = coin.denom.as_ref().as_bytes().to_vec();
		Self { denom, amount: coin.amount }
	}
}

#[cfg(test)]
mod tests {
	use crate::legacy::StdSignDoc;
	use base64ct::{Base64, Encoding};
	use sp_core::hashing::sha2_256;

	#[test]
	fn test_sign_amino_doc_hash() {
		let tx_bytes =  "Cp0BCpgBChwvY29zbW9zLmJhbmsudjFiZXRhMS5Nc2dTZW5kEngKLWNvc21vczFwdnJhbjRkbDl1NzNxNXo0dzNtY2xnbDUzMGtsdHdxY2EwMnk4ZBItY29zbW9zMThwd3ZxajB0ZG5oZ20zM241bG4wMjBqdnk4MjBmcjI5aDJtc213GhgKBHVjZHQSEDEwMDAwMDAwMDAwMDAwMDASABJkClAKRgofL2Nvc21vcy5jcnlwdG8uc2VjcDI1NmsxLlB1YktleRIjCiED9ZPCan9HZlZbW/+hDSWLfy6cW+aPzrjSILmLmCSnUUcSBAoCCH8YABIQCgoKBHVjZHQSAjI1EKCNBhpA0YAS1zXHInFcdO2w/tZjTEWa9fNs53mTsitzpx21mxRVaJv8lJ2eErg+/IWvCWLHfsh71fMxOY2AJ7DrQIzTxg==";
		let tx_bytes = Base64::decode_vec(tx_bytes).unwrap();
		let tx = cosmrs::Tx::from_bytes(&tx_bytes).unwrap();
		let sign_doc = StdSignDoc::new(&tx, "noir").unwrap();
		let hash = sha2_256(&sign_doc.to_bytes().unwrap());
		assert_eq!(
			array_bytes::bytes2hex("", &hash),
			"c853e81f04e499cb842c67b8c75a1e23d60bdc02ee51ff9f5e28925f5d9706a8"
		);
	}
}
