// system.rs ---

#[cfg(feature = "stargate")]
use crate::executor::ibc::{
	IbcChannelCloseCall, IbcChannelConnectCall, IbcPacketAckCall, IbcPacketReceiveCall,
	IbcPacketTimeoutCall,
};
#[cfg(feature = "stargate")]
use crate::executor::AsFunctionName;
use crate::{
	executor::{
		cosmwasm_call, AllocateCall, CosmwasmCallInput, CosmwasmCallWithoutInfoInput,
		CosmwasmQueryResult, DeallocateCall, DeserializeLimit, ExecuteCall, ExecutorError, HasInfo,
		InstantiateCall, MigrateCall, QueryResult, ReadLimit, ReplyCall, Unit,
	},
	has::Has,
	input::{Input, OutputOf},
	memory::{PointerOf, ReadWriteMemory, ReadableMemoryErrorOf, WritableMemoryErrorOf},
	transaction::{Transactional, TransactionalErrorOf},
	vm::{
		VmAddressOf, VmErrorOf, VmGasCheckpoint, VmInputOf, VmMessageCustomOf, VmOutputOf,
		VmQueryCustomOf, VM,
	},
};
use alloc::{
	fmt::Display,
	format,
	string::{String, ToString},
	vec,
	vec::Vec,
};
use core::fmt::Debug;
use cosmwasm_std::{
	Addr, AllBalanceResponse, Attribute, BalanceResponse, BankMsg, BankQuery, Binary,
	ContractResult, CosmosMsg, Env, Event, MessageInfo, QueryRequest, Reply, ReplyOn, Response,
	SubMsgResponse, SubMsgResult, SupplyResponse, SystemResult, WasmMsg, WasmQuery,
};
#[cfg(feature = "stargate")]
use cosmwasm_std::{Empty, IbcMsg};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

// WasmModuleEventType is stored with any contract TX that returns non empty EventAttributes
pub const WASM_MODULE_EVENT_TYPE: &str = "wasm";

// CustomContractEventPrefix contracts can create custom events. To not mix them with other system
// events they got the `wasm-` prefix.
pub const CUSTOM_CONTRACT_EVENT_PREFIX: &str = "wasm-";

// Minimum length of an event type
pub const CUSTOM_CONTRACT_EVENT_TYPE_MIN_LENGTH: usize = 2;

pub const WASM_MODULE_EVENT_RESERVED_PREFIX: &str = "_";

#[allow(unused)]
#[allow(clippy::module_name_repetitions)]
pub enum SystemEventType {
	StoreCode,
	Instantiate,
	Execute,
	Migrate,
	PinCode,
	UnpinCode,
	Sudo,
	Reply,
	#[cfg(feature = "stargate")]
	IbcChannelConnect,
	#[cfg(feature = "stargate")]
	IbcChannelClose,
	#[cfg(feature = "stargate")]
	IbcPacketReceive,
	#[cfg(feature = "stargate")]
	IbcPacketAck,
	#[cfg(feature = "stargate")]
	IbcPacketTimeout,
}

#[allow(clippy::module_name_repetitions)]
pub enum SystemAttributeKey {
	ContractAddr,
	CodeID,
	ResultDataHex,
	Feature,
}

#[allow(clippy::module_name_repetitions)]
pub struct SystemAttribute {
	key: SystemAttributeKey,
	value: String,
}

#[allow(clippy::module_name_repetitions)]
pub struct SystemEvent {
	ty: SystemEventType,
	attributes: Vec<SystemAttribute>,
}

impl From<SystemAttribute> for Attribute {
	fn from(SystemAttribute { key, value }: SystemAttribute) -> Self {
		let attr_str = match key {
			SystemAttributeKey::ContractAddr => "_contract_address",
			SystemAttributeKey::CodeID => "code_id",
			SystemAttributeKey::ResultDataHex => "result",
			SystemAttributeKey::Feature => "feature",
		};

		Attribute { key: attr_str.into(), value }
	}
}

