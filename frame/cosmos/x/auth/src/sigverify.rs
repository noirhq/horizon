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

use alloc::string::ToString;
use core::marker::PhantomData;
use cosmos_sdk_proto::{
	cosmos::{
		crypto::{multisig::LegacyAminoPubKey, secp256k1},
		tx::v1beta1::{ModeInfo, SignerInfo, Tx},
	},
	prost::Message,
	Any,
};
use pallet_cosmos::AddressMapping;
use pallet_cosmos_types::{address::acc_address_from_bech32, any_match, handler::AnteDecorator};
use pallet_cosmos_x_auth_signing::{
	sign_mode_handler::{traits::SignModeHandler, SignerData},
	sign_verifiable_tx::traits::SigVerifiableTx,
};
use ripemd::Digest;
use sp_core::{ecdsa, sha2_256, ByteArray, Get, H160};
use sp_runtime::{
	transaction_validity::{
		InvalidTransaction, TransactionValidity, TransactionValidityError, ValidTransaction,
	},
	SaturatedConversion,
};

pub struct SigVerificationDecorator<T>(PhantomData<T>);

impl<T> AnteDecorator for SigVerificationDecorator<T>
where
	T: frame_system::Config + pallet_cosmos::Config,
{
	fn ante_handle(tx: &Tx, _simulate: bool) -> TransactionValidity {
		let signatures = &tx.signatures;
		let signers =
			T::SigVerifiableTx::get_signers(tx).map_err(|_| InvalidTransaction::BadSigner)?;

		let auth_info = tx.auth_info.as_ref().ok_or(InvalidTransaction::BadSigner)?;
		if signatures.len() != signers.len() {
			return Err(InvalidTransaction::BadSigner.into());
		}

		if signatures.len() != auth_info.signer_infos.len() {
			return Err(InvalidTransaction::BadSigner.into());
		}

		for (i, sig) in signatures.iter().enumerate() {
			let signer = signers.get(i).ok_or(InvalidTransaction::BadSigner)?;

			let signer_info = auth_info.signer_infos.get(i).ok_or(InvalidTransaction::BadSigner)?;

			let (_hrp, signer_addr_raw) =
				acc_address_from_bech32(signer).map_err(|_| InvalidTransaction::BadSigner)?;

			if signer_addr_raw.len() != 20 {
				return Err(InvalidTransaction::BadSigner.into());
			}

			let who = T::AddressMapping::into_account_id(H160::from_slice(&signer_addr_raw));
			let sequence = frame_system::Pallet::<T>::account_nonce(&who).saturated_into();

			if signer_info.sequence > sequence {
				return Err(InvalidTransaction::Future.into());
			} else if signer_info.sequence < sequence {
				return Err(InvalidTransaction::Stale.into());
			}

			let public_key =
				signer_info.public_key.as_ref().ok_or(InvalidTransaction::BadSigner)?;
			let chain_id = T::ChainId::get().to_string();
			let signer_data = SignerData {
				address: signer.clone(),
				chain_id,
				account_number: 0,
				sequence: signer_info.sequence,
				pub_key: public_key.clone(),
			};

			let sign_mode = signer_info.mode_info.as_ref().ok_or(InvalidTransaction::BadSigner)?;

			Self::verify_signature(public_key, &signer_data, sign_mode, sig, tx)?;
		}

		Ok(ValidTransaction::default())
	}
}

impl<T> SigVerificationDecorator<T>
where
	T: pallet_cosmos::Config,
{
	fn verify_signature(
		public_key: &Any,
		signer_data: &SignerData,
		sign_mode: &ModeInfo,
		signature: &[u8],
		tx: &Tx,
	) -> Result<(), TransactionValidityError> {
		any_match!(
			public_key, {
				secp256k1::PubKey => {
					let public_key =
						secp256k1::PubKey::decode(&mut &*public_key.value).map_err(|_| {
							InvalidTransaction::BadSigner
						})?;
					let mut hasher = ripemd::Ripemd160::new();
					hasher.update(sha2_256(&public_key.key));
					let address = H160::from_slice(&hasher.finalize());

					let (_hrp, signer_addr_raw) =
						acc_address_from_bech32(&signer_data.address).map_err(|_| {
							InvalidTransaction::BadSigner
						})?;
					if signer_addr_raw.len() != 20 {
						return Err(InvalidTransaction::BadSigner.into());
					}
					if  H160::from_slice(&signer_addr_raw) != address {
						return Err(InvalidTransaction::BadSigner.into());
					}

					let sign_bytes = T::SignModeHandler::get_sign_bytes(sign_mode, signer_data, tx)
						.map_err(|_| InvalidTransaction::Call)?;

					if !ecdsa_verify(signature, &sign_bytes, &public_key.key) {
						return Err(InvalidTransaction::BadProof.into());
					}

					Ok(())
				}
			},
			Err(InvalidTransaction::BadSigner.into())
		)
	}
}

