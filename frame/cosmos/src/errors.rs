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

use frame_support::weights::Weight;

#[derive(Clone)]
pub struct CosmosError {
	pub weight: Weight,
	pub error: CosmosErrorCode,
}

#[derive(Clone, Copy)]
pub enum CosmosErrorCode {
	// ErrUnauthorized is used whenever a request without sufficient
	// authorization is handled.
	ErrUnauthorized = 1,
	// ErrInsufficientFunds is used when the account cannot pay requested amount.
	ErrInsufficientFunds = 2,
	// ErrOutOfGas to doc
	ErrOutOfGas = 3,
	// ErrInsufficientFee to doc
	ErrInsufficientFee = 4,
	// ErrInvalidType defines an error an invalid type.
	ErrInvalidType = 5,
	// ErrDenomMetadataNotFound
	ErrDenomMetadataNotFound = 6,
}
