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

use frame_support::{
	pallet_prelude::*,
	traits::{Currency, ExistenceRequirement},
};
use hp_cosmos::{msgs::MsgSend, Any};
use pallet_cosmos::AddressMapping;
use sp_runtime::SaturatedConversion;

pub struct MsgSendHandler<T>(PhantomData<T>);

impl<T> Default for MsgSendHandler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T> pallet_cosmos_modules::MsgHandler for MsgSendHandler<T>
where
	T: pallet_cosmos::Config,
{
	type Error = ();

	fn handle(&self, msg: &Any) -> Result<Weight, Self::Error> {
		let (_, value) = hp_io::protobuf_to_scale::to_scale(&msg.type_url, &msg.type_url).unwrap();
		let MsgSend { from_address, to_address, amount } = Decode::decode(&mut &value[..]).unwrap();

		let from = T::AddressMapping::into_account_id(from_address.address);
		let to = T::AddressMapping::into_account_id(to_address.address);

		for amt in amount.iter() {
			if T::NativeDenom::get() == amt.denom {
				T::Currency::transfer(
					&from,
					&to,
					amt.amount.saturated_into(),
					ExistenceRequirement::AllowDeath,
				)
				.map_err(|_| ())?;
			} else {
				// TODO: Asset support planned
				return Err(());
			}
		}
		Ok(Weight::zero())
	}
}