impl Display for SystemEventType {
	fn fmt(&self, f: &mut alloc::fmt::Formatter) -> alloc::fmt::Result {
		let event_str = match self {
			SystemEventType::StoreCode => "store_code",
			SystemEventType::Instantiate => "instantiate",
			SystemEventType::Execute => "execute",
			SystemEventType::Migrate => "migrate",
			SystemEventType::PinCode => "pin_code",
			SystemEventType::UnpinCode => "unpin_code",
			SystemEventType::Sudo => "sudo",
			SystemEventType::Reply => "reply",
			#[cfg(feature = "stargate")]
			SystemEventType::IbcChannelConnect => IbcChannelConnectCall::<Empty>::NAME,
			#[cfg(feature = "stargate")]
			SystemEventType::IbcChannelClose => IbcChannelCloseCall::<Empty>::NAME,
			#[cfg(feature = "stargate")]
			SystemEventType::IbcPacketReceive => IbcPacketReceiveCall::<Empty>::NAME,
			#[cfg(feature = "stargate")]
			SystemEventType::IbcPacketAck => IbcPacketAckCall::<Empty>::NAME,
			#[cfg(feature = "stargate")]
			SystemEventType::IbcPacketTimeout => IbcPacketTimeoutCall::<Empty>::NAME,
		};

		write!(f, "{event_str}")
	}
}

impl From<SystemEvent> for Event {
	fn from(sys_event: SystemEvent) -> Self {
		Event::new(sys_event.ty.to_string())
			.add_attributes(sys_event.attributes.into_iter().map(Into::<Attribute>::into))
	}
}

pub trait EventHasCodeId {
	const HAS_CODE_ID: bool;
}

impl<T> EventHasCodeId for InstantiateCall<T> {
	const HAS_CODE_ID: bool = true;
}

impl<T> EventHasCodeId for ExecuteCall<T> {
	const HAS_CODE_ID: bool = false;
}

impl<T> EventHasCodeId for MigrateCall<T> {
	const HAS_CODE_ID: bool = true;
}

impl<T> EventHasCodeId for ReplyCall<T> {
	const HAS_CODE_ID: bool = false;
}

#[cfg(feature = "stargate")]
impl<T> EventHasCodeId for IbcChannelConnectCall<T> {
	const HAS_CODE_ID: bool = false;
}

#[cfg(feature = "stargate")]
impl<T> EventHasCodeId for IbcChannelCloseCall<T> {
	const HAS_CODE_ID: bool = false;
}

#[cfg(feature = "stargate")]
impl<T> EventHasCodeId for IbcPacketReceiveCall<T> {
	const HAS_CODE_ID: bool = false;
}

#[cfg(feature = "stargate")]
impl<T> EventHasCodeId for IbcPacketAckCall<T> {
	const HAS_CODE_ID: bool = false;
}

#[cfg(feature = "stargate")]
impl<T> EventHasCodeId for IbcPacketTimeoutCall<T> {
	const HAS_CODE_ID: bool = false;
}

pub trait EventIsTyped {
	const TYPE: SystemEventType;
}

impl<T> EventIsTyped for InstantiateCall<T> {
	const TYPE: SystemEventType = SystemEventType::Instantiate;
}

impl<T> EventIsTyped for ExecuteCall<T> {
	const TYPE: SystemEventType = SystemEventType::Execute;
}

impl<T> EventIsTyped for MigrateCall<T> {
	const TYPE: SystemEventType = SystemEventType::Migrate;
}

impl<T> EventIsTyped for ReplyCall<T> {
	const TYPE: SystemEventType = SystemEventType::Reply;
}

#[cfg(feature = "stargate")]
impl<T> EventIsTyped for IbcChannelConnectCall<T> {
	const TYPE: SystemEventType = SystemEventType::IbcChannelConnect;
}

#[cfg(feature = "stargate")]
impl<T> EventIsTyped for IbcChannelCloseCall<T> {
	const TYPE: SystemEventType = SystemEventType::IbcChannelClose;
}

#[cfg(feature = "stargate")]
impl<T> EventIsTyped for IbcPacketReceiveCall<T> {
	const TYPE: SystemEventType = SystemEventType::IbcPacketReceive;
}

#[cfg(feature = "stargate")]
impl<T> EventIsTyped for IbcPacketAckCall<T> {
	const TYPE: SystemEventType = SystemEventType::IbcPacketAck;
}

#[cfg(feature = "stargate")]
impl<T> EventIsTyped for IbcPacketTimeoutCall<T> {
	const TYPE: SystemEventType = SystemEventType::IbcPacketTimeout;
}

pub trait HasEvent {
	fn generate_event(address: String, code_id: CosmwasmCodeId) -> Event;
}

