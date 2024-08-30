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

extern crate alloc;

pub mod weights;

pub use self::pallet::*;
use crate::weights::WeightInfo;
use alloc::{string::String, vec::Vec};
use core::marker::PhantomData;
use cosmos_sdk_proto::{cosmos::tx::v1beta1::Tx, prost::Message, Any};
use frame_support::{
	dispatch::{DispatchErrorWithPostInfo, DispatchInfo, PostDispatchInfo},
	pallet_prelude::{DispatchResultWithPostInfo, InvalidTransaction, Pays},
	traits::{
		tokens::{fungible::Inspect, fungibles, AssetId, Balance, Fortitude, Preservation},
		Currency, Get,
	},
	weights::Weight,
};
use frame_system::{pallet_prelude::OriginFor, CheckWeight};
use pallet_cosmos_types::{
	address::address_from_bech32,
	errors::{CosmosError, RootError},
	events,
	events::{CosmosEvent, CosmosEvents, EventManager as _},
	handler::AnteDecorator,
	msgservice::MsgServiceRouter,
	store::{self, BasicGasMeter, Context, Gas, GasMeter},
	tx::Account,
};
use pallet_cosmos_x_auth_signing::{
	sign_mode_handler::SignModeHandler, sign_verifiable_tx::SigVerifiableTx,
};
use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_core::H160;
use sp_runtime::{
	traits::{Convert, DispatchInfoOf, Dispatchable, UniqueSaturatedInto},
	transaction_validity::{
		TransactionValidity, TransactionValidityError, ValidTransactionBuilder,
	},
	RuntimeDebug,
};

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
		_ => Err("bad origin: expected to be a Cosmos transaction"),
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
		if let Call::transact { tx_bytes } = self {
			let check = || {
				let tx = Tx::decode(&mut &tx_bytes[..])
					.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Call))?;
				let fee_payer = T::SigVerifiableTx::fee_payer(&tx).map_err(|_| {
					TransactionValidityError::Invalid(InvalidTransaction::BadSigner)
				})?;

				address_from_bech32(&fee_payer)
					.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::BadSigner))
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
		if let Call::transact { tx_bytes } = self {
			if let Err(e) = CheckWeight::<T>::do_pre_dispatch(dispatch_info, len) {
				return Some(Err(e));
			}

			Some(Pallet::<T>::validate_transaction_in_block(*origin, tx_bytes))
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
		if let Call::transact { tx_bytes } = self {
			if let Err(e) = CheckWeight::<T>::do_validate(dispatch_info, len) {
				return Some(Err(e));
			}

			Some(Pallet::<T>::validate_transaction_in_pool(*origin, tx_bytes))
		} else {
			None
		}
	}
}

