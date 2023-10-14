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

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::comparison_chain, clippy::large_enum_variant)]
#![deny(unused_crate_dependencies)]

pub mod errors;
pub mod handler;
pub mod weights;

pub use self::{handler::MsgHandler, pallet::*};
use crate::errors::{CosmosError, CosmosErrorCode};
use frame_support::{
	codec::{Decode, Encode, MaxEncodedLen},
	dispatch::{DispatchErrorWithPostInfo, DispatchInfo, PostDispatchInfo},
	pallet_prelude::{DispatchClass, DispatchResultWithPostInfo, Pays},
	scale_info::TypeInfo,
	traits::{
		tokens::{fungible::Inspect, Fortitude, Preservation},
		Currency, ExistenceRequirement, Get, WithdrawReasons,
	},
	weights::{Weight, WeightToFee},
};
use frame_system::{pallet_prelude::OriginFor, CheckWeight};
use hp_cosmos::{Account, Msg};
use sp_core::{ecdsa, H160};
use sp_runtime::{
	traits::{BadOrigin, Convert, DispatchInfoOf, Dispatchable, UniqueSaturatedInto},
	transaction_validity::{
		InvalidTransaction, TransactionValidity, TransactionValidityError, ValidTransactionBuilder,
	},
	RuntimeDebug,
};
use sp_std::{marker::PhantomData, vec::Vec};
pub use weights::*;

#[derive(Clone, Eq, PartialEq, RuntimeDebug, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub enum RawOrigin {
	CosmosTransaction(H160),
}

pub fn ensure_cosmos_transaction<OuterOrigin>(o: OuterOrigin) -> Result<H160, &'static str>
where
	OuterOrigin: Into<Result<RawOrigin, OuterOrigin>>,
{
	match o.into() {
		Ok(RawOrigin::CosmosTransaction(n)) => Ok(n),
		_ => Err("bad origin: expected to be an Cosmos transaction"),
	}
}

