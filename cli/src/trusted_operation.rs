/*
	Copyright 2021 Integritee AG and Supercomputing Systems AG

	Licensed under the Apache License, Version 2.0 (the "License");
	you may not use this file except in compliance with the License.
	You may obtain a copy of the License at

		http://www.apache.org/licenses/LICENSE-2.0

	Unless required by applicable law or agreed to in writing, software
	distributed under the License is distributed on an "AS IS" BASIS,
	WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
	See the License for the specific language governing permissions and
	limitations under the License.

*/

use crate::{
	command_utils::{encode_encrypt, get_chain_api, get_pair_from_str, get_worker_api_direct},
	trusted_commands::TrustedArgs,
	Cli,
};
use base58::FromBase58;
use codec::{Decode, Encode};
use ita_stf::{ShardIdentifier, TrustedCallSigned, TrustedOperation};
use itc_rpc_client::direct_client::DirectApi;
use itp_node_api_extensions::TEEREX;
use itp_types::{DirectRequestStatus, RpcRequest, RpcResponse, RpcReturnValue};
use log::*;
use my_node_runtime::{AccountId, Hash};
use sp_core::{sr25519 as sr25519_core, Pair, H256};
use std::{result::Result as StdResult, sync::mpsc::channel};
use substrate_api_client::{compose_extrinsic, XtStatus};
use teerex_primitives::Request;

pub fn perform_trusted_operation(
	cli: &Cli,
	trusted_args: &TrustedArgs,
	top: &TrustedOperation,
) -> Option<Vec<u8>> {
	match top {
		TrustedOperation::indirect_call(call) => send_request(cli, trusted_args, call.clone()),
		TrustedOperation::direct_call(call) =>
			send_direct_request(cli, trusted_args, TrustedOperation::direct_call(call.clone())),
		TrustedOperation::get(getter) =>
			get_state(cli, trusted_args, TrustedOperation::get(getter.clone())),
	}
}

fn get_state(cli: &Cli, trusted_args: &TrustedArgs, getter: TrustedOperation) -> Option<Vec<u8>> {
	// TODO: ensure getter is signed?
	let (_operation_call_encoded, operation_call_encrypted) = match encode_encrypt(cli, getter) {
		Ok((encoded, encrypted)) => (encoded, encrypted),
		Err(msg) => {
			println!("[Error] {}", msg);
			return None
		},
	};
	let shard = read_shard(trusted_args).unwrap();

	// compose jsonrpc call
	let data = Request { shard, cyphertext: operation_call_encrypted };
	let rpc_method = "author_submitAndWatchExtrinsic".to_owned();
	let jsonrpc_call: String = RpcRequest::compose_jsonrpc_call(rpc_method, data.encode());

	let direct_api = get_worker_api_direct(cli);
	let (sender, receiver) = channel();
	direct_api.watch(jsonrpc_call, sender);

	loop {
		match receiver.recv() {
			Ok(response) => {
				let response: RpcResponse = serde_json::from_str(&response).unwrap();
				if let Ok(return_value) = RpcReturnValue::decode(&mut response.result.as_slice()) {
					if return_value.status == DirectRequestStatus::Error {
						println!(
							"[Error] {}",
							String::decode(&mut return_value.value.as_slice()).unwrap()
						);
						return None
					}
					if !return_value.do_watch {
						return match Option::decode(&mut return_value.value.as_slice()) {
							Ok(value_opt) => value_opt,
							Err(_) => panic!("Error when decoding response"),
						}
					}
				};
			},
			Err(_) => return None,
		};
	}
}