pub trait AddressMapping<A> {
	fn from_address_raw(address: H160) -> A;
	fn from_bech32(address: &str) -> Option<A>;
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{fungibles::metadata::Inspect as _, Contains},
	};

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::origin]
	pub type Origin = RawOrigin;

	/// Default implementations of [`DefaultConfig`], which can be used to implement [`Config`].
	pub mod config_preludes {
		use super::*;
		use frame_support::{derive_impl, parameter_types};

		pub struct TestDefaultConfig;

		#[derive_impl(frame_system::config_preludes::TestDefaultConfig as frame_system::DefaultConfig, no_aggregated_types)]
		impl frame_system::DefaultConfig for TestDefaultConfig {}

		pub struct MsgFilter;
		impl Contains<Any> for MsgFilter {
			fn contains(_msg: &Any) -> bool {
				false
			}
		}

		pub struct GasToWeight;
		impl Convert<Gas, Weight> for GasToWeight {
			fn convert(gas: Gas) -> Weight {
				Weight::from_parts(gas, 0u64)
			}
		}

		pub struct WeightToGas;
		impl Convert<Weight, Gas> for WeightToGas {
			fn convert(weight: Weight) -> Gas {
				weight.ref_time()
			}
		}

		parameter_types! {
			pub const MaxMemoCharacters: u64 = 256;
			pub NativeDenom: &'static str = "acdt";
			pub ChainId: &'static str = "dev";
			pub const TxSigLimit: u64 = 7;
			pub const MaxDenomLimit: u32 = 128;
			pub const AddressPrefix: &'static str = "cosmos";
		}

		#[derive(Clone, Debug, Default)]
		pub struct EventManager {
			events: CosmosEvents,
		}

		impl events::EventManager for EventManager {
			fn new() -> Self {
				Self::default()
			}

			fn events(&self) -> CosmosEvents {
				self.events.clone()
			}

			fn emit_event(&mut self, event: events::CosmosEvent) {
				self.events.push(event);
			}

			fn emit_events(&mut self, events: CosmosEvents) {
				self.events.extend(events);
			}
		}

		pub struct Context {
			pub gas_meter: BasicGasMeter,
			pub event_manager: EventManager,
		}

		impl store::Context for Context {
			type GasMeter = BasicGasMeter;
			type EventManager = EventManager;

			fn new(limit: Gas) -> Self {
				Self {
					gas_meter: Self::GasMeter::new(limit),
					event_manager: Self::EventManager::new(),
				}
			}

			fn gas_meter(&mut self) -> &mut Self::GasMeter {
				&mut self.gas_meter
			}

			fn event_manager(&mut self) -> &mut Self::EventManager {
				&mut self.event_manager
			}
		}

		#[frame_support::register_default_impl(TestDefaultConfig)]
		impl DefaultConfig for TestDefaultConfig {
			#[inject_runtime_type]
			type RuntimeEvent = ();
			type AnteHandler = ();
			type Balance = u64;
			type AssetId = u32;
			type MaxMemoCharacters = MaxMemoCharacters;
			type NativeDenom = NativeDenom;
			type ChainId = ChainId;
			type MsgFilter = MsgFilter;
			type GasToWeight = GasToWeight;
			type WeightToGas = WeightToGas;
			type TxSigLimit = TxSigLimit;
			type MaxDenomLimit = MaxDenomLimit;
			type AddressPrefix = AddressPrefix;
			type Context = Context;
		}
	}

	#[pallet::storage]
	#[pallet::getter(fn denom_to_asset)]
	pub type DenomAssetRouter<T: Config> =
		StorageMap<_, Twox64Concat, BoundedVec<u8, T::MaxDenomLimit>, T::AssetId, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn asset_to_denom)]
	pub type AssetDenomRouter<T: Config> =
		StorageMap<_, Twox64Concat, T::AssetId, BoundedVec<u8, T::MaxDenomLimit>, OptionQuery>;

	#[pallet::config(with_default)]
	pub trait Config: frame_system::Config {
		/// Mapping an address to an account id.
		#[pallet::no_default]
		type AddressMapping: AddressMapping<Self::AccountId>;
		/// Native asset type.
		#[pallet::no_default]
		type NativeAsset: Currency<Self::AccountId> + Inspect<Self::AccountId>;
		/// Type of an account balance.
		type Balance: Balance + Into<u128>;
		/// Type of a tradable asset id.
		/// The [`Ord`] constraint is required for [`BoundedBTreeMap`].
		type AssetId: AssetId + Ord + MaybeSerializeDeserialize;
		/// Interface from which we are going to execute assets operations.
		#[pallet::no_default]
		type Assets: fungibles::Inspect<Self::AccountId, Balance = Self::Balance, AssetId = Self::AssetId>
			+ fungibles::metadata::Inspect<
				Self::AccountId,
				Balance = Self::Balance,
				AssetId = Self::AssetId,
			> + fungibles::Mutate<Self::AccountId, Balance = Self::Balance, AssetId = Self::AssetId>
			+ fungibles::Balanced<Self::AccountId, Balance = Self::Balance, AssetId = Self::AssetId>;
		/// The overarching event type.
		#[pallet::no_default_bounds]
		type RuntimeEvent: From<Event> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// Verify the validity of a Cosmos transaction.
		type AnteHandler: AnteDecorator;
		/// The maximum number of characters allowed in a memo.
		#[pallet::constant]
		type MaxMemoCharacters: Get<u64>;
		/// The native denomination for the currency.
		#[pallet::constant]
		type NativeDenom: Get<&'static str>;
		/// Router for handling message services.
		#[pallet::no_default]
		type MsgServiceRouter: MsgServiceRouter<Self::Context>;
		/// The chain ID.
		#[pallet::constant]
		type ChainId: Get<&'static str>;
		/// The message filter.
		type MsgFilter: Contains<Any>;
		/// Converter for converting Gas to Weight.
		type GasToWeight: Convert<Gas, Weight>;
		/// Converter for converting Weight to Gas.
		type WeightToGas: Convert<Weight, Gas>;
		/// The maximum number of transaction signatures allowed.
		#[pallet::constant]
		type TxSigLimit: Get<u64>;
		/// Defines the features for all signature verification handlers.
		#[pallet::no_default]
		type SigVerifiableTx: SigVerifiableTx;
		/// Handler for managing different signature modes in transactions.
		#[pallet::no_default]
		type SignModeHandler: SignModeHandler;
		#[pallet::no_default]
		type WeightInfo: WeightInfo;
		/// A way to convert from cosmos coin denom to asset id.
		#[pallet::no_default]
		type AssetToDenom: Convert<String, Result<Self::AssetId, ()>>
			+ Convert<Self::AssetId, String>;
		#[pallet::constant]
		type MaxDenomLimit: Get<u32>;
		/// The chain ID.
		#[pallet::constant]
		type AddressPrefix: Get<&'static str>;

		type Context: store::Context;
	}

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		pub assets: Vec<(Vec<u8>, T::AssetId)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			for (symbol, asset_id) in &self.assets {
				let denom = BoundedVec::<u8, T::MaxDenomLimit>::try_from(symbol.clone())
					.expect("Invalid denom");
				assert!(DenomAssetRouter::<T>::get(denom.clone()).is_none());
				assert!(*symbol == T::Assets::symbol(asset_id.clone()));

				DenomAssetRouter::<T>::insert(denom, asset_id);
			}
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub fn deposit_event)]
	pub enum Event {
		AnteHandled(Vec<CosmosEvent>),
		Executed { gas_wanted: u64, gas_used: u64, events: Vec<CosmosEvent> },
	}

	#[pallet::error]
	pub enum Error<T> {
		CosmosError(CosmosError),
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		OriginFor<T>: Into<Result<RawOrigin, OriginFor<T>>>,
	{
		/// Transact a Cosmos transaction.
		#[pallet::call_index(0)]
		#[pallet::weight({
			use cosmos_sdk_proto::traits::Message;
			use cosmos_sdk_proto::cosmos::tx::v1beta1::Tx;

			Tx::decode(&mut &tx_bytes[..])
				.ok()
				.and_then(|tx| tx.auth_info)
				.and_then(|auth_info| auth_info.fee)
				.map_or(T::WeightInfo::default_weight(), |fee| {
					T::GasToWeight::convert(fee.gas_limit)
				})
		 })]
		pub fn transact(origin: OriginFor<T>, tx_bytes: Vec<u8>) -> DispatchResultWithPostInfo {
			let source = ensure_cosmos_transaction(origin)?;

			let tx = Tx::decode(&mut &*tx_bytes).map_err(|_| DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(T::WeightInfo::default_weight()),
					pays_fee: Pays::Yes,
				},
				error: Error::<T>::CosmosError(RootError::TxDecodeError.into()).into(),
			})?;

			Self::apply_validated_transaction(source, tx)
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Validate a Cosmos transaction already in block
	///
	/// This function must be called during the pre-dispatch phase
	/// (just before applying the extrinsic).
	pub fn validate_transaction_in_block(
		origin: H160,
		tx_bytes: &[u8],
	) -> Result<(), TransactionValidityError> {
		let (_who, _) = Self::account(&origin);
		let tx = Tx::decode(&mut &*tx_bytes)
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Call))?;

		T::AnteHandler::ante_handle(&tx, false)?;

		Ok(())
	}

	// Controls that must be performed by the pool.
	fn validate_transaction_in_pool(origin: H160, tx_bytes: &[u8]) -> TransactionValidity {
		let (who, _) = Self::account(&origin);
		let tx = Tx::decode(&mut &*tx_bytes)
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Call))?;

		T::AnteHandler::ante_handle(&tx, true)?;

		let transaction_nonce = T::SigVerifiableTx::sequence(&tx)
			.map_err(|_| TransactionValidityError::Invalid(InvalidTransaction::Call))?;

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

	pub fn apply_validated_transaction(_source: H160, tx: Tx) -> DispatchResultWithPostInfo {
		let body = tx.body.ok_or(DispatchErrorWithPostInfo {
			post_info: PostDispatchInfo {
				actual_weight: Some(T::WeightInfo::default_weight()),
				pays_fee: Pays::Yes,
			},
			error: Error::<T>::CosmosError(RootError::TxDecodeError.into()).into(),
		})?;
		let gas_limit = tx
			.auth_info
			.as_ref()
			.and_then(|auth_info| auth_info.fee.as_ref())
			.ok_or(DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(T::WeightInfo::default_weight()),
					pays_fee: Pays::Yes,
				},
				error: Error::<T>::CosmosError(RootError::TxDecodeError.into()).into(),
			})?
			.gas_limit;

		let mut ctx = T::Context::new(gas_limit);
		ctx.gas_meter()
			.consume_gas(T::WeightInfo::default_weight().ref_time(), "")
			.map_err(|_| DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(Weight::from_parts(ctx.gas_meter().consumed_gas(), 0)),
					pays_fee: Pays::Yes,
				},
				error: Error::<T>::CosmosError(RootError::OutOfGas.into()).into(),
			})?;

		for msg in body.messages.iter() {
			let handler = T::MsgServiceRouter::route(msg).ok_or(DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(Weight::from_parts(ctx.gas_meter().consumed_gas(), 0)),
					pays_fee: Pays::Yes,
				},
				error: Error::<T>::CosmosError(RootError::UnknownRequest.into()).into(),
			})?;

			handler.handle(msg, &mut ctx).map_err(|e| DispatchErrorWithPostInfo {
				post_info: PostDispatchInfo {
					actual_weight: Some(Weight::from_parts(ctx.gas_meter().consumed_gas(), 0)),
					pays_fee: Pays::Yes,
				},
				error: Error::<T>::CosmosError(e).into(),
			})?;
		}

		Self::deposit_event(Event::Executed {
			gas_wanted: gas_limit,
			gas_used: T::WeightToGas::convert(Weight::from_parts(
				ctx.gas_meter().consumed_gas(),
				0,
			)),
			events: ctx.event_manager().events(),
		});

		Ok(PostDispatchInfo {
			actual_weight: Some(Weight::from_parts(ctx.gas_meter().consumed_gas(), 0)),
			pays_fee: Pays::Yes,
		})
	}

	/// Get the base account info.
	pub fn account(address: &H160) -> (Account, Weight) {
		let account_id = T::AddressMapping::from_address_raw(*address);

		let nonce = frame_system::Pallet::<T>::account_nonce(&account_id);
		// keepalive `true` takes into account ExistentialDeposit as part of what's considered
		// liquid balance.
		let balance = T::NativeAsset::reducible_balance(
			&account_id,
			Preservation::Preserve,
			Fortitude::Polite,
		);

		(
			Account {
				sequence: UniqueSaturatedInto::<u64>::unique_saturated_into(nonce),
				amount: UniqueSaturatedInto::<u128>::unique_saturated_into(balance),
			},
			T::DbWeight::get().reads(2),
		)
	}
}
