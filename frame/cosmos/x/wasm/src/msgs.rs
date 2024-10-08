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

use alloc::{string::ToString, vec, vec::Vec};
use core::{marker::PhantomData, str::FromStr};
use core2::io::Read;
use cosmos_sdk_proto::{
	cosmos::base::v1beta1::Coin,
	cosmwasm::wasm::v1::{
		MsgExecuteContract, MsgInstantiateContract2, MsgMigrateContract, MsgStoreCode,
		MsgUpdateAdmin,
	},
	prost::Message,
	Any,
};
use hp_crypto::EcdsaExt;
use libflate::gzip::Decoder;
use pallet_cosmos::AddressMapping;
use pallet_cosmos_types::{
	address::acc_address_from_bech32,
	context,
	errors::{CosmosError, RootError},
	events::{traits::EventManager, CosmosEvent, EventAttribute},
	gas::traits::GasMeter,
	msgservice::MsgHandler,
};
use pallet_cosmos_x_wasm_types::{
	errors::WasmError,
	events::{
		ATTRIBUTE_KEY_CHECKSUM, ATTRIBUTE_KEY_CODE_ID, ATTRIBUTE_KEY_CONTRACT_ADDR,
		ATTRIBUTE_KEY_NEW_ADMIN, EVENT_TYPE_EXECUTE, EVENT_TYPE_INSTANTIATE, EVENT_TYPE_MIGRATE,
		EVENT_TYPE_STORE_CODE, EVENT_TYPE_UPDATE_CONTRACT_ADMIN,
	},
};
use pallet_cosmwasm::{
	runtimes::vm::InitialStorageMutability,
	types::{
		CodeIdentifier, ContractCodeOf, ContractLabelOf, ContractMessageOf, ContractSaltOf, FundsOf,
	},
};
use sp_core::H160;
use sp_runtime::{traits::Convert, SaturatedConversion};

pub struct MsgStoreCodeHandler<T>(PhantomData<T>);

impl<T> Default for MsgStoreCodeHandler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T, Context> MsgHandler<Context> for MsgStoreCodeHandler<T>
where
	T: pallet_cosmos::Config + pallet_cosmwasm::Config,
	Context: context::traits::Context,
{
	fn handle(&self, msg: &Any, ctx: &mut Context) -> Result<(), CosmosError> {
		// TODO: Apply actual weights
		let MsgStoreCode { sender, wasm_byte_code, instantiate_permission: _ } =
			MsgStoreCode::decode(&mut &*msg.value).map_err(|_| RootError::TxDecodeError)?;

		let (_hrp, address_raw) =
			acc_address_from_bech32(&sender).map_err(|_| RootError::InvalidAddress)?;
		if address_raw.len() != 20 {
			return Err(RootError::InvalidAddress.into());
		}
		let who = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
		let mut decoder = Decoder::new(&wasm_byte_code[..]).map_err(|_| WasmError::CreateFailed)?;
		let mut decoded_code = Vec::new();
		decoder.read_to_end(&mut decoded_code).map_err(|_| WasmError::CreateFailed)?;

		let code: ContractCodeOf<T> =
			decoded_code.try_into().map_err(|_| WasmError::CreateFailed)?;

		let (code_hash, code_id) = pallet_cosmwasm::Pallet::<T>::do_upload(&who, code)
			.map_err(|_| WasmError::CreateFailed)?;

		// TODO: Same events emitted pallet_cosmos and pallet_cosmwasm
		let msg_event = CosmosEvent {
			r#type: EVENT_TYPE_STORE_CODE.into(),
			attributes: vec![
				EventAttribute {
					key: ATTRIBUTE_KEY_CODE_ID.into(),
					value: code_id.to_string().into(),
				},
				EventAttribute {
					key: ATTRIBUTE_KEY_CHECKSUM.into(),
					value: hex::encode(code_hash.0).into(),
				},
			],
		};

		ctx.event_manager().emit_event(msg_event);

		Ok(())
	}
}

pub struct MsgInstantiateContract2Handler<T>(PhantomData<T>);

