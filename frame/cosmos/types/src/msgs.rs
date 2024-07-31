// This file is part of Horizon.

// Copyright (C) 2023 Haderech Pte. Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{error::DecodeError, legacy::LegacyMsg};
use cosmos_sdk_proto::{prost::alloc::string::String, Any};

pub trait Msg {
	const TYPE_URL: &'static [u8];
	const AMINO_NAME: &'static [u8];

	fn get_signers(&self) -> sp_std::vec::Vec<String>;
	fn legacy_msg(any: Any) -> Result<LegacyMsg, DecodeError>;
}
