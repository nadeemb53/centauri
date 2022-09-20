// Copyright (C) 2022 ComposableFi.
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

use crate::runtime::api::runtime_types::polkadot_parachain::primitives::Id;
use anyhow::anyhow;
use beefy_prover::helpers::{fetch_timestamp_extrinsic_with_proof, TimeStampExtWithProof};
use codec::{Decode, Encode};
use finality_grandpa_rpc::GrandpaApiClient;
use primitives::{
	parachain_header_storage_key, FinalityProof, ParachainHeaderProofs,
	ParachainHeadersWithFinalityProof,
};
use serde::{Deserialize, Serialize};
use sp_core::H256;
use sp_runtime::traits::{Header, Zero};
use std::collections::BTreeMap;
use subxt::{sp_runtime::traits::Header as _, Client, Config};

pub mod host_functions;
pub mod runtime;

pub struct GrandpaProver<T: Config> {
	pub relay_client: Client<T>,
	pub para_client: Client<T>,
	pub para_id: u32,
}

/// An encoded justification proving that the given header has been finalized
#[derive(Clone, Serialize, Deserialize)]
pub struct JustificationNotification(sp_core::Bytes);

impl<T> GrandpaProver<T>
where
	T: Config,
	T::BlockNumber: Ord + Zero,
	u32: From<T::BlockNumber>,
{
	/// Returns the finalized parachain headers in between the given relay chain hashes.
	pub async fn query_finalized_parachain_headers_between(
		&self,
		latest_finalized_hash: T::Hash,
		previous_finalized_hash: T::Hash,
	) -> Result<Vec<T::Header>, anyhow::Error> {
		let api = self
			.relay_client
			.clone()
			.to_runtime_api::<runtime::api::RuntimeApi<T, subxt::PolkadotExtrinsicParams<_>>>();
		let change_set = self
			.relay_client
			.storage()
			.query_storage(
				// we are interested only in the blocks where our parachain header changes.
				vec![parachain_header_storage_key(self.para_id)],
				previous_finalized_hash,
				Some(latest_finalized_hash),
			)
			.await?;

		let mut headers = vec![];
		for changes in change_set {
			let header =
				self.relay_client.rpc().header(Some(changes.block)).await?.ok_or_else(|| {
					anyhow!("[get_parachain_headers] block not found {:?}", changes.block)
				})?;

			let head = api
				.storage()
				.paras()
				.heads(&Id(self.para_id), Some(header.hash()))
				.await?
				.expect("Header exists in its own changeset; qed");

			let para_header = T::Header::decode(&mut &head.0[..])
				.map_err(|_| anyhow!("Failed to decode header"))?;
			headers.push(para_header);
		}

		Ok(headers)
	}

	/// Returns the finality proof for the given parachain header numbers in between the given relay
	/// chain hashes.
	pub async fn query_finalized_parachain_headers_with_proof<H>(
		&self,
		latest_finalized_hash: T::Hash,
		previous_finalized_hash: T::Hash,
		header_numbers: Vec<T::BlockNumber>,
	) -> Result<ParachainHeadersWithFinalityProof<H>, anyhow::Error>
	where
		H: Header,
		H::Hash: From<T::Hash>,
	{
		let header = self
			.relay_client
			.rpc()
			.header(Some(latest_finalized_hash))
			.await?
			.ok_or_else(|| anyhow!("Header not found!"))?;

		let encoded = GrandpaApiClient::<JustificationNotification, H256, u32>::prove_finality(
			&*self.relay_client.rpc().client,
			u32::from(*header.number()),
		)
		.await?
		.ok_or_else(|| anyhow!("No justification found for block: {:?}", header.hash()))?
		.0;
		let mut finality_proof = FinalityProof::<H>::decode(&mut &encoded[..])?;
		finality_proof.unknown_headers = {
			let mut unknown_headers = vec![H::decode(&mut &header.encode()[..])?];
			let mut current = *header.parent_hash();
			loop {
				if current == previous_finalized_hash {
					break
				}
				let header = self
					.relay_client
					.rpc()
					.header(Some(current))
					.await?
					.ok_or_else(|| anyhow!("Header with hash: {current:?} not found!"))?;
				current = *header.parent_hash();
				unknown_headers.push(H::decode(&mut &header.encode()[..])?);
			}
			unknown_headers
		};

		// we are interested only in the blocks where our parachain header changes.
		let keys = vec![parachain_header_storage_key(self.para_id)];
		let change_set = self
			.relay_client
			.storage()
			.query_storage(keys.clone(), previous_finalized_hash, Some(latest_finalized_hash))
			.await?;

		let api = self
			.relay_client
			.clone()
			.to_runtime_api::<runtime::api::RuntimeApi<T, subxt::PolkadotExtrinsicParams<_>>>();
		let mut parachain_headers = BTreeMap::<H::Hash, ParachainHeaderProofs>::default();

		for changes in change_set {
			let header = self
				.relay_client
				.rpc()
				.header(Some(changes.block))
				.await?
				.ok_or_else(|| anyhow!("block not found {:?}", changes.block))?;

			let parachain_header_bytes = api
				.storage()
				.paras()
				.heads(&Id(self.para_id), Some(header.hash()))
				.await?
				.expect("Header exists in its own changeset; qed")
				.0;

			let para_header: T::Header = Decode::decode(&mut &parachain_header_bytes[..])?;
			let para_block_number = *para_header.number();
			// skip genesis header or any unknown headers
			if para_block_number == Zero::zero() || !header_numbers.contains(&para_block_number) {
				continue
			}

			let state_proof = self
				.relay_client
				.rpc()
				.read_proof(keys.clone(), Some(header.hash()))
				.await?
				.proof
				.into_iter()
				.map(|p| p.0)
				.collect();

			let TimeStampExtWithProof { ext: extrinsic, proof: extrinsic_proof } =
				fetch_timestamp_extrinsic_with_proof(&self.para_client, Some(para_header.hash()))
					.await
					.map_err(|err| anyhow!("Error fetching timestamp with proof: {err:?}"))?;
			let proofs = ParachainHeaderProofs { state_proof, extrinsic, extrinsic_proof };
			parachain_headers.insert(header.hash().into(), proofs);
		}

		Ok(ParachainHeadersWithFinalityProof { finality_proof, parachain_headers })
	}
}