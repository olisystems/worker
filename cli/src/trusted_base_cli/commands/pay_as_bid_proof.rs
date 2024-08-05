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
	trusted_cli::TrustedCli, trusted_command_utils::get_pair_from_str,
	trusted_operation::perform_trusted_operation, Cli, CliResult, CliResultOk,
};

use ita_stf::{Getter, MerkleProofWithCodec, TrustedCallSigned, TrustedGetter};
use itp_stf_primitives::types::{KeyPair, TrustedOperation};
use sp_core::{Pair, H256};

#[derive(Parser)]
pub struct PayAsBidProofCommand {
	/// AccountId in ss58check format
	pub account: String,
	pub timestamp: String,
	pub actor_id: String,
}

impl PayAsBidProofCommand {
	pub(crate) fn run(&self, cli: &Cli, trusted_args: &TrustedCli) -> CliResult {
		pay_as_bid_proof(
			cli,
			trusted_args,
			&self.account,
			self.timestamp.clone(),
			self.actor_id.clone(),
		)
	}
}

pub(crate) fn pay_as_bid_proof(
	cli: &Cli,
	trusted_args: &TrustedCli,
	arg_who: &str,
	timestamp: String,
	actor_id: String,
) -> CliResult {
	let who = get_pair_from_str(trusted_args, arg_who);

	let top: TrustedOperation<TrustedCallSigned, Getter> = Getter::trusted(
		TrustedGetter::pay_as_bid_proof(who.public().into(), timestamp, actor_id)
			.sign(&KeyPair::Sr25519(Box::new(who))),
	)
	.into();

	Ok(perform_trusted_operation::<MerkleProofWithCodec<H256, Vec<u8>>>(cli, trusted_args, &top)
		.map(|proof| {
			let p_string = serde_json::to_string(&proof).unwrap();
			println!("{}", p_string);
			CliResultOk::PayAsBidProofOutput(proof)
		})?)
}