fn send_request(cli: &Cli, trusted_args: &TrustedArgs, call: TrustedCallSigned) -> Option<Vec<u8>> {
	let chain_api = get_chain_api(cli);
	let (_, call_encrypted) = match encode_encrypt(cli, call) {
		Ok((encoded, encrypted)) => (encoded, encrypted),
		Err(msg) => {
			println!("[Error]: {}", msg);
			return None
		},
	};

	let shard = read_shard(trusted_args).unwrap();

	let arg_signer = &trusted_args.xt_signer;
	let signer = get_pair_from_str(arg_signer);
	let _chain_api = chain_api.set_signer(sr25519_core::Pair::from(signer));

	let request = Request { shard, cyphertext: call_encrypted };
	let xt = compose_extrinsic!(_chain_api, TEEREX, "call_worker", request);

	// send and watch extrinsic until block is executed
	let block_hash =
		_chain_api.send_extrinsic(xt.hex_encode(), XtStatus::InBlock).unwrap().unwrap();
	info!(
		"Trusted call extrinsic sent and sucessfully included in parentchain block with hash {:?}.",
		block_hash
	);
	info!("Waiting for execution confirmation from enclave...");
	let (events_in, events_out) = channel();
	_chain_api.subscribe_events(events_in).unwrap();

	loop {
		let ret: ProcessedParentchainBlockArgs = _chain_api
			.wait_for_event::<ProcessedParentchainBlockArgs>(
				TEEREX,
				"ProcessedParentchainBlock",
				None,
				&events_out,
			)
			.unwrap();
		info!("Confirmation of ProcessedParentchainBlock received");
		debug!("Expected block Hash: {:?}", block_hash);
		debug!("Confirmed stf block Hash: {:?}", ret.block_hash);
		if ret.block_hash == block_hash {
			return Some(ret.block_hash.encode())
		}
	}
}

fn read_shard(trusted_args: &TrustedArgs) -> StdResult<ShardIdentifier, codec::Error> {
	match &trusted_args.shard {
		Some(s) => match s.from_base58() {
			Ok(s) => ShardIdentifier::decode(&mut &s[..]),
			_ => panic!("shard argument must be base58 encoded"),
		},
		None => match trusted_args.mrenclave.from_base58() {
			Ok(s) => ShardIdentifier::decode(&mut &s[..]),
			_ => panic!("mrenclave argument must be base58 encoded"),
		},
	}
}

/// sends a rpc watch request to the worker api server
fn send_direct_request(
	cli: &Cli,
	trusted_args: &TrustedArgs,
	operation_call: TrustedOperation,
) -> Option<Vec<u8>> {
	let (_operation_call_encoded, operation_call_encrypted) =
		match encode_encrypt(cli, operation_call) {
			Ok((encoded, encrypted)) => (encoded, encrypted),
			Err(msg) => {
				println!("[Error] {}", msg);
				return None
			},
		};
	let shard = read_shard(trusted_args).unwrap();

	// compose jsonrpc call
	let data = Request { shard, cyphertext: operation_call_encrypted };
	let direct_invocation_call = RpcRequest {
		jsonrpc: "2.0".to_owned(),
		method: "author_submitAndWatchExtrinsic".to_owned(),
		params: data.encode(),
		id: 1,
	};
	let jsonrpc_call: String = serde_json::to_string(&direct_invocation_call).unwrap();

	debug!("get direct api");
	let direct_api = get_worker_api_direct(cli);

	debug!("setup sender and receiver");
	let (sender, receiver) = channel();
	direct_api.watch(jsonrpc_call, sender);

	debug!("waiting for rpc response");
	loop {
		match receiver.recv() {
			Ok(response) => {
				debug!("received response");
				let response: RpcResponse = serde_json::from_str(&response).unwrap();
				if let Ok(return_value) = RpcReturnValue::decode(&mut response.result.as_slice()) {
					debug!("successfully decoded rpc response");
					match return_value.status {
						DirectRequestStatus::Error => {
							debug!("request status is error");
							if let Ok(value) = String::decode(&mut return_value.value.as_slice()) {
								println!("[Error] {}", value);
							}
							return None
						},
						DirectRequestStatus::TrustedOperationStatus(status) => {
							debug!("request status is: {:?}", status);
							if let Ok(value) = Hash::decode(&mut return_value.value.as_slice()) {
								println!("Trusted call {:?} is {:?}", value, status);
							}
						},
						_ => {
							debug!("request status is ignored");
							return None
						},
					}
					if !return_value.do_watch {
						debug!("do watch is false, closing connection");
						return None
					}
				};
			},
			Err(e) => {
				error!("failed to receive rpc response: {:?}", e);
				return None
			},
		};
	}
}

#[allow(dead_code)]
#[derive(Decode)]
struct ProcessedParentchainBlockArgs {
	signer: AccountId,
	block_hash: H256,
	merkle_root: H256,
}