impl<T> Default for MsgInstantiateContract2Handler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T, Context> MsgHandler<Context> for MsgInstantiateContract2Handler<T>
where
	T: pallet_cosmos::Config + pallet_cosmwasm::Config,
	T::AccountId: EcdsaExt,
	Context: context::traits::Context,
{
	// TODO: Consume gas
	fn handle(&self, msg: &Any, ctx: &mut Context) -> Result<(), CosmosError> {
		// TODO: Ignore fix_msg
		let MsgInstantiateContract2 { sender, admin, code_id, label, msg, funds, salt, fix_msg: _ } =
			MsgInstantiateContract2::decode(&mut &*msg.value)
				.map_err(|_| RootError::TxDecodeError)?;

		if sender.is_empty() {
			return Err(WasmError::Empty.into());
		}
		let (_hrp, address_raw) =
			acc_address_from_bech32(&sender).map_err(|_| RootError::InvalidAddress)?;
		if address_raw.len() != 20 {
			return Err(RootError::InvalidAddress.into());
		}
		let who = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
		let gas = ctx.gas_meter().gas_remaining();
		let mut shared = pallet_cosmwasm::Pallet::<T>::do_create_vm_shared(
			gas,
			InitialStorageMutability::ReadWrite,
		);
		let code_identifier = CodeIdentifier::CodeId(code_id);

		let admin_account = if !admin.is_empty() {
			let admin_account =
				T::AccountToAddr::convert(admin).map_err(|_| RootError::InvalidAddress)?;
			Some(admin_account)
		} else {
			None
		};

		let salt: ContractSaltOf<T> = salt.try_into().map_err(|_| RootError::TxDecodeError)?;
		let label: ContractLabelOf<T> =
			label.as_bytes().to_vec().try_into().map_err(|_| RootError::TxDecodeError)?;
		let funds = convert_funds::<T>(&funds)?;
		let message: ContractMessageOf<T> = msg.try_into().map_err(|_| RootError::TxDecodeError)?;

		let contract = pallet_cosmwasm::Pallet::<T>::do_instantiate(
			&mut shared,
			who,
			code_identifier,
			salt,
			admin_account,
			label,
			funds,
			message,
		)
		.map_err(|_| WasmError::InstantiateFailed)?;
		let contract = T::AccountToAddr::convert(contract);

		// TODO: Same events emitted pallet_cosmos and pallet_cosmwasm
		let msg_event = CosmosEvent {
			r#type: EVENT_TYPE_INSTANTIATE.into(),
			attributes: vec![
				EventAttribute { key: ATTRIBUTE_KEY_CONTRACT_ADDR.into(), value: contract.into() },
				EventAttribute {
					key: ATTRIBUTE_KEY_CODE_ID.into(),
					value: code_id.to_string().into(),
				},
			],
		};

		ctx.event_manager().emit_event(msg_event);

		Ok(())
	}
}

pub struct MsgExecuteContractHandler<T>(PhantomData<T>);

impl<T> Default for MsgExecuteContractHandler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T, Context> MsgHandler<Context> for MsgExecuteContractHandler<T>
where
	T: pallet_cosmos::Config + pallet_cosmwasm::Config,
	T::AccountId: EcdsaExt,
	Context: context::traits::Context,
{
	// TODO: Consume gas
	fn handle(&self, msg: &Any, ctx: &mut Context) -> Result<(), CosmosError> {
		let MsgExecuteContract { sender, contract, msg, funds } =
			MsgExecuteContract::decode(&mut &*msg.value).map_err(|_| RootError::TxDecodeError)?;

		if sender.is_empty() {
			return Err(WasmError::Empty.into());
		}
		let (_hrp, address_raw) =
			acc_address_from_bech32(&sender).map_err(|_| RootError::InvalidAddress)?;
		if address_raw.len() != 20 {
			return Err(RootError::InvalidAddress.into());
		}
		let who = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
		let gas = ctx.gas_meter().gas_remaining();
		let mut shared = pallet_cosmwasm::Pallet::<T>::do_create_vm_shared(
			gas,
			InitialStorageMutability::ReadWrite,
		);

		let contract_account =
			T::AccountToAddr::convert(contract.clone()).map_err(|_| RootError::TxDecodeError)?;
		let funds: FundsOf<T> = convert_funds::<T>(&funds)?;
		let message: ContractMessageOf<T> = msg.try_into().map_err(|_| RootError::TxDecodeError)?;

		pallet_cosmwasm::Pallet::<T>::do_execute(
			&mut shared,
			who,
			contract_account,
			funds,
			message,
		)
		.map_err(|_| WasmError::ExecuteFailed)?;

		// TODO: Same events emitted pallet_cosmos and pallet_cosmwasm
		let msg_event = CosmosEvent {
			r#type: EVENT_TYPE_EXECUTE.into(),
			attributes: vec![EventAttribute {
				key: ATTRIBUTE_KEY_CONTRACT_ADDR.into(),
				value: contract.into(),
			}],
		};

		ctx.event_manager().emit_event(msg_event);

		Ok(())
	}
}

pub struct MsgMigrateContractHandler<T>(PhantomData<T>);

