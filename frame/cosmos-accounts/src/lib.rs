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

pub mod weights;

use crate::weights::WeightInfo;
#[cfg(feature = "std")]
use frame_support::{sp_runtime, traits::BuildGenesisConfig};
use hp_crypto::EcdsaExt;
pub use pallet::*;
use sp_core::H160;
use sp_std::vec::Vec;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// An cosmos account connected.
		Connected { address: H160, who: T::AccountId },
	}

	#[pallet::error]
	pub enum Error<T> {
		DeriveFailed,
	}

	#[pallet::storage]
	pub type AccountOf<T: Config> = StorageMap<_, Blake2_128Concat, H160, T::AccountId>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub accounts: Vec<T::AccountId>,
	}

	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { accounts: Default::default() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T>
	where
		T::AccountId: EcdsaExt,
	{
		fn build(&self) {
			for account in self.accounts.iter() {
				let _ = Pallet::<T>::connect_account(account);
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		T::AccountId: EcdsaExt,
	{
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::connect())]
		pub fn connect(origin: OriginFor<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let _ = Self::connect_account(&who)?;
			Ok(())
		}
	}

	impl<T: Config> Pallet<T>
	where
		T::AccountId: EcdsaExt,
	{
		pub fn connect_account(who: &T::AccountId) -> Result<(), DispatchError> {
			let address = who.to_cosm_address().ok_or(Error::<T>::DeriveFailed)?;
			AccountOf::<T>::insert(&address, &who);
			Self::deposit_event(Event::<T>::Connected { address, who: who.clone() });
			Ok(())
		}
	}
}
