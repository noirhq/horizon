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

use frame_support::{dispatch::DispatchClass, weights::Weight};
use sp_core::Get;
use sp_std::marker::PhantomData;

pub trait WeightInfo {
	fn default_weight() -> Weight;
}

pub struct CosmosWeight<T>(PhantomData<T>);
impl<T> WeightInfo for CosmosWeight<T>
where
	T: frame_system::Config,
{
	fn default_weight() -> Weight {
		T::BlockWeights::get().get(DispatchClass::Normal).base_extrinsic
	}
}