impl<I> HasEvent for I
where
	I: Input + EventHasCodeId + EventIsTyped,
{
	fn generate_event(address: String, code_id: CosmwasmCodeId) -> Event {
		let addr_attr = SystemAttribute { key: SystemAttributeKey::ContractAddr, value: address };
		let attributes = if I::HAS_CODE_ID {
			vec![
				addr_attr,
				SystemAttribute { key: SystemAttributeKey::CodeID, value: code_id.to_string() },
			]
		} else {
			vec![addr_attr]
		};
		SystemEvent { ty: I::TYPE, attributes }.into()
	}
}

/// Errors likely to happen while a VM is executing.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(clippy::module_name_repetitions)]
pub enum SystemError {
	UnsupportedMessage,
	FailedToSerialize,
	ContractExecutionFailure(String),
	ImmutableCantMigrate,
	MustBeAdmin,
	ReservedEventPrefixIsUsed,
	EmptyEventKey,
	EmptyEventValue,
	EventTypeIsTooShort,
}

#[derive(Debug)]
enum SubCallContinuation<E> {
	Continue(Option<Binary>),
	Reply(SubMsgResult),
	Abort(E),
}

pub type CosmwasmCodeId = u64;

/// Minimum metadata associated to contracts.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CosmwasmContractMeta<Account> {
	pub code_id: CosmwasmCodeId,
	pub admin: Option<Account>,
	pub label: String,
}

pub trait CosmwasmBaseVM = VM<
		ContractMeta = CosmwasmContractMeta<VmAddressOf<Self>>,
		StorageKey = Vec<u8>,
		StorageValue = Vec<u8>,
	> + ReadWriteMemory
	+ Transactional
	+ Has<Env>
	+ Has<MessageInfo>
where
	VmMessageCustomOf<Self>: DeserializeOwned + Debug,
	VmQueryCustomOf<Self>: DeserializeOwned + Debug,
	VmAddressOf<Self>: Clone + TryFrom<String, Error = VmErrorOf<Self>> + Into<Addr>,
	VmErrorOf<Self>: From<ReadableMemoryErrorOf<Self>>
		+ From<WritableMemoryErrorOf<Self>>
		+ From<ExecutorError>
		+ From<SystemError>
		+ From<TransactionalErrorOf<Self>>
		+ Debug,
	PointerOf<Self>: for<'x> TryFrom<VmOutputOf<'x, Self>, Error = VmErrorOf<Self>>;

pub trait CosmwasmCallVM<I> = CosmwasmBaseVM
where
	I: Input + HasInfo + HasEvent,
	OutputOf<I>: Into<ContractResult<Response<VmMessageCustomOf<Self>>>>
		+ From<ContractResult<Response<VmMessageCustomOf<Self>>>>;

pub trait CosmwasmDynamicVM<I> = CosmwasmBaseVM
where
	for<'x> Unit: TryFrom<VmOutputOf<'x, Self>, Error = VmErrorOf<Self>>,
	for<'x> VmInputOf<'x, Self>: TryFrom<AllocateCall<PointerOf<Self>>, Error = VmErrorOf<Self>>
		+ TryFrom<DeallocateCall<PointerOf<Self>>, Error = VmErrorOf<Self>>
		+ TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, InstantiateCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, ExecuteCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, ReplyCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallWithoutInfoInput<'x, PointerOf<Self>, ReplyCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallWithoutInfoInput<'x, PointerOf<Self>, MigrateCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<CosmwasmCallInput<'x, PointerOf<Self>, I>, Error = VmErrorOf<Self>>
		+ TryFrom<CosmwasmCallWithoutInfoInput<'x, PointerOf<Self>, I>, Error = VmErrorOf<Self>>,
	I: Input + HasInfo + HasEvent,
	OutputOf<I>: DeserializeOwned + ReadLimit + DeserializeLimit;

#[cfg(feature = "stargate")]
/// Extra constraints required by stargate enabled `CosmWasm` VM (a.k.a. IBC capable).
pub trait StargateCosmwasmCallVM = CosmwasmBaseVM
where
	for<'x> VmInputOf<'x, Self>: TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, IbcChannelConnectCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, IbcChannelCloseCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, IbcPacketReceiveCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, IbcPacketAckCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		> + TryFrom<
			CosmwasmCallInput<'x, PointerOf<Self>, IbcPacketTimeoutCall<VmMessageCustomOf<Self>>>,
			Error = VmErrorOf<Self>,
		>;

#[cfg(not(feature = "stargate"))]
pub trait StargateCosmwasmCallVM =;

pub fn cosmwasm_system_entrypoint_serialize<I, V, M>(
	vm: &mut V,
	message: &M,
) -> Result<(Option<Binary>, Vec<Event>), VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + CosmwasmDynamicVM<I> + StargateCosmwasmCallVM,
	M: Serialize,
{
	cosmwasm_system_entrypoint_serialize_hook(vm, message, |vm, msg| {
		cosmwasm_call::<I, V>(vm, msg).map(Into::into)
	})
}

/// Extra helper to dispatch a typed message, serializing on the go.
pub fn cosmwasm_system_entrypoint_serialize_hook<I, V, M>(
	vm: &mut V,
	message: &M,
	hook: impl FnOnce(
		&mut V,
		&[u8],
	) -> Result<ContractResult<Response<VmMessageCustomOf<V>>>, VmErrorOf<V>>,
) -> Result<(Option<Binary>, Vec<Event>), VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + StargateCosmwasmCallVM,
	M: Serialize,
{
	cosmwasm_system_entrypoint_hook(
		vm,
		&serde_json::to_vec(message).map_err(|_| SystemError::FailedToSerialize)?,
		hook,
	)
}

pub fn cosmwasm_system_entrypoint<I, V>(
	vm: &mut V,
	message: &[u8],
) -> Result<(Option<Binary>, Vec<Event>), VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + CosmwasmDynamicVM<I> + StargateCosmwasmCallVM,
{
	log::debug!("SystemEntrypoint");
	cosmwasm_system_entrypoint_hook(vm, message, |vm, msg| {
		cosmwasm_call::<I, V>(vm, msg).map(Into::into)
	})
}

/// High level dispatch for a `CosmWasm` VM.
/// This call will manage and handle subcall as well as the transactions etc...
/// The implementation must be semantically valid w.r.t <https://github.com/CosmWasm/cosmwasm/blob/main/SEMANTICS.md>
///
/// Returns either the value produced by the contract along the generated events or a `VmErrorOf<V>`
pub fn cosmwasm_system_entrypoint_hook<I, V>(
	vm: &mut V,
	message: &[u8],
	hook: impl FnOnce(
		&mut V,
		&[u8],
	) -> Result<ContractResult<Response<VmMessageCustomOf<V>>>, VmErrorOf<V>>,
) -> Result<(Option<Binary>, Vec<Event>), VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + StargateCosmwasmCallVM,
{
	log::debug!("SystemEntrypointHook");
	let mut events = Vec::<Event>::new();
	let mut event_handler = |event: Event| {
		events.push(event);
	};
	vm.transaction_begin()?;
	match cosmwasm_system_run_hook::<I, V>(vm, message, &mut event_handler, hook) {
		Ok(data) => {
			vm.transaction_commit()?;
			Ok((data, events))
		},
		Err(e) => {
			vm.transaction_rollback()?;
			Err(e)
		},
	}
}

/// Set `new_code_id` as the code id of the contract `contract_addr`
///
/// Fails if the caller is not the admin of the contract
pub fn migrate<V: CosmwasmBaseVM>(
	vm: &mut V,
	sender: VmAddressOf<V>,
	contract_addr: VmAddressOf<V>,
	new_code_id: CosmwasmCodeId,
) -> Result<(), VmErrorOf<V>> {
	let CosmwasmContractMeta { admin, label, .. } = vm.contract_meta(contract_addr.clone())?;
	ensure_admin::<V>(&sender.into(), admin.clone())?;
	vm.set_contract_meta(
		contract_addr,
		CosmwasmContractMeta { code_id: new_code_id, admin, label },
	)?;
	Ok(())
}

/// Set `new_admin` as the new admin of the contract `contract_addr`
///
/// Fails if the caller is not currently admin of the target contract.
pub fn update_admin<V: CosmwasmBaseVM>(
	vm: &mut V,
	sender: &Addr,
	contract_addr: VmAddressOf<V>,
	new_admin: Option<VmAddressOf<V>>,
) -> Result<(), VmErrorOf<V>> {
	let CosmwasmContractMeta { code_id, admin, label } = vm.contract_meta(contract_addr.clone())?;
	ensure_admin::<V>(sender, admin)?;
	vm.set_contract_meta(contract_addr, CosmwasmContractMeta { code_id, admin: new_admin, label })?;
	Ok(())
}

