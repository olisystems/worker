/*
	Copyright 2019 Supercomputing Systems AG

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

#![crate_name = "substratee_worker_enclave"]
#![crate_type = "staticlib"]

#![cfg_attr(not(target_env = "sgx"), no_std)]
#![cfg_attr(target_env = "sgx", feature(rustc_private))]

extern crate crypto;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate my_node_runtime;
extern crate parity_codec;
extern crate primitive_types;
extern crate primitives;
extern crate runtime_primitives;
extern crate rust_base58;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate sgx_crypto_helper;
extern crate sgx_rand;
extern crate sgx_serialize;
#[macro_use]
extern crate sgx_serialize_derive;
extern crate sgx_tseal;
#[cfg(not(target_env = "sgx"))]
#[macro_use]
extern crate sgx_tstd as std;
extern crate sgx_types;
extern crate sgxwasm;
extern crate wasmi;

use crypto::ed25519::{keypair, signature};
use my_node_runtime::{Call, Hash, SubstraTEEProxyCall, UncheckedExtrinsic};
use parity_codec::{Compact, Decode, Encode};
use primitive_types::U256;
use primitives::ed25519;
use runtime_primitives::generic::Era;
use rust_base58::ToBase58;
use sgx_crypto_helper::rsa3072::{Rsa3072KeyPair};
use sgx_crypto_helper::RsaKeyPair;
use sgx_serialize::{DeSerializeHelper, SerializeHelper};
use sgx_tseal::SgxSealedData;
use sgx_types::{sgx_sealed_data_t, sgx_sha256_hash_t, sgx_status_t};
use sgx_types::marker::ContiguousMemory;
use wasmi::{ImportsBuilder, Module, ModuleInstance, NopExternals, RuntimeValue};

use constants::{COUNTERSTATE, ED25519_SEALED_KEY_FILE, RSA3072_SEALED_KEY_FILE};
use std::collections::HashMap;
use std::sgxfs::SgxFile;
use std::slice;
use std::string::String;
use std::string::ToString;
use std::vec::Vec;

mod constants;
mod utils;
mod wasm;

// FIXME: Log does not work in enclave

#[no_mangle]
pub extern "C" fn get_rsa_encryption_pubkey(pubkey: *mut u8, pubkey_size: u32) -> sgx_status_t {

	if let Err(x) = SgxFile::open(RSA3072_SEALED_KEY_FILE) {
		info!("[Enclave] Keyfile not found, creating new! {}", x);
		if let Err(status) = create_sealed_rsa3072_keypair() {
			return status
		}
	}

	let rsa_pubkey=match  utils::read_rsa_pubkey() {
		Ok(key) => key,
		Err(status) => return status,
	};

	let rsa_pubkey_json = match serde_json::to_string(&rsa_pubkey) {
		Ok(k) => k,
		Err(x) => {
			println!("[Enclave] can't serialize rsa_pubkey {:?} {}", rsa_pubkey, x);
			return sgx_status_t::SGX_ERROR_UNEXPECTED;
		}
	};

	let pubkey_slice = unsafe { slice::from_raw_parts_mut(pubkey, pubkey_size as usize) };

	// split the pubkey_slice at the length of the rsa_pubkey_json
	// and fill the right side with whitespace so that the json can be decoded later on
	let (left, right) = pubkey_slice.split_at_mut(rsa_pubkey_json.len());
	left.clone_from_slice(rsa_pubkey_json.as_bytes());
	right.iter_mut().for_each(|x| *x = 0x20);

	sgx_status_t::SGX_SUCCESS
}

fn create_sealed_rsa3072_keypair() -> Result<sgx_status_t, sgx_status_t> {
	let rsa_keypair = Rsa3072KeyPair::new().unwrap();
	let rsa_key_json = serde_json::to_string(&rsa_keypair).unwrap();
	// println!("[Enclave] generated RSA3072 key pair. Cleartext: {}", rsa_key_json);
	utils::write_file(rsa_key_json.as_bytes(), RSA3072_SEALED_KEY_FILE)
}

#[no_mangle]
pub extern "C" fn get_ecc_signing_pubkey(pubkey: *mut u8, pubkey_size: u32) -> sgx_status_t {
	match SgxFile::open(ED25519_SEALED_KEY_FILE) {
		Ok(_k) => (),
		Err(x) => {
			info!("[Enclave] Keyfile not found, creating new! {}", x);
			if let Err(status) = utils::create_sealed_ed25519_seed() {
				return status;
			}
		},
	}

	let _seed = match utils::_get_ecc_seed_file() {
		Ok(seed) => seed,
		Err(status) => return status,
	};

	let (_privkey, _pubkey) = keypair(&_seed);
	info!("[Enclave] Restored ECC pubkey: {:?}", _pubkey.to_base58());

	let pubkey_slice = unsafe { slice::from_raw_parts_mut(pubkey, pubkey_size as usize) };
	pubkey_slice.clone_from_slice(&_pubkey);

	sgx_status_t::SGX_SUCCESS
}

#[no_mangle]
pub extern "C" fn call_counter_wasm(req_bin: *const u8,
									req_length: usize,
									ciphertext: *mut u8,
									ciphertext_size: u32,
									hash: *const u8,
									hash_size: u32,
									nonce: *const u8,
									nonce_size: u32,
									wasm_hash: *const u8,
									wasm_hash_size: u32,
									unchecked_extrinsic: *mut u8,
									unchecked_extrinsic_size: u32
) -> sgx_status_t {
	#[derive(Debug, Serialize, Deserialize)]
	struct Message {
		account: String,
		amount: u32,
		sha256: sgx_sha256_hash_t
	}

	let ciphertext_slice = unsafe { slice::from_raw_parts(ciphertext, ciphertext_size as usize) };
	let hash_slice = unsafe { slice::from_raw_parts(hash, hash_size as usize) };
	let mut nonce_slice = unsafe { slice::from_raw_parts(nonce, nonce_size as usize) };
	let extrinsic_slice = unsafe { slice::from_raw_parts_mut(unchecked_extrinsic, unchecked_extrinsic_size as usize) };

	debug!("[Enclave] Read RSA keypair");
	let rsa_keypair = match utils::read_rsa_keypair() {
		Ok(pair) => pair,
		Err(status) => return status,
	};

	debug!("[Enclave] Read RSA keypair done");

	// decode the payload
	println!("    [Enclave] Decode the payload");
	let plaintext_vec = utils::decode_payload(&ciphertext_slice, &rsa_keypair);
	let plaintext_string = String::from_utf8(plaintext_vec.clone()).unwrap();
	let message: Message = serde_json::from_str(&plaintext_string).unwrap();

	// get the elements
	let account = message.account;
	let increment = message.amount;
	let sha256 = message.sha256;
	println!("    [Enclave] Message decoded:");
	println!("    [Enclave]   account   = {}", account);
	println!("    [Enclave]   increment = {}", increment);
	println!("    [Enclave]   sha256    = {:?}", sha256);

	// get the calculated SHA256 hash
	let wasm_hash_slice = unsafe { slice::from_raw_parts(wasm_hash, wasm_hash_size as usize) };
	let wasm_hash_calculated: sgx_sha256_hash_t = serde_json::from_slice(wasm_hash_slice).unwrap();

	// compare the hashes and return error if not matching
	if wasm_hash_calculated != sha256 {
		println!("    [Enclave] SHA256 of WASM code not matching");
		println!("    [Enclave]   Wanted by client    : {:?}", sha256);
		println!("    [Enclave]   Calculated by worker: {:?}", wasm_hash_calculated);
		println!("    [Enclave] Returning ERROR_UNEXPECTED and not updating STF");
		return sgx_status_t::SGX_ERROR_UNEXPECTED;
	} else {
		println!("    [Enclave] SHA256 of WASM code identical");
	}

	let state = match utils::read_counterstate(COUNTERSTATE) {
		Ok(state) => state,
		Err(status) => return status,
	};

	let helper = DeSerializeHelper::<AllCounts>::new(state);
	let mut counter = helper.decode().unwrap();

	// get the current counter value of the account or initialize with 0
	let counter_value_old: u32 = *counter.entries.entry(account.to_string()).or_insert(0);
	info!("    [Enclave] Current counter state of '{}' = {}", account, counter_value_old);

	println!("    [Enclave] Executing WASM code");
	let req_slice = unsafe { slice::from_raw_parts(req_bin, req_length) };
	let action_req: sgxwasm::SgxWasmAction = serde_json::from_slice(req_slice).unwrap();

	match action_req {
		sgxwasm::SgxWasmAction::Call { module, function } => {
			let _module = Module::from_buffer(module.unwrap()).unwrap();
			let instance =
				ModuleInstance::new(
					&_module,
					&ImportsBuilder::default()
				)
					.expect("failed to instantiate wasm module")
					.assert_no_start();

			let args = vec![RuntimeValue::I32(counter_value_old as i32),
							RuntimeValue::I32(increment as i32)
			];
			debug!("    [Enclave] Calling WASM with arguments = {:?}", args);

			let r = instance.invoke_export(&function, &args, &mut NopExternals);
			debug!("    [Enclave] invoke_export successful. r = {:?}", r);

			match r {
				Ok(Some(RuntimeValue::I32(v))) => {
					info!("    [Enclave] Add {} to '{}'", v, account);
					counter.entries.insert(account.to_string(), v as u32);
					println!("    [Enclave] WASM executed and counter updated");
				},
				_ => {
					error!("    [Enclave] Could not decode result");
				}
			};
		},
		_ => {
			error!("    [Enclave] Unsupported action");
		},
	}

	// write the counter state
	if let Err(status) = write_counter_state(counter) {
		return status;
	}

	// get information for composing the extrinsic
	let _seed = match utils::_get_ecc_seed_file() {
		Ok(seed) => seed,
		Err(status) => return status,
	};
	let nonce = U256::decode(&mut nonce_slice).unwrap();
	let genesis_hash = utils::hash_from_slice(hash_slice);
	let call_hash = utils::blake2s(&plaintext_vec);
	debug!("[Enclave]: Call hash {:?}", call_hash);

	let ex = compose_extrinsic(_seed, &call_hash, nonce, genesis_hash);

	let encoded = ex.encode();
	extrinsic_slice.clone_from_slice(&encoded);
	sgx_status_t::SGX_SUCCESS
}

#[no_mangle]
pub extern "C" fn get_counter(account: *const u8, account_size: u32, value: *mut u32) -> sgx_status_t {
	let account_slice = unsafe { slice::from_raw_parts(account, account_size as usize) };
	let acc_str = std::str::from_utf8(account_slice).unwrap();

	let state_vec = match utils::read_counterstate(COUNTERSTATE) {
		Ok(state) => state,
		Err(status) => return status,
	};

	let helper = DeSerializeHelper::<AllCounts>::new(state_vec);
	let mut counter = helper.decode().unwrap();
	unsafe {
		let ref_mut = &mut *value;
		*ref_mut = *counter.entries.entry(acc_str.to_string()).or_insert(0);
	}
	sgx_status_t::SGX_SUCCESS
}

fn write_counter_state(value: AllCounts) -> Result<sgx_status_t, sgx_status_t> {
	let helper = SerializeHelper::new();
	let c = helper.encode(value).unwrap();
	utils::write_file(&c, COUNTERSTATE)
}

#[no_mangle]
pub extern "C" fn sign(sealed_seed: *mut u8, sealed_seed_size: u32,
					   msg: *mut u8, msg_size: u32,
					   sig: *mut u8, sig_size: u32) -> sgx_status_t {

	// runseal seed
	let opt = from_sealed_log::<[u8; 32]>(sealed_seed, sealed_seed_size);
	let sealed_data = match opt {
		Some(x) => x,
		None => {
			return sgx_status_t::SGX_ERROR_INVALID_PARAMETER;
		},
	};

	let result = sealed_data.unseal_data();
	let unsealed_data = match result {
		Ok(x) => x,
		Err(ret) => {
			return ret;
		},
	};

	let seed = unsealed_data.get_decrypt_txt();

	//restore ed25519 keypair from seed
	let (_privkey, _pubkey) = keypair(seed);

	info!("[Enclave]: Restored sealed key pair with pubkey: {:?}", _pubkey.to_base58());

	// sign message
	let msg_slice = unsafe { slice::from_raw_parts_mut(msg, msg_size as usize) };
	let sig_slice = unsafe { slice::from_raw_parts_mut(sig, sig_size as usize) };
	let _sig = signature(&msg_slice, &_privkey);
	sig_slice.clone_from_slice(&_sig);

	sgx_status_t::SGX_SUCCESS
}

#[derive(Serializable, DeSerializable, Debug)]
struct AllCounts {
	entries: HashMap<String, u32>
}

fn from_sealed_log<'a, T: Copy + ContiguousMemory>(sealed_log: *mut u8, sealed_log_size: u32) -> Option<SgxSealedData<'a, T>> {
	unsafe {
		SgxSealedData::<T>::from_raw_sealed_data_t(sealed_log as *mut sgx_sealed_data_t, sealed_log_size)
	}
}

pub fn compose_extrinsic(seed: Vec<u8>, call_hash: &[u8], nonce: U256, genesis_hash: Hash) -> UncheckedExtrinsic {
	let (_privkey, _pubkey) = keypair(&seed);

	let era = Era::immortal();
	let function = Call::SubstraTEEProxy(SubstraTEEProxyCall::confirm_call(call_hash.to_vec()));

	let index = nonce.low_u64();
	let raw_payload = (Compact(index), function, era, genesis_hash);

	let sign = raw_payload.using_encoded(|payload| if payload.len() > 256 {
		// should not be thrown as we calculate a 32 byte hash ourselves
		error!("unsupported payload size");
		signature(&[0u8; 64], &_privkey)
	} else {
		//println!("signing {}", HexDisplay::from(&payload));
		signature(payload, &_privkey)
	});

	let signerpub = ed25519::Public::from_raw(_pubkey);
	let signature = ed25519::Signature::from_raw(sign);

	UncheckedExtrinsic::new_signed(
		index,
		raw_payload.1,
		signerpub.into(),
		signature,
		era,
	)
}

