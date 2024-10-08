#![cfg_attr(not(feature = "std"), no_std)]
#![feature(trait_alias)]
#![cfg_attr(test, feature(assert_matches))]
#![allow(soft_unstable)]
#![feature(test)]

extern crate alloc;

pub mod code_gen;
pub mod validation;
pub mod version;

pub(crate) mod error;
pub(crate) mod host_functions;
pub(crate) mod vm;

pub use error::*;
pub use vm::*;

#[cfg(test)]
mod semantic;

use alloc::{
	string::{String, ToString},
	vec,
	vec::Vec,
};
use core::marker::PhantomData;
use cosmwasm_vm::{
	executor::{
		AllocateCall, AsFunctionName, CosmwasmCallInput, CosmwasmCallWithoutInfoInput,
		DeallocateCall, Unit,
	},
	tagged::Tagged,
	vm::{VMBase, VmErrorOf, VmGas, VmQueryCustomOf},
};
use wasmi::{
	AsContextMut, Engine, Extern, Instance, Linker, Memory, Module, Store, Value as RuntimeValue,
};

/// A wasmi module reference
#[derive(Clone)]
pub struct WasmiModule {
	/// Instance of a wasm module
	pub instance: Instance,
	/// Exported memory of the wasm module
	pub memory: Memory,
}

/// Api to `WasmiModule` in the inner VM.
/// This should be handled by the inner VM because of the current limitations
/// with `Wasmi`
pub trait WasmiContext {
	/// Returns the module that is ready to be executed, if any
	fn executing_module(&self) -> Option<WasmiModule>;

	/// Sets the executing module
	fn set_wasmi_context(&mut self, instance: Instance, memory: Memory);

	/// Returns function call depth counter.
	///
	/// Counts how many times we’ve moved from host code to inside of the WASM
	/// code.  This is tracked to avoid unbound recursion caused by malicious
	/// contract calling host function which calls back into WASM code.  See
	/// https://github.com/JumpCrypto/security-research/blob/master/advisories/2023-003-cosmwasm.md
	fn call_depth_mut(&mut self) -> &mut u32;
}

/// Output of wasmi functions
pub struct WasmiOutput<T>(WasmiFunctionResult, PhantomData<T>);

/// Name of wasm functions
pub struct WasmiFunctionName(String);

/// Params of wasm functions
pub struct WasmiFunctionParams(Vec<RuntimeValue>);

/// Result of wasm functions
pub struct WasmiFunctionResult(Vec<RuntimeValue>);

/// Describes wasm functions
/// fn `WasmiFunctionName`(`WasmiFunctionParams`..) -> `WasmiFunctionResult`;
pub struct WasmiInput<T>(
	WasmiFunctionName,
	WasmiFunctionParams,
	/// This result will be overwritten by the wasm call. See `raw_call`.
	WasmiFunctionResult,
	PhantomData<T>,
);

impl<T> WasmiInput<T> {
	#[must_use]
	pub fn new(name: String, params: Vec<RuntimeValue>, result: Vec<RuntimeValue>) -> Self {
		WasmiInput(
			WasmiFunctionName(name),
			WasmiFunctionParams(params),
			WasmiFunctionResult(result),
			PhantomData,
		)
	}
}

impl<V, S> TryFrom<WasmiOutput<WasmiVM<V, S>>> for RuntimeValue
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
{
	type Error = VmErrorOf<V>;
	fn try_from(
		WasmiOutput(WasmiFunctionResult(values), _): WasmiOutput<WasmiVM<V, S>>,
	) -> Result<Self, Self::Error> {
		match values.as_slice() {
			[ref run_val] => Ok(run_val.clone()),
			_ => Err(WasmiVMError::UnexpectedReturnType.into()),
		}
	}
}

impl<V, S> TryFrom<WasmiOutput<WasmiVM<V, S>>> for Unit
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
{
	type Error = VmErrorOf<V>;
	fn try_from(
		WasmiOutput(WasmiFunctionResult(values), _): WasmiOutput<WasmiVM<V, S>>,
	) -> Result<Self, Self::Error> {
		if values.is_empty() {
			Ok(Unit)
		} else {
			Err(WasmiVMError::ExpectedUnit.into())
		}
	}
}

