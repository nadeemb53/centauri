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

use crate::{error::Error, proto, proto::Header as RawHeader};
use alloc::{collections::BTreeMap, vec::Vec};
use anyhow::anyhow;
use codec::Encode;
use grandpa_client_primitives::{FinalityProof, ParachainHeaderProofs};
use ibc::Height;
use primitive_types::H256;
use sp_runtime::traits::BlakeTwo256;
use tendermint_proto::Protobuf;

/// Protobuf type url for GRANDPA header
pub const GRANDPA_HEADER_TYPE_URL: &str = "/ibc.lightclients.grandpa.v1.Header";

/// Relay chain substrate header type
pub type RelayChainHeader = sp_runtime::generic::Header<u32, BlakeTwo256>;

/// Parachain headers with a Grandpa finality proof.
#[derive(Clone, Debug)]
pub struct Header {
	/// The grandpa finality proof: contains relay chain headers from the
	/// last known finalized grandpa block.
	pub finality_proof: FinalityProof<RelayChainHeader>,
	/// Contains a map of relay chain header hashes to parachain headers
	/// finalzed at the relay chain height. We check for this parachain header finalization
	/// via state proofs. Also contains extrinsic proof for timestamp.
	pub parachain_headers: BTreeMap<H256, ParachainHeaderProofs>,
}

impl ibc::core::ics02_client::header::Header for Header {
	fn encode_to_vec(&self) -> Vec<u8> {
		self.encode_vec()
	}

	fn height(&self) -> Height {
		unimplemented!()
	}
}

impl Protobuf<RawHeader> for Header {}

impl TryFrom<RawHeader> for Header {
	type Error = Error;

	fn try_from(raw_header: RawHeader) -> Result<Self, Self::Error> {
		let finality_proof = raw_header
			.finality_proof
			.ok_or_else(|| anyhow!("Grandpa finality proof is required!"))?;
		let block = if finality_proof.block.len() == 32 {
			H256::from_slice(&*finality_proof.block)
		} else {
			Err(anyhow!("Invalid hash type with length: {}", finality_proof.block.len()))?
		};

		let parachain_headers = raw_header
			.parachain_headers
			.into_iter()
			.map(|header| {
				let block = if header.relay_hash.len() == 32 {
					H256::from_slice(&*header.relay_hash)
				} else {
					Err(anyhow!("Invalid hash type with length: {}", header.relay_hash.len()))?
				};
				let proto::ParachainHeaderProofs { state_proof, extrinsic_proof, extrinsic } =
					header
						.parachain_header
						.ok_or_else(|| anyhow!("Parachain header is required!"))?;
				let parachain_header_proofs =
					ParachainHeaderProofs { state_proof, extrinsic, extrinsic_proof };
				Ok((block, parachain_header_proofs))
			})
			.collect::<Result<_, Error>>()?;

		let unknown_headers = finality_proof
			.unknown_headers
			.into_iter()
			.map(|h| {
				let header = codec::Decode::decode(&mut &h[..])?;
				Ok(header)
			})
			.collect::<Result<_, Error>>()?;

		Ok(Self {
			finality_proof: FinalityProof {
				block,
				justification: finality_proof.justification,
				unknown_headers,
			},
			parachain_headers,
		})
	}
}

impl From<Header> for RawHeader {
	fn from(header: Header) -> Self {
		let parachain_headers = header
			.parachain_headers
			.into_iter()
			.map(|(hash, parachain_header_proofs)| proto::ParachainHeaderWithRelayHash {
				relay_hash: hash.as_bytes().to_vec(),
				parachain_header: Some(proto::ParachainHeaderProofs {
					state_proof: parachain_header_proofs.state_proof,
					extrinsic: parachain_header_proofs.extrinsic,
					extrinsic_proof: parachain_header_proofs.extrinsic_proof,
				}),
			})
			.collect();
		let finality_proof = proto::FinalityProof {
			block: header.finality_proof.block.as_bytes().to_vec(),
			justification: header.finality_proof.justification,
			unknown_headers: header
				.finality_proof
				.unknown_headers
				.into_iter()
				.map(|h| h.encode())
				.collect(),
		};
		Self { finality_proof: Some(finality_proof), parachain_headers }
	}
}