fn ensure_admin<V: CosmwasmBaseVM>(
	sender: &Addr,
	contract_admin: Option<VmAddressOf<V>>,
) -> Result<(), VmErrorOf<V>> {
	match contract_admin.map(Into::<Addr>::into) {
		None => Err(SystemError::ImmutableCantMigrate.into()),
		Some(admin) if admin == *sender => Ok(()),
		_ => Err(SystemError::MustBeAdmin.into()),
	}
}

fn sanitize_custom_attributes(
	mut attributes: Vec<Attribute>,
	contract_address: String,
) -> Result<Vec<Attribute>, SystemError> {
	for attr in attributes.iter_mut() {
		let new_key = attr.key.trim();
		if new_key.is_empty() {
			return Err(SystemError::EmptyEventKey);
		}

		let new_value = attr.value.trim();
		if new_value.is_empty() {
			return Err(SystemError::EmptyEventValue);
		}

		// this must be checked after being trimmed
		if new_key.starts_with(WASM_MODULE_EVENT_RESERVED_PREFIX) {
			return Err(SystemError::ReservedEventPrefixIsUsed);
		}

		attr.key = new_key.into();
		attr.value = new_value.into();
	}

	// contract address attribute is added to every event
	attributes.push(
		SystemAttribute { key: SystemAttributeKey::ContractAddr, value: contract_address }.into(),
	);

	Ok(attributes)
}

#[allow(clippy::too_many_lines)]
fn dispatch_submessage<V, I>(
	vm: &mut V,
	caller_contract: &Addr,
	msg: CosmosMsg<VmMessageCustomOf<V>>,
	gas_limit: Option<u64>,
	event_handler: &mut dyn FnMut(Event),
) -> Result<(Option<Binary>, Vec<Event>), VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + StargateCosmwasmCallVM,
{
	// Gas might be limited for the sub message execution.
	vm.gas_checkpoint_push(match gas_limit {
		Some(limit) => VmGasCheckpoint::Limited(limit),
		None => VmGasCheckpoint::Unlimited,
	})?;

	let mut sub_events = Vec::<Event>::new();

	// Events dispatched by a submessage are added to both the
	// submessage events and parent events.
	let mut sub_event_handler = |event: Event| {
		event_handler(event.clone());
		sub_events.push(event);
	};

	let sub_result = (|| match msg {
		CosmosMsg::Custom(message) =>
			vm.message_custom(message, &mut sub_event_handler).map_err(Into::into),
		CosmosMsg::Wasm(wasm_message) => match wasm_message {
			WasmMsg::Execute { contract_addr, msg, funds } => {
				let vm_contract_addr = contract_addr.try_into()?;
				vm.continue_execute(vm_contract_addr, funds, &msg, &mut sub_event_handler)
			},
			WasmMsg::Instantiate { admin, code_id, msg, funds, label } => vm
				.continue_instantiate(
					CosmwasmContractMeta {
						code_id,
						admin: match admin {
							Some(admin) => Some(admin.try_into()?),
							None => None,
						},
						label,
					},
					funds,
					&msg,
					&mut sub_event_handler,
				)
				.map(|(_, data)| data),
			WasmMsg::Instantiate2 { admin, code_id, label, msg, funds, salt } => vm
				.continue_instantiate2(
					CosmwasmContractMeta {
						code_id,
						admin: match admin {
							Some(admin) => Some(admin.try_into()?),
							None => None,
						},
						label,
					},
					funds,
					&salt,
					&msg,
					&mut sub_event_handler,
				)
				.map(|(_, data)| data),
			WasmMsg::Migrate { contract_addr, new_code_id, msg } => {
				let contract_addr = VmAddressOf::<V>::try_from(contract_addr)?;
				let sender = VmAddressOf::<V>::try_from(caller_contract.clone().into_string())?;
				migrate::<V>(vm, sender, contract_addr.clone(), new_code_id)?;
				vm.continue_migrate(contract_addr, &msg, &mut sub_event_handler)
			},
			WasmMsg::UpdateAdmin { contract_addr, admin: new_admin } => {
				let new_admin = new_admin.try_into()?;
				let vm_contract_addr = VmAddressOf::<V>::try_from(contract_addr)?;
				update_admin::<V>(vm, caller_contract, vm_contract_addr, Some(new_admin))?;
				Ok(None)
			},
			WasmMsg::ClearAdmin { contract_addr } => {
				let vm_contract_addr = VmAddressOf::<V>::try_from(contract_addr)?;
				update_admin::<V>(vm, caller_contract, vm_contract_addr, None)?;
				Ok(None)
			},
			_ => Err(SystemError::UnsupportedMessage.into()),
		},
		CosmosMsg::Bank(bank_message) => match bank_message {
			BankMsg::Send { to_address, amount } => {
				let vm_contract_addr = to_address.try_into()?;
				vm.transfer(&vm_contract_addr, &amount)?;
				Ok(None)
			},
			BankMsg::Burn { amount } => {
				vm.burn(&amount)?;
				Ok(None)
			},
			_ => Err(SystemError::UnsupportedMessage.into()),
		},
		#[cfg(feature = "stargate")]
		CosmosMsg::Ibc(ibc_message) => match ibc_message {
			IbcMsg::Transfer { channel_id, to_address, amount, timeout, memo: _ } => {
				vm.ibc_transfer(channel_id, to_address, amount, timeout)?;
				Ok(None)
			},
			IbcMsg::SendPacket { channel_id, data, timeout } => {
				vm.ibc_send_packet(channel_id, data, timeout)?;
				Ok(None)
			},
			IbcMsg::CloseChannel { channel_id } => {
				vm.ibc_close_channel(channel_id)?;
				Ok(None)
			},
			_ => Err(SystemError::UnsupportedMessage.into()),
		},
		// TODO(hussein-aitlahcen): determine whether we handle.
		#[cfg(feature = "stargate")]
		CosmosMsg::Stargate { .. } => Err(SystemError::UnsupportedMessage.into()),
		// TODO(hussein-aitlahcen): determine whether we handle.
		#[cfg(feature = "stargate")]
		CosmosMsg::Gov(_) => Err(SystemError::UnsupportedMessage.into()),
		_ => Err(SystemError::UnsupportedMessage.into()),
	})();

	// Make sure we remove the checkpoint.
	vm.gas_checkpoint_pop()?;

	sub_result.map(|data| (data, sub_events))
}