impl<V, S> TryFrom<WasmiOutput<WasmiVM<V, S>>> for u32
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
{
	type Error = VmErrorOf<V>;
	fn try_from(
		WasmiOutput(WasmiFunctionResult(values), _): WasmiOutput<WasmiVM<V, S>>,
	) -> Result<Self, Self::Error> {
		// we target wasm32 so this will not truncate
		#[allow(
			clippy::cast_possible_truncation,
			clippy::cast_possible_wrap,
			clippy::cast_sign_loss
		)]
		match values.as_slice() {
			&[RuntimeValue::I32(ret_val)] => Ok(ret_val as u32),
			_ => Err(WasmiVMError::ExpectedPointer.into()),
		}
	}
}

impl<V, S> TryFrom<AllocateCall<u32>> for WasmiInput<WasmiVM<V, S>>
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
{
	type Error = VmErrorOf<V>;
	// we target wasm32 so this will not truncate
	#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
	fn try_from(AllocateCall(ptr): AllocateCall<u32>) -> Result<Self, Self::Error> {
		Ok(WasmiInput::new(
			AllocateCall::<u32>::NAME.into(),
			vec![RuntimeValue::I32(ptr as i32)],
			vec![RuntimeValue::I32(0)],
		))
	}
}

impl<V, S> TryFrom<DeallocateCall<u32>> for WasmiInput<WasmiVM<V, S>>
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
{
	type Error = VmErrorOf<V>;
	#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
	fn try_from(DeallocateCall(ptr): DeallocateCall<u32>) -> Result<Self, Self::Error> {
		Ok(WasmiInput::new(
			DeallocateCall::<u32>::NAME.into(),
			vec![RuntimeValue::I32(ptr as i32)],
			Vec::new(),
		))
	}
}

impl<'a, I, V, S> TryFrom<CosmwasmCallInput<'a, u32, I>> for WasmiInput<WasmiVM<V, S>>
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
	I: AsFunctionName,
{
	type Error = VmErrorOf<V>;
	#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss)]
	fn try_from(
		CosmwasmCallInput(Tagged(env_ptr, _), Tagged(info_ptr, _), Tagged(msg_ptr, _), _): CosmwasmCallInput<'a, u32, I>,
	) -> Result<Self, Self::Error> {
		Ok(WasmiInput::new(
			I::NAME.into(),
			vec![
				RuntimeValue::I32(env_ptr as i32),
				RuntimeValue::I32(info_ptr as i32),
				RuntimeValue::I32(msg_ptr as i32),
			],
			vec![RuntimeValue::I32(0)],
		))
	}
}

impl<'a, I, V, S> TryFrom<CosmwasmCallWithoutInfoInput<'a, u32, I>> for WasmiInput<WasmiVM<V, S>>
where
	V: WasmiBaseVM,
	S: AsContextMut<UserState = V>,
	I: AsFunctionName,
{
	type Error = VmErrorOf<V>;
	#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss)]
	fn try_from(
		CosmwasmCallWithoutInfoInput(Tagged(env_ptr, _), Tagged(msg_ptr, _), _): CosmwasmCallWithoutInfoInput<
            'a,
            u32,
            I,
        >,
	) -> Result<Self, Self::Error> {
		Ok(WasmiInput::new(
			I::NAME.into(),
			vec![RuntimeValue::I32(env_ptr as i32), RuntimeValue::I32(msg_ptr as i32)],
			vec![RuntimeValue::I32(0)],
		))
	}
}

/// Note that validation is not done here since the implementers probably wouldn't want
/// to do an expensive validation on each time they load the same code. So DO NOT forget
/// to use `CodeValidation` to properly validate the wasm module.
pub fn new_wasmi_vm<V: WasmiBaseVM>(code: &[u8], data: V) -> Result<OwnedWasmiVM<V>, VmErrorOf<V>> {
	let engine = Engine::default();
	let module = Module::new(&engine, code).map_err(Into::<wasmi::Error>::into)?;

	let mut store = Store::new(&engine, data);
	let mut linker = <Linker<V>>::new(&engine);

	host_functions::define(store.as_context_mut(), &mut linker)?;

	let instance = linker
		.instantiate(&mut store, &module)
		.map_err(Into::<wasmi::Error>::into)?
		.start(&mut store)
		.map_err(Into::<wasmi::Error>::into)?;

	let memory = instance
		.get_export(store.as_context_mut(), "memory")
		.and_then(Extern::into_memory)
		.ok_or(WasmiVMError::MemoryNotExported)?;

	store.data_mut().set_wasmi_context(instance, memory);

	Ok(WasmiVM(store))
}