impl<T> Call<T>
where
	OriginFor<T>: Into<Result<RawOrigin, OriginFor<T>>>,
	T: Send + Sync + Config,
	T::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
{
	pub fn is_self_contained(&self) -> bool {
		matches!(self, Call::transact { .. })
	}

	pub fn check_self_contained(&self) -> Option<Result<ecdsa::Public, TransactionValidityError>> {
		if let Call::transact { tx } = self {
			let check = || {
				if let Some(hp_cosmos::SignerPublicKey::Single(hp_cosmos::PublicKey::Secp256k1(
					pk,
				))) = tx.auth_info.signer_infos[0].public_key
				{
					let sig = &tx.signatures[0];
					let msg = tx.hash.as_bytes();
					if hp_io::crypto::secp256k1_ecdsa_verify(sig, msg, &pk) {
						Ok(ecdsa::Public::from_raw(pk))
					} else {
						Err(InvalidTransaction::Custom(
							hp_cosmos::error::TransactionValidationError::InvalidSignature as u8,
						))?
					}
				} else {
					Err(InvalidTransaction::Custom(
						hp_cosmos::error::TransactionValidationError::UnsupportedSignerType as u8,
					))?
				}
			};

			Some(check())
		} else {
			None
		}
	}

	pub fn pre_dispatch_self_contained(
		&self,
		origin: &H160,
		dispatch_info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Option<Result<(), TransactionValidityError>> {
		if let Call::transact { tx } = self {
			if let Err(e) = CheckWeight::<T>::do_pre_dispatch(dispatch_info, len) {
				return Some(Err(e))
			}

			Some(Pallet::<T>::validate_transaction_in_block(*origin, tx))
		} else {
			None
		}
	}

	pub fn validate_self_contained(
		&self,
		origin: &H160,
		dispatch_info: &DispatchInfoOf<T::RuntimeCall>,
		len: usize,
	) -> Option<TransactionValidity> {
		if let Call::transact { tx } = self {
			if let Err(e) = CheckWeight::<T>::do_validate(dispatch_info, len) {
				return Some(Err(e))
			}

			Some(Pallet::<T>::validate_transaction_in_pool(*origin, tx))
		} else {
			None
		}
	}
}

pub trait AddressMapping<A> {
	fn into_account_id(address: H160) -> A;
}

pub trait EnsureAddressOrigin<OuterOrigin> {
	/// Success return type.
	type Success;

	/// Perform the origin check.
	fn ensure_address_origin(
		address: &H160,
		origin: OuterOrigin,
	) -> Result<Self::Success, BadOrigin> {
		Self::try_address_origin(address, origin).map_err(|_| BadOrigin)
	}

	/// Try with origin.
	fn try_address_origin(
		address: &H160,
		origin: OuterOrigin,
	) -> Result<Self::Success, OuterOrigin>;
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::origin]
	pub type Origin = RawOrigin;

	/// Type alias for currency balance.
	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Mapping from address to account id.
		type AddressMapping: AddressMapping<Self::AccountId>;
		/// Currency type for withdraw and balance storage.
		type Currency: Currency<Self::AccountId> + Inspect<Self::AccountId>;
		/// Handle cosmos messages.
		type MsgHandler: MsgHandler<Self>;
		/// Convert a length value into a deductible fee based on the currency type.
		type LengthToFee: WeightToFee<Balance = BalanceOf<Self>>;
		/// The overarching event type.
		type RuntimeEvent: From<Event> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
		/// Used to answer contracts' queries regarding the current weight price. This is **not**
		/// used to calculate the actual fee and is only for informational purposes.
		type WeightPrice: Convert<Weight, BalanceOf<Self>>;
		/// Convert a weight value into a deductible fee based on the currency type.
		type WeightToFee: WeightToFee<Balance = BalanceOf<Self>>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event {
		Executed { code: u32, gas_used: Weight, messages: Vec<Msg> },
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidTx,
		Unauthorized,
		InsufficientFunds,
		OutOfGas,
		InsufficientFee,
		InvalidType,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		OriginFor<T>: Into<Result<RawOrigin, OriginFor<T>>>,
	{
		/// Transact an Cosmos transaction.
		#[pallet::call_index(0)]
		#[pallet::weight(tx.auth_info.fee.gas_limit)]
		pub fn transact(origin: OriginFor<T>, tx: hp_cosmos::Tx) -> DispatchResultWithPostInfo {
			let source = ensure_cosmos_transaction(origin)?;
			if !tx.is_valid() {
				return Err(DispatchErrorWithPostInfo {
					post_info: Default::default(),
					error: Error::<T>::InvalidTx.into(),
				})
			}
			Self::apply_validated_transaction(source, tx)
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Validate an Cosmos transaction already in block
	///
	/// This function must be called during the pre-dispatch phase
	/// (just before applying the extrinsic).
	pub fn validate_transaction_in_block(
		origin: H160,
		tx: &hp_cosmos::Tx,
	) -> Result<(), TransactionValidityError> {
		let (who, _) = Self::account(&origin);
		if tx.auth_info.signer_infos[0].sequence < who.sequence {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Stale))
		} else if tx.auth_info.signer_infos[0].sequence > who.sequence {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Future))
		}

		let mut total_payment = 0u128;
		total_payment = total_payment.saturating_add(tx.auth_info.fee.amount[0].amount);
		match &tx.body.messages[0] {
			Msg::MsgSend { amount, .. } => {
				total_payment = total_payment.saturating_add(amount[0].amount);
			},
		}
		if total_payment > who.amount {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Payment))
		}

		Ok(())
	}

	// Controls that must be performed by the pool.
	fn validate_transaction_in_pool(origin: H160, tx: &hp_cosmos::Tx) -> TransactionValidity {
		let (who, _) = Self::account(&origin);
		let transaction_nonce = tx.auth_info.signer_infos[0].sequence;

		if transaction_nonce < who.sequence {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Stale))
		}
		let mut total_payment = 0u128;
		total_payment = total_payment.saturating_add(tx.auth_info.fee.amount[0].amount);
		match &tx.body.messages[0] {
			Msg::MsgSend { amount, .. } => {
				total_payment = total_payment.saturating_add(amount[0].amount);
			},
		}
		if total_payment > who.amount {
			return Err(TransactionValidityError::Invalid(InvalidTransaction::Payment))
		}

		let mut builder =
			ValidTransactionBuilder::default().and_provides((origin, transaction_nonce));

		// In the context of the pool, a transaction with
		// too high a nonce is still considered valid
		if transaction_nonce > who.sequence {
			if let Some(prev_nonce) = transaction_nonce.checked_sub(1) {
				builder = builder.and_requires((origin, prev_nonce))
			}
		}

		builder.build()
	}

	fn apply_validated_transaction(source: H160, tx: hp_cosmos::Tx) -> DispatchResultWithPostInfo {
		sp_io::storage::start_transaction();
		match Self::execute(&source, &tx) {
			Ok(weight) => {
				Self::deposit_event(Event::Executed {
					code: 0u32,
					gas_used: weight,
					messages: tx.body.messages.clone(),
				});
				sp_io::storage::commit_transaction();
				Ok(PostDispatchInfo { actual_weight: Some(weight), pays_fee: Pays::No })
			},
			Err(e) => {
				sp_io::storage::rollback_transaction();
				Self::deposit_event(Event::Executed {
					code: e.error as u32,
					gas_used: e.weight,
					messages: tx.body.messages.clone(),
				});
				let origin = T::AddressMapping::into_account_id(source);
				let fee = Self::compute_fee(tx.len, e.weight);
				if let Ok(_) = T::Currency::withdraw(
					&origin,
					fee,
					WithdrawReasons::FEE,
					ExistenceRequirement::AllowDeath,
				) {
					Ok(PostDispatchInfo { actual_weight: Some(e.weight), pays_fee: Pays::No })
				} else {
					Err(DispatchErrorWithPostInfo {
						post_info: PostDispatchInfo {
							actual_weight: Some(e.weight),
							pays_fee: Pays::No,
						},
						error: Error::<T>::InsufficientFee.into(),
					})
				}
			},
		}
	}

	fn execute(source: &H160, tx: &hp_cosmos::Tx) -> Result<Weight, CosmosError> {
		let mut total_weight = Weight::zero();
		total_weight = total_weight
			.saturating_add(T::BlockWeights::get().get(DispatchClass::Normal).base_extrinsic);
		match &tx.body.messages[0] {
			hp_cosmos::Msg::MsgSend { from_address, to_address, amount } => {
				if *source != *from_address {
					return Err(CosmosError {
						weight: total_weight,
						error: CosmosErrorCode::ErrUnauthorized,
					})
				}
				let weight = T::MsgHandler::msg_send(from_address, to_address, amount[0].amount)
					.map_err(|e| CosmosError {
						weight: total_weight.saturating_add(e.weight),
						error: e.error,
					})?;
				total_weight = total_weight.saturating_add(weight);
			},
		};

		// Includes weights of finding origin, increment account nonce, withdraw fee.
		total_weight = total_weight.saturating_add(
			T::DbWeight::get().reads(3).saturating_add(T::DbWeight::get().writes(2)),
		);
		let fee = Self::compute_fee(tx.len, total_weight);
		let maximum_fee = tx.auth_info.fee.amount[0].amount.unique_saturated_into();
		if fee > maximum_fee {
			return Err(CosmosError {
				weight: total_weight,
				error: CosmosErrorCode::ErrInsufficientFee,
			})
		}
		let origin = T::AddressMapping::into_account_id(*source);
		T::Currency::withdraw(&origin, fee, WithdrawReasons::FEE, ExistenceRequirement::AllowDeath)
			.map_err(|_| CosmosError {
				weight: total_weight,
				error: CosmosErrorCode::ErrInsufficientFee,
			})?;
		frame_system::Pallet::<T>::inc_account_nonce(origin);
		Ok(total_weight)
	}

	/// Get the base account info.
	pub fn account(address: &H160) -> (Account, frame_support::weights::Weight) {
		let account_id = T::AddressMapping::into_account_id(*address);

		let nonce = frame_system::Pallet::<T>::account_nonce(&account_id);
		// keepalive `true` takes into account ExistentialDeposit as part of what's considered
		// liquid balance.
		let balance =
			T::Currency::reducible_balance(&account_id, Preservation::Preserve, Fortitude::Polite);

		(
			Account {
				sequence: UniqueSaturatedInto::<u64>::unique_saturated_into(nonce),
				amount: UniqueSaturatedInto::<u128>::unique_saturated_into(balance),
			},
			T::DbWeight::get().reads(2),
		)
	}

	fn compute_fee(len: u32, weight: Weight) -> BalanceOf<T> {
		// Base fee is already included.
		let adjusted_weight_fee = T::WeightPrice::convert(weight);
		let length_fee = Self::length_to_fee(len);
		let inclusion_fee = length_fee + adjusted_weight_fee;
		inclusion_fee
	}

	/// Compute the length portion of a fee by invoking the configured `LengthToFee` impl.
	pub fn length_to_fee(length: u32) -> BalanceOf<T> {
		T::LengthToFee::weight_to_fee(&Weight::from_parts(length as u64, 0))
	}

	/// Compute the unadjusted portion of the weight fee by invoking the configured `WeightToFee`
	/// impl. Note that the input `weight` is capped by the maximum block weight before computation.
	pub fn weight_to_fee(weight: Weight) -> BalanceOf<T> {
		// cap the weight to the maximum defined in runtime, otherwise it will be the
		// `Bounded` maximum of its data type, which is not desired.
		let capped_weight = weight.min(T::BlockWeights::get().max_block);
		T::WeightToFee::weight_to_fee(&capped_weight)
	}
}