#[allow(clippy::too_many_lines)]
pub fn cosmwasm_system_run<I, V>(
	vm: &mut V,
	message: &[u8],
	event_handler: &mut dyn FnMut(Event),
) -> Result<Option<Binary>, VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + CosmwasmDynamicVM<I> + StargateCosmwasmCallVM,
{
	cosmwasm_system_run_hook(vm, message, event_handler, |vm, msg| {
		cosmwasm_call::<I, V>(vm, msg).map(Into::into)
	})
}

pub fn cosmwasm_system_run_hook<I, V>(
	vm: &mut V,
	message: &[u8],
	event_handler: &mut dyn FnMut(Event),
	hook: impl FnOnce(
		&mut V,
		&[u8],
	) -> Result<ContractResult<Response<VmMessageCustomOf<V>>>, VmErrorOf<V>>,
) -> Result<Option<Binary>, VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + StargateCosmwasmCallVM,
{
	log::debug!("SystemRun");
	let info: MessageInfo = vm.get();
	let env: Env = vm.get();
	vm.transfer_from(
		&info.sender.clone().into_string().try_into()?,
		&env.contract.address.clone().into_string().try_into()?,
		info.funds.as_slice(),
	)?;
	let output = hook(vm, message).map(Into::into);
	log::debug!("Output: {:?}", output);
	match output {
		Ok(ContractResult::Ok(response)) =>
			handle_ok_run_response(vm, event_handler, env, response),
		Ok(ContractResult::Err(e)) => Err(SystemError::ContractExecutionFailure(e).into()),
		Err(e) => Err(e),
	}
}