impl<T> Default for MsgMigrateContractHandler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T, Context> MsgHandler<Context> for MsgMigrateContractHandler<T>
where
	T: pallet_cosmos::Config + pallet_cosmwasm::Config,
	Context: context::traits::Context,
{
	fn handle(&self, msg: &Any, ctx: &mut Context) -> Result<(), CosmosError> {
		let MsgMigrateContract { sender, contract, code_id, msg } =
			MsgMigrateContract::decode(&mut &*msg.value).map_err(|_| RootError::TxDecodeError)?;

		if sender.is_empty() {
			return Err(WasmError::Empty.into());
		}
		let (_hrp, address_raw) =
			acc_address_from_bech32(&sender).map_err(|_| RootError::InvalidAddress)?;
		if address_raw.len() != 20 {
			return Err(RootError::InvalidAddress.into());
		}
		let who = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
		let gas = ctx.gas_meter().gas_remaining();
		let mut shared = pallet_cosmwasm::Pallet::<T>::do_create_vm_shared(
			gas,
			InitialStorageMutability::ReadWrite,
		);

		let contract_account =
			T::AccountToAddr::convert(contract.clone()).map_err(|_| RootError::TxDecodeError)?;
		let new_code_identifier = CodeIdentifier::CodeId(code_id);
		let message: ContractMessageOf<T> = msg.try_into().map_err(|_| RootError::TxDecodeError)?;

		pallet_cosmwasm::Pallet::<T>::do_migrate(
			&mut shared,
			who,
			contract_account,
			new_code_identifier,
			message,
		)
		.map_err(|_| WasmError::MigrationFailed)?;

		// TODO: Same events emitted pallet_cosmos and pallet_cosmwasm
		let msg_event = CosmosEvent {
			r#type: EVENT_TYPE_MIGRATE.into(),
			attributes: vec![
				EventAttribute {
					key: ATTRIBUTE_KEY_CODE_ID.into(),
					value: code_id.to_string().into(),
				},
				EventAttribute { key: ATTRIBUTE_KEY_CONTRACT_ADDR.into(), value: contract.into() },
			],
		};

		ctx.event_manager().emit_event(msg_event);

		Ok(())
	}
}

pub struct MsgUpdateAdminHandler<T>(PhantomData<T>);

impl<T> Default for MsgUpdateAdminHandler<T> {
	fn default() -> Self {
		Self(Default::default())
	}
}

impl<T, Context> MsgHandler<Context> for MsgUpdateAdminHandler<T>
where
	T: pallet_cosmos::Config + pallet_cosmwasm::Config,
	Context: context::traits::Context,
{
	fn handle(&self, msg: &Any, ctx: &mut Context) -> Result<(), CosmosError> {
		let MsgUpdateAdmin { sender, new_admin, contract } =
			MsgUpdateAdmin::decode(&mut &*msg.value).map_err(|_| RootError::TxDecodeError)?;

		if sender.is_empty() {
			return Err(WasmError::Empty.into());
		}
		let (_hrp, address_raw) =
			acc_address_from_bech32(&sender).map_err(|_| RootError::InvalidAddress)?;
		if address_raw.len() != 20 {
			return Err(RootError::InvalidAddress.into());
		}
		let who = T::AddressMapping::into_account_id(H160::from_slice(&address_raw));
		let gas = ctx.gas_meter().gas_remaining();
		let mut shared = pallet_cosmwasm::Pallet::<T>::do_create_vm_shared(
			gas,
			InitialStorageMutability::ReadWrite,
		);

		let new_admin_account = if !new_admin.is_empty() {
			let new_admin_account = T::AccountToAddr::convert(new_admin.clone())
				.map_err(|_| RootError::InvalidAddress)?;
			Some(new_admin_account)
		} else {
			None
		};

		let contract_account =
			T::AccountToAddr::convert(contract.clone()).map_err(|_| RootError::TxDecodeError)?;

		pallet_cosmwasm::Pallet::<T>::do_update_admin(
			&mut shared,
			who,
			contract_account,
			new_admin_account,
		)
		.map_err(|_| WasmError::MigrationFailed)?;

		// TODO: Same events emitted pallet_cosmos and pallet_cosmwasm
		let msg_event = CosmosEvent {
			r#type: EVENT_TYPE_UPDATE_CONTRACT_ADMIN.into(),
			attributes: vec![
				EventAttribute { key: ATTRIBUTE_KEY_CONTRACT_ADDR.into(), value: contract.into() },
				EventAttribute { key: ATTRIBUTE_KEY_NEW_ADMIN.into(), value: new_admin.into() },
			],
		};

		ctx.event_manager().emit_event(msg_event);

		Ok(())
	}
}

fn convert_funds<T: pallet_cosmwasm::Config>(coins: &[Coin]) -> Result<FundsOf<T>, CosmosError> {
	// TODO: Handle native asset
	let mut funds = FundsOf::<T>::default();
	for coin in coins.iter() {
		let asset_id =
			T::AssetToDenom::convert(coin.denom.clone()).map_err(|_| RootError::TxDecodeError)?;
		let amount = u128::from_str(&coin.amount).map_err(|_| RootError::TxDecodeError)?;

		funds
			.try_insert(asset_id, (amount.saturated_into(), true))
			.map_err(|_| RootError::TxDecodeError)?;
	}

	Ok(funds)
}
