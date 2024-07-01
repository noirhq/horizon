// This file is part of Hrozion.

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

use frame_support::weights::Weight;
use hp_cosmos::msgs::MsgSend;
use parity_scale_codec::Decode;
use sp_std::marker::PhantomData;

pub struct MsgServiceRouter<T>(PhantomData<T>);
impl<T> pallet_cosmos_modules::MsgServiceRouter<T> for MsgServiceRouter<T>
where
	T: frame_system::Config + pallet_cosmos::Config,
{
	type Error = ();

	// TODO: Register handler

	fn route(type_url: &[u8], value: &[u8]) -> Result<Weight, Self::Error> {
		match core::str::from_utf8(type_url).map_err(|_| ())? {
			"/cosmos.bank.v1beta1.MsgSend" => {
				let (_, value) = hp_io::protobuf_to_scale::to_scale(type_url, value).unwrap();
				let msg: MsgSend = Decode::decode(&mut &value[..]).unwrap();
				pallet_cosmos_bank::msgs::MsgServer::<T>::send(msg)
			},
			_ => Err(()),
		}
	}
}