fn handle_ok_run_response<I, V>(
	vm: &mut V,
	event_handler: &mut dyn FnMut(Event),
	env: Env,
	response: Response<VmMessageCustomOf<V>>,
) -> Result<Option<Binary>, VmErrorOf<V>>
where
	V: CosmwasmCallVM<I> + StargateCosmwasmCallVM,
{
	let CosmwasmContractMeta { code_id, .. } = vm.running_contract_meta()?;
	let event = I::generate_event(env.contract.address.clone().into_string(), code_id);
	event_handler(event);

	// https://github.com/CosmWasm/wasmd/blob/ac92fdcf37388cc8dc24535f301f64395f8fb3da/x/wasm/keeper/events.go#L16
	if !response.attributes.is_empty() {
		let attributes = sanitize_custom_attributes(
			response.attributes,
			env.contract.address.clone().into_string(),
		)?;
		event_handler(Event::new(WASM_MODULE_EVENT_TYPE).add_attributes(attributes));
	}

	// Embed orphan attributes in a custom contract event.
	// https://github.com/CosmWasm/wasmd/blob/ac92fdcf37388cc8dc24535f301f64395f8fb3da/x/wasm/keeper/events.go#L29
	for event in response.events {
		let ty = event.ty.trim();
		if ty.len() < CUSTOM_CONTRACT_EVENT_TYPE_MIN_LENGTH {
			return Err(SystemError::EventTypeIsTooShort.into());
		}
		let attributes = sanitize_custom_attributes(
			event.attributes,
			env.contract.address.clone().into_string(),
		)?;
		let ty = [CUSTOM_CONTRACT_EVENT_PREFIX, ty].concat();
		event_handler(Event::new(ty).add_attributes(attributes));
	}

	// Fold dispatch over the submessages. If an exception occur (unless
	// expected reply on error), we abort.  Otherwise, it's up to the parent
	// contract to decide in each case.
	let mut data = response.data;
	for submsg in response.messages {
		log::debug!("Executing submessages");

		// For each submessages, we might rollback and reply the failure to the
		// parent contract. Hence, a new state tx must be created prior to the
		// call.
		vm.transaction_begin()?;

		// The result MUST be captured to determine whether we rollback or
		// commit the local transaction.  We MUST not return using something
		// like the questionmark operator, as we want to catch both the success
		// and failure branches here.  Both branches may be used depending on
		// the reply attached to the message. See reply_on.
		let sub_res = dispatch_submessage(
			vm,
			&env.contract.address,
			submsg.msg,
			submsg.gas_limit,
			event_handler,
		);

		log::debug!("Submessage result: {:?}", sub_res);

		let sub_cont = match (sub_res, submsg.reply_on) {
			// If the submessage suceeded and no reply was asked or only on
			// error, the call is considered successful and state change is
			// comitted.
			(Ok((data, _)), ReplyOn::Never | ReplyOn::Error) => {
				log::debug!("Commit & Continue");
				vm.transaction_commit()?;
				SubCallContinuation::Continue(data)
			},
			// Similarly to previous case, if the submessage suceeded and we ask
			// for a reply, the call is considered successful and we redispatch
			// a reply to the parent contract.
			(Ok((data, events)), ReplyOn::Always | ReplyOn::Success) => {
				log::debug!("Commit & Reply");
				vm.transaction_commit()?;
				// TODO: Handle msg_response
				SubCallContinuation::Reply(SubMsgResult::Ok(SubMsgResponse {
					events,
					data,
					msg_responses: Vec::new(),
				}))
			},
			// If the submessage failed and a reply is required, rollback the
			// state change and dispatch a reply to the parent contract. The
			// transaction is not aborted unless the reply also fails
			// (cascading).
			(Err(e), ReplyOn::Always | ReplyOn::Error) => {
				log::debug!("Rollback & Reply");
				vm.transaction_rollback()?;
				SubCallContinuation::Reply(SubMsgResult::Err(format!("{e:?}")))
			},
			// If an error happen and we did not expected it, abort the whole
			// transaction.
			(Err(e), ReplyOn::Never | ReplyOn::Success) => {
				log::debug!("Rollback & Abort");
				vm.transaction_rollback()?;
				SubCallContinuation::Abort(e)
			},
		};

		log::debug!("Submessage cont: {:?}", sub_cont);

		data = match sub_cont {
			// If the submessage execution suceeded and we don't want to reply,
			// proceed by overwritting the current `data` field if a new one has
			// been yield by the current call.
			SubCallContinuation::Continue(new_data) => new_data.or(data),
			// An exception occured and we did not expected it (no reply on
			// error), abort the current execution.
			SubCallContinuation::Abort(e) => return Err(e),
			// The parent contract is expected to get a reply, try to execute
			// the reply and optionally overwrite the current data with with the
			// one yield by the reply.
			SubCallContinuation::Reply(response) => {
				// TODO: Handle payload and gas_used
				let new_data = vm.continue_reply(
					Reply {
						id: submsg.id,
						result: response.clone(),
						payload: Binary::default(),
						gas_used: 0,
					},
					event_handler,
				)?;
				// Tricky situation, either the reply provide a new value that
				// we use, or we use the submessage value or we keep the current
				// one.
				new_data.or_else(|| Result::from(response).ok().and_then(|x| x.data)).or(data)
			},
		}
	}

	Ok(data)
}

