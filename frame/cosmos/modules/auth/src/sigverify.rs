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

use hp_cosmos::{PublicKey, SignerPublicKey, Tx};
use hp_io::crypto::secp256k1_ecdsa_verify;
use pallet_cosmos_modules::ante::AnteHandler;
use sp_runtime::transaction_validity::{InvalidTransaction, TransactionValidityError};
use sp_std::marker::PhantomData;

pub struct SigVerificationHandler<T>(PhantomData<T>);

impl<T> AnteHandler for SigVerificationHandler<T>
where
	T: frame_system::Config,
{
	fn handle(tx: &Tx) -> Result<(), TransactionValidityError> {
		let signatures = &tx.signatures;
		let signers = &tx.auth_info.signer_infos;

		if signatures.len() != signers.len() {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner));
		}

		for (i, sig) in signatures.iter().enumerate() {
			let signer = signers
				.get(i)
				.ok_or(TransactionValidityError::Invalid(InvalidTransaction::BadSigner))?;

			// TODO: Support other types of Signers as well
			let public_key = signer
				.public_key
				.as_ref()
				.ok_or(TransactionValidityError::Invalid(InvalidTransaction::BadSigner))?;

			if let SignerPublicKey::Single(PublicKey::Secp256k1(public_key)) = public_key {
				// TODO: Validate if the sequence of the account is valid

				if !secp256k1_ecdsa_verify(sig, &tx.hash.0, public_key) {
					return Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof));
				}
			} else {
				return Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner));
			}
		}

		Ok(())
	}
}
