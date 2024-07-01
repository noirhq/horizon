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

pub mod errors;
pub mod weights;

pub use self::pallet::*;
use frame_support::{
	dispatch::{DispatchInfo, PostDispatchInfo},
	pallet_prelude::{DispatchResultWithPostInfo, Pays},
	traits::{
		tokens::{fungible::Inspect, Fortitude, Preservation},
		Currency, Get,
	},
	weights::{Weight, WeightToFee},
};
use frame_system::{pallet_prelude::OriginFor, CheckWeight};
use hp_cosmos::{Account, PublicKey, SignerPublicKey};
use hp_io::crypto::ripemd160;
use pallet_cosmos_modules::{AnteHandler, MsgServiceRouter};
use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_core::H160;
use sp_io::hashing::sha2_256;
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

	pub fn check_self_contained(&self) -> Option<Result<H160, TransactionValidityError>> {
		if let Call::transact { tx_bytes, chain_id } = self {
			let tx = hp_io::decode_tx::decode(tx_bytes, chain_id)?;

			if let Err(e) = T::AnteHandler::handle(&tx) {
				return Some(Err(e));
			}

			if let Some(signer) = tx.auth_info.signer_infos.first() {
				if let Some(SignerPublicKey::Single(PublicKey::Secp256k1(public_key))) =
					signer.public_key
				{
					let address = ripemd160(&sha2_256(&public_key)).into();
					Some(Ok(address))
				} else {
					Some(Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)))
				}
			} else {
				Some(Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)))
			}
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
		if let Call::transact { tx_bytes, chain_id } = self {
			if let Err(e) = CheckWeight::<T>::do_pre_dispatch(dispatch_info, len) {
				return Some(Err(e));
			}

			Some(Pallet::<T>::validate_transaction_in_block(*origin, tx_bytes, chain_id))
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
		if let Call::transact { tx_bytes, chain_id } = self {
			if let Err(e) = CheckWeight::<T>::do_validate(dispatch_info, len) {
				return Some(Err(e));
			}

			Some(Pallet::<T>::validate_transaction_in_pool(*origin, tx_bytes, chain_id))
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
	use hp_cosmos::Any;
	use pallet_cosmos_modules::{AnteHandler, MsgServiceRouter};

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
		/// Verify the validity of a Cosmos transaction.
		type AnteHandler: AnteHandler;
		/// The maximum size of the memo.
		#[pallet::constant]
		type MaxMemoCharacters: Get<u64>;

		#[pallet::constant]
		type NativeDenom: Get<BoundedVec<u8, Self::DenomMaxLen>>;

		#[pallet::constant]
		type DenomMaxLen: Get<u32>;

		type MsgServiceRouter: MsgServiceRouter<Self>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event {
		Executed { code: u32, gas_used: Weight, messages: Vec<Any> },
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
		#[pallet::weight({ 0 })]
		pub fn transact(
			origin: OriginFor<T>,
			tx_bytes: Vec<u8>,
			chain_id: Vec<u8>,
		) -> DispatchResultWithPostInfo {
			let source = ensure_cosmos_transaction(origin)?;
			let tx = hp_io::decode_tx::decode(&tx_bytes, &chain_id).unwrap();

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
		tx_bytes: &[u8],
		chain_id: &[u8],
	) -> Result<(), TransactionValidityError> {
		let (_who, _) = Self::account(&origin);
		let tx = hp_io::decode_tx::decode(tx_bytes, chain_id)
			.ok_or(TransactionValidityError::Invalid(InvalidTransaction::Call))?;

		T::AnteHandler::handle(&tx)?;

		Ok(())
	}

	// Controls that must be performed by the pool.
	fn validate_transaction_in_pool(
		origin: H160,
		tx_bytes: &[u8],
		chain_id: &[u8],
	) -> TransactionValidity {
		let (who, _) = Self::account(&origin);
		let tx = hp_io::decode_tx::decode(tx_bytes, chain_id)
			.ok_or(TransactionValidityError::Invalid(InvalidTransaction::Call))?;

		T::AnteHandler::handle(&tx)?;

		let transaction_nonce = tx
			.auth_info
			.signer_infos
			.first()
			.ok_or(TransactionValidityError::Invalid(InvalidTransaction::Call))?
			.sequence;

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
		let mut total_weight = Weight::zero();
		for msg in tx.body.messages.iter() {
			let result = T::MsgServiceRouter::route(&msg.type_url, &msg.value);
		}
		Ok(PostDispatchInfo { actual_weight: Some(Weight::zero()), pays_fee: Pays::Yes })
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
		length_fee + adjusted_weight_fee
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
