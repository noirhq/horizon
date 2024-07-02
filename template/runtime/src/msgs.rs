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

use pallet_cosmos_bank::msgs::MsgSendHandler;
use pallet_cosmos_modules::msgs::MsgHandler;

pub struct MsgServiceRouter<T>(sp_std::marker::PhantomData<T>);
impl<T> pallet_cosmos_modules::msgs::MsgServiceRouter for MsgServiceRouter<T>
where
	T: frame_system::Config + pallet_cosmos::Config,
{
	fn route(type_url: &[u8]) -> Option<sp_std::boxed::Box<dyn MsgHandler>> {
		match core::str::from_utf8(type_url).unwrap() {
			"/cosmos.bank.v1beta1.MsgSend" =>
				Some(sp_std::boxed::Box::<MsgSendHandler<T>>::default()),
			_ => None,
		}
	}
}