/// High level query for a `CosmWasm` VM.
///
/// Returns either the value returned by the contract `query` export or a `VmErrorOf<V>`
pub fn cosmwasm_system_query<V>(
	vm: &mut V,
	request: QueryRequest<VmQueryCustomOf<V>>,
) -> Result<SystemResult<CosmwasmQueryResult>, VmErrorOf<V>>
where
	V: CosmwasmBaseVM,
{
	log::debug!("SystemQuery");
	match request {
		QueryRequest::Custom(query) => Ok(vm.query_custom(query)?),
		QueryRequest::Bank(bank_query) => match bank_query {
			BankQuery::Balance { address, denom } => {
				let vm_account_addr = address.try_into()?;
				let amount = vm.balance(&vm_account_addr, denom)?;
				let serialized_info = serde_json::to_vec(&BalanceResponse::new(amount))
					.map_err(|_| SystemError::FailedToSerialize)?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(serialized_info))))
			},
			BankQuery::AllBalances { address } => {
				let vm_account_addr = address.try_into()?;
				let amount = vm.all_balance(&vm_account_addr)?;
				let serialized_info = serde_json::to_vec(&AllBalanceResponse::new(amount))
					.map_err(|_| SystemError::FailedToSerialize)?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(serialized_info))))
			},
			BankQuery::Supply { denom } => {
				// `SupplyResponse` is non-exhaustive, so can't define as `SupplyResponse { denom }`
				let supply = SupplyResponse::new(vm.supply(denom)?);
				let serialized_info =
					serde_json::to_vec(&supply).map_err(|_| SystemError::FailedToSerialize)?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(serialized_info))))
			},
			_ => Err(SystemError::UnsupportedMessage.into()),
		},
		QueryRequest::Wasm(wasm_query) => match wasm_query {
			WasmQuery::Smart { contract_addr, msg: message } => {
				let vm_contract_addr = contract_addr.try_into()?;
				let QueryResult(output) = vm.continue_query(vm_contract_addr, &message)?;
				Ok(SystemResult::Ok(output))
			},
			WasmQuery::Raw { contract_addr, key } => {
				let vm_contract_addr = contract_addr.try_into()?;
				let value = vm.query_raw(vm_contract_addr, key.to_vec())?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(value.unwrap_or_default()))))
			},
			WasmQuery::ContractInfo { contract_addr } => {
				let vm_contract_addr = contract_addr.try_into()?;
				let info = vm.query_contract_info(vm_contract_addr)?;
				let serialized_info =
					serde_json::to_vec(&info).map_err(|_| SystemError::FailedToSerialize)?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(serialized_info))))
			},
			WasmQuery::CodeInfo { code_id } => {
				let info = vm.query_code_info(code_id)?;
				let serialized_info =
					serde_json::to_vec(&info).map_err(|_| SystemError::FailedToSerialize)?;
				Ok(SystemResult::Ok(ContractResult::Ok(Binary::new(serialized_info))))
			},
			_ => Err(SystemError::UnsupportedMessage.into()),
		},
		_ => Err(SystemError::UnsupportedMessage.into()),
	}
}

/// High level query for a `CosmWasm` VM with remarshalling for contract execution continuation.
///
/// Returns either the JSON serialized value returned by the contract `query` export or a
/// `VmErrorOf<V>`
pub fn cosmwasm_system_query_raw<V>(
	vm: &mut V,
	request: QueryRequest<VmQueryCustomOf<V>>,
) -> Result<Binary, VmErrorOf<V>>
where
	V: CosmwasmBaseVM,
{
	log::debug!("SystemQueryRaw");
	let output = cosmwasm_system_query(vm, request)?;
	Ok(Binary::new(serde_json::to_vec(&output).map_err(|_| SystemError::FailedToSerialize)?))
}