pub fn ecdsa_verify(signature: &[u8], message: &[u8], public_key: &[u8]) -> bool {
	let pub_key = match ecdsa::Public::from_slice(public_key) {
		Ok(pub_key) => pub_key,
		Err(_) => return false,
	};
	let msg = sha2_256(message);

	if signature.len() == 64 {
		for rec_id in 0..=3 {
			let mut rec_sig = [0u8; 65];
			rec_sig[0..signature.len()].copy_from_slice(signature);
			rec_sig[64] = rec_id;
			let sig = ecdsa::Signature(rec_sig);

			if sp_io::crypto::ecdsa_verify_prehashed(&sig, &msg, &pub_key) {
				return true;
			}
		}
		false
	} else if signature.len() == 65 {
		match ecdsa::Signature::try_from(signature) {
			Ok(sig) => sp_io::crypto::ecdsa_verify_prehashed(&sig, &msg, &pub_key),
			Err(_) => false,
		}
	} else {
		false
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;

	#[test]
	fn ecdsa_verify_test() {
		let sig = hex::decode("f7e0d198c62821cc5817c8e935f523308301e29819f5d882f3249b9e173a614f38000ddbff446c0abfa7c7d019dbb17072b28933fc8187c973fbf03d0459f76e").unwrap();
		let message = hex::decode("0a93010a90010a1c2f636f736d6f732e62616e6b2e763162657461312e4d736753656e6412700a2d636f736d6f7331716436396e75776a393567746134616b6a677978746a39756a6d7a34773865646d7179737177122d636f736d6f7331676d6a32657861673033747467616670726b6463337438383067726d61396e776566636432771a100a057561746f6d12073130303030303012710a4e0a460a1f2f636f736d6f732e63727970746f2e736563703235366b312e5075624b657912230a21020a1091341fe5664bfa1782d5e04779689068c916b04cb365ec3153755684d9a112040a020801121f0a150a057561746f6d120c3838363838303030303030301080c0f1c59495141a1174686574612d746573746e65742d30303120ad8a2e").unwrap();
		let public_key =
			hex::decode("020a1091341fe5664bfa1782d5e04779689068c916b04cb365ec3153755684d9a1")
				.unwrap();

		assert!(ecdsa_verify(&sig, &message, &public_key));
	}
}

pub struct ValidateSigCountDecorator<T>(core::marker::PhantomData<T>);

impl<T> AnteDecorator for ValidateSigCountDecorator<T>
where
	T: pallet_cosmos::Config,
{
	fn ante_handle(tx: &Tx, _simulate: bool) -> TransactionValidity {
		let mut sig_count = 0u64;

		let auth_info = tx.auth_info.as_ref().ok_or(InvalidTransaction::BadSigner)?;
		for SignerInfo { public_key, .. } in auth_info.signer_infos.iter() {
			let public_key = public_key.as_ref().ok_or(InvalidTransaction::BadSigner)?;
			sig_count = sig_count.saturating_add(Self::count_sub_keys(public_key)?);

			if sig_count > T::TxSigLimit::get() {
				return Err(InvalidTransaction::BadProof.into());
			}
		}

		Ok(ValidTransaction::default())
	}
}

impl<T> ValidateSigCountDecorator<T> {
	fn count_sub_keys(pubkey: &Any) -> Result<u64, TransactionValidityError> {
		// TODO: Support legacy multi signatures.
		if LegacyAminoPubKey::decode(&mut &*pubkey.value).is_ok() {
			Err(InvalidTransaction::BadProof.into())
		} else {
			Ok(1)
		}
	}
}

pub struct IncrementSequenceDecorator<T>(core::marker::PhantomData<T>);

impl<T> AnteDecorator for IncrementSequenceDecorator<T>
where
	T: frame_system::Config + pallet_cosmos::Config,
{
	fn ante_handle(tx: &Tx, _simulate: bool) -> TransactionValidity {
		let signers = T::SigVerifiableTx::get_signers(tx).map_err(|_| InvalidTransaction::Call)?;
		for signer in signers.iter() {
			let (_hrp, address_raw) =
				acc_address_from_bech32(signer).map_err(|_| InvalidTransaction::BadSigner)?;
			if address_raw.len() != 20 {
				return Err(InvalidTransaction::BadSigner.into());
			}

			let account = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
			frame_system::pallet::Pallet::<T>::inc_account_nonce(account);
		}

		Ok(ValidTransaction::default())
	}
}
