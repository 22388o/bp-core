// Bitcoin protocol primitives library.
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2019-2023 by
//     Dr Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// Copyright (C) 2019-2023 LNP/BP Standards Association. All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(unused_braces)] // required due to strict dumb derivation and compiler bug

use std::borrow::Borrow;
use std::fmt::{self, Formatter, LowerHex, UpperHex};
use std::ops::BitXor;
use std::{cmp, io, slice, vec};

use amplify::confinement::{Confined, U32};
use amplify::{confinement, Bytes32, Wrapper};
use commit_verify::{DigestExt, Sha256};
use secp256k1::{Scalar, XOnlyPublicKey};
use strict_encoding::{
    DecodeError, ReadTuple, StrictDecode, StrictEncode, StrictProduct, StrictTuple, StrictType,
    TypeName, TypedRead, TypedWrite, WriteTuple,
};

use crate::opcodes::*;
use crate::{ScriptBytes, ScriptPubkey, WitnessVer, LIB_NAME_BITCOIN};

/// The SHA-256 midstate value for the TapLeaf hash.
const MIDSTATE_TAPLEAF: [u8; 7] = *b"TapLeaf";
// 9ce0e4e67c116c3938b3caf2c30f5089d3f3936c47636e607db33eeaddc6f0c9

/// The SHA-256 midstate value for the TapBranch hash.
const MIDSTATE_TAPBRANCH: [u8; 9] = *b"TapBranch";
// 23a865a9b8a40da7977c1e04c49e246fb5be13769d24c9b7b583b5d4a8d226d2

/// The SHA-256 midstate value for the TapTweak hash.
const MIDSTATE_TAPTWEAK: [u8; 8] = *b"TapTweak";
// d129a2f3701c655d6583b6c3b941972795f4e23294fd54f4a2ae8d8547ca590b

/// The SHA-256 midstate value for the TapSig hash.
#[warn(dead_code)]
const MIDSTATE_TAPSIGHASH: [u8; 10] = *b"TapSighash";
// f504a425d7f8783b1363868ae3e556586eee945dbc7888dd02a6e2c31873fe9f

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display, Error)]
#[display("invalid public key")]
pub struct InvalidPubkey;

#[derive(Wrapper, WrapperMut, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
#[wrapper(Deref, LowerHex, Display, FromStr)]
#[wrapper_mut(DerefMut)]
#[derive(StrictType, StrictDumb)]
#[strict_type(lib = LIB_NAME_BITCOIN, dumb = { Self(XOnlyPublicKey::from_slice(&[1u8; 32]).unwrap()) })]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct InternalPk(XOnlyPublicKey);

impl InternalPk {
    pub fn from_byte_array(data: [u8; 32]) -> Result<Self, InvalidPubkey> {
        XOnlyPublicKey::from_slice(data.as_ref())
            .map(Self)
            .map_err(|_| InvalidPubkey)
    }

    pub fn to_byte_array(&self) -> [u8; 32] { self.0.serialize() }

    pub fn to_output_key(&self, merkle_root: Option<impl IntoTapHash>) -> XOnlyPublicKey {
        let mut engine = Sha256::from_tag(MIDSTATE_TAPTWEAK);
        // always hash the key
        engine.input_raw(&self.0.serialize());
        if let Some(merkle_root) = merkle_root {
            engine.input_raw(merkle_root.into_tap_hash().as_slice());
        }
        let tweak =
            Scalar::from_be_bytes(engine.finish()).expect("hash value greater than curve order");
        let (output_key, tweaked_parity) = self
            .0
            .add_tweak(secp256k1::SECP256K1, &tweak)
            .expect("hash collision");
        debug_assert!(self.tweak_add_check(
            secp256k1::SECP256K1,
            &output_key,
            tweaked_parity,
            tweak
        ));
        output_key
    }
}

impl StrictEncode for InternalPk {
    fn strict_encode<W: TypedWrite>(&self, writer: W) -> io::Result<W> {
        let bytes = Bytes32::from(self.0.serialize());
        writer.write_newtype::<Self>(&bytes)
    }
}

impl StrictDecode for InternalPk {
    fn strict_decode(reader: &mut impl TypedRead) -> Result<Self, DecodeError> {
        reader.read_tuple(|r| {
            let bytes: Bytes32 = r.read_field()?;
            XOnlyPublicKey::from_slice(bytes.as_slice())
                .map(Self)
                .map_err(|_| {
                    DecodeError::DataIntegrityError(format!(
                        "invalid x-only public key value '{bytes:x}'"
                    ))
                })
        })
    }
}

pub trait IntoTapHash {
    fn into_tap_hash(self) -> TapNodeHash;
}

#[derive(Wrapper, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
#[wrapper(Index, RangeOps, BorrowSlice, Hex, Display, FromStr)]
#[derive(StrictType, StrictEncode, StrictDecode, StrictDumb)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct TapLeafHash(
    #[from]
    #[from([u8; 32])]
    Bytes32,
);

impl TapLeafHash {
    pub fn with_leaf_script(leaf_script: &LeafScript) -> Self {
        let mut engine = Sha256::from_tag(MIDSTATE_TAPLEAF);
        engine.input_raw(&[leaf_script.version.to_consensus_u8()]);
        engine.input_with_len::<U32>(leaf_script.script.as_slice());
        Self(engine.finish().into())
    }

    pub fn with_tap_script(tap_script: &TapScript) -> Self {
        let mut engine = Sha256::from_tag(MIDSTATE_TAPLEAF);
        engine.input_raw(&[TAPROOT_LEAF_TAPSCRIPT]);
        engine.input_with_len::<U32>(tap_script.as_slice());
        Self(engine.finish().into())
    }
}

impl IntoTapHash for TapLeafHash {
    fn into_tap_hash(self) -> TapNodeHash { TapNodeHash(self.0) }
}

#[derive(Wrapper, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
#[wrapper(Index, RangeOps, BorrowSlice, Hex, Display, FromStr)]
#[derive(StrictType, StrictEncode, StrictDecode, StrictDumb)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct TapBranchHash(
    #[from]
    #[from([u8; 32])]
    Bytes32,
);

impl TapBranchHash {
    pub fn with_nodes(node1: TapNodeHash, node2: TapNodeHash) -> Self {
        let mut engine = Sha256::from_tag(MIDSTATE_TAPBRANCH);
        engine.input_raw(cmp::min(&node1, &node2).borrow());
        engine.input_raw(cmp::max(&node1, &node2).borrow());
        Self(engine.finish().into())
    }
}

impl IntoTapHash for TapBranchHash {
    fn into_tap_hash(self) -> TapNodeHash { TapNodeHash(self.0) }
}

#[derive(Wrapper, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
#[wrapper(Deref, Index, RangeOps, BorrowSlice, Hex, Display, FromStr)]
#[derive(StrictType, StrictDumb, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct TapNodeHash(
    #[from]
    #[from([u8; 32])]
    #[from(TapLeafHash)]
    #[from(TapBranchHash)]
    Bytes32,
);

impl IntoTapHash for TapNodeHash {
    fn into_tap_hash(self) -> TapNodeHash { self }
}

#[derive(Wrapper, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From, Default)]
#[wrapper(Deref)]
#[derive(StrictType, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct TapMerklePath(Confined<Vec<TapBranchHash>, 0, 128>);

impl IntoIterator for TapMerklePath {
    type Item = TapBranchHash;
    type IntoIter = vec::IntoIter<TapBranchHash>;

    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

impl<'a> IntoIterator for &'a TapMerklePath {
    type Item = &'a TapBranchHash;
    type IntoIter = slice::Iter<'a, TapBranchHash>;

    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

impl TapMerklePath {
    /// Tries to construct a confinement over a collection. Fails if the number
    /// of items in the collection exceeds one of the confinement bounds.
    // We can't use `impl TryFrom` due to the conflict with core library blanked
    // implementation
    #[inline]
    pub fn try_from(path: Vec<TapBranchHash>) -> Result<Self, confinement::Error> {
        Confined::try_from(path).map(Self::from_inner)
    }

    /// Tries to construct a confinement with a collection of elements taken
    /// from an iterator. Fails if the number of items in the collection
    /// exceeds one of the confinement bounds.
    #[inline]
    pub fn try_from_iter<I: IntoIterator<Item = TapBranchHash>>(
        iter: I,
    ) -> Result<Self, confinement::Error> {
        Confined::try_from_iter(iter).map(Self::from_inner)
    }
}

/// Taproot annex prefix.
pub const TAPROOT_ANNEX_PREFIX: u8 = 0x50;

/// Tapscript leaf version.
// https://github.com/bitcoin/bitcoin/blob/e826b22da252e0599c61d21c98ff89f366b3120f/src/script/interpreter.h#L226
pub const TAPROOT_LEAF_TAPSCRIPT: u8 = 0xc0;

/// Tapleaf mask for getting the leaf version from first byte of control block.
// https://github.com/bitcoin/bitcoin/blob/e826b22da252e0599c61d21c98ff89f366b3120f/src/script/interpreter.h#L225
pub const TAPROOT_LEAF_MASK: u8 = 0xfe;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Display, Error)]
#[display(doc_comments)]
/// invalid taproot leaf version {0}.
pub struct InvalidLeafVer(u8);

/// The leaf version for tapleafs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", rename_all = "camelCase")
)]
pub enum LeafVer {
    /// BIP-342 tapscript.
    #[default]
    TapScript,

    /// Future leaf version.
    Future(FutureLeafVer),
}

impl StrictType for LeafVer {
    const STRICT_LIB_NAME: &'static str = LIB_NAME_BITCOIN;
    fn strict_name() -> Option<TypeName> { Some(tn!("LeafVer")) }
}
impl StrictProduct for LeafVer {}
impl StrictTuple for LeafVer {
    const FIELD_COUNT: u8 = 1;
}
impl StrictEncode for LeafVer {
    fn strict_encode<W: TypedWrite>(&self, writer: W) -> std::io::Result<W> {
        writer.write_tuple::<Self>(|w| Ok(w.write_field(&self.to_consensus_u8())?.complete()))
    }
}
impl StrictDecode for LeafVer {
    fn strict_decode(reader: &mut impl TypedRead) -> Result<Self, DecodeError> {
        reader.read_tuple(|r| {
            let version = r.read_field()?;
            Self::from_consensus_u8(version)
                .map_err(|err| DecodeError::DataIntegrityError(err.to_string()))
        })
    }
}

impl LeafVer {
    #[doc(hidden)]
    #[deprecated(since = "0.10.9", note = "use from_consensus_u8")]
    pub fn from_consensus(version: u8) -> Result<Self, InvalidLeafVer> {
        Self::from_consensus_u8(version)
    }

    /// Creates a [`LeafVer`] from consensus byte representation.
    ///
    /// # Errors
    ///
    /// - If the last bit of the `version` is odd.
    /// - If the `version` is 0x50 ([`TAPROOT_ANNEX_PREFIX`]).
    pub fn from_consensus_u8(version: u8) -> Result<Self, InvalidLeafVer> {
        match version {
            TAPROOT_LEAF_TAPSCRIPT => Ok(LeafVer::TapScript),
            TAPROOT_ANNEX_PREFIX => Err(InvalidLeafVer(TAPROOT_ANNEX_PREFIX)),
            future => FutureLeafVer::from_consensus(future).map(LeafVer::Future),
        }
    }

    #[doc(hidden)]
    #[deprecated(since = "0.10.9", note = "use to_consensus_u8")]
    pub fn to_consensus(self) -> u8 { self.to_consensus_u8() }

    /// Returns the consensus representation of this [`LeafVer`].
    pub fn to_consensus_u8(self) -> u8 {
        match self {
            LeafVer::TapScript => TAPROOT_LEAF_TAPSCRIPT,
            LeafVer::Future(version) => version.to_consensus(),
        }
    }
}

impl LowerHex for LeafVer {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result { LowerHex::fmt(&self.to_consensus_u8(), f) }
}

impl UpperHex for LeafVer {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result { UpperHex::fmt(&self.to_consensus_u8(), f) }
}

/// Inner type representing future (non-tapscript) leaf versions. See
/// [`LeafVer::Future`].
///
/// NB: NO PUBLIC CONSTRUCTOR!
/// The only way to construct this is by converting `u8` to [`LeafVer`] and then
/// extracting it.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(StrictType, StrictDumb, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN, dumb = { Self(0x51) })]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(crate = "serde_crate"))]
pub struct FutureLeafVer(u8);

impl FutureLeafVer {
    pub(self) fn from_consensus(version: u8) -> Result<FutureLeafVer, InvalidLeafVer> {
        match version {
            TAPROOT_LEAF_TAPSCRIPT => unreachable!(
                "FutureLeafVersion::from_consensus should be never called for 0xC0 value"
            ),
            TAPROOT_ANNEX_PREFIX => Err(InvalidLeafVer(TAPROOT_ANNEX_PREFIX)),
            odd if odd & 0xFE != odd => Err(InvalidLeafVer(odd)),
            even => Ok(FutureLeafVer(even)),
        }
    }

    /// Returns the consensus representation of this [`FutureLeafVer`].
    #[inline]
    pub fn to_consensus(self) -> u8 { self.0 }
}

impl LowerHex for FutureLeafVer {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { LowerHex::fmt(&self.0, f) }
}

impl UpperHex for FutureLeafVer {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { UpperHex::fmt(&self.0, f) }
}

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Default, Display)]
#[derive(StrictType, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(crate = "serde_crate"))]
#[display("{version:04x} {script:x}")]
pub struct LeafScript {
    pub version: LeafVer,
    pub script: ScriptBytes,
}

// TODO: Impl Hex and FromStr for LeafScript

impl From<TapScript> for LeafScript {
    fn from(tap_script: TapScript) -> Self {
        LeafScript {
            version: LeafVer::TapScript,
            script: tap_script.into_inner(),
        }
    }
}

impl LeafScript {
    #[inline]
    pub fn new(version: LeafVer, script: ScriptBytes) -> Self { LeafScript { version, script } }
    #[inline]
    pub fn with_bytes(version: LeafVer, script: Vec<u8>) -> Result<Self, confinement::Error> {
        Ok(LeafScript {
            version,
            script: ScriptBytes::from(script),
        })
    }
    #[inline]
    pub fn from_tap_script(tap_script: TapScript) -> Self { Self::from(tap_script) }
    #[inline]
    pub fn tap_leaf_hash(&self) -> TapLeafHash { TapLeafHash::with_leaf_script(self) }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Display)]
#[derive(StrictType, StrictDumb, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN, tags = repr, into_u8, try_from_u8)]
#[repr(u8)]
#[non_exhaustive]
pub enum TapCode {
    /// Push the next 32 bytes as an array onto the stack.
    #[display("OP_PUSH_BYTES32")]
    PushBytes32 = OP_PUSHBYTES_32,

    /// Synonym for OP_RETURN.
    Reserved = OP_RESERVED,

    /// Fail the script immediately.
    #[display("OP_RETURN")]
    #[strict_type(dumb)]
    Return = OP_RETURN,

    /// Read the next byte as N; push the next N bytes as an array onto the
    /// stack.
    #[display("OP_PUSH_DATA1")]
    PushData1 = OP_PUSHDATA1,
    /// Read the next 2 bytes as N; push the next N bytes as an array onto the
    /// stack.
    #[display("OP_PUSH_DATA2")]
    PushData2 = OP_PUSHDATA2,
    /// Read the next 4 bytes as N; push the next N bytes as an array onto the
    /// stack.
    #[display("OP_PUSH_DATA3")]
    PushData4 = OP_PUSHDATA4,
}

#[derive(Wrapper, WrapperMut, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From, Default)]
#[wrapper(Deref, Index, RangeOps, BorrowSlice, LowerHex, UpperHex)]
#[wrapper_mut(DerefMut, IndexMut, RangeMut, BorrowSliceMut)]
#[derive(StrictType, StrictEncode, StrictDecode)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", transparent)
)]
pub struct TapScript(ScriptBytes);
// TODO: impl Display/FromStr for TapScript providing correct opcodes

impl TapScript {
    pub fn new() -> Self { Self::default() }

    pub fn with_capacity(capacity: usize) -> Self {
        Self(ScriptBytes::from(Confined::with_capacity(capacity)))
    }

    /// Adds a single opcode to the script.
    pub fn push_opcode(&mut self, op_code: TapCode) { self.0.push(op_code as u8); }

    pub fn as_script_bytes(&self) -> &ScriptBytes { &self.0 }
}

impl ScriptPubkey {
    pub fn p2tr(internal_key: InternalPk, merkle_root: Option<impl IntoTapHash>) -> Self {
        let output_key = internal_key.to_output_key(merkle_root);
        Self::p2tr_tweaked(output_key)
    }

    pub fn p2tr_key_only(internal_key: InternalPk) -> Self {
        let output_key = internal_key.to_output_key(None::<TapNodeHash>);
        Self::p2tr_tweaked(output_key)
    }

    pub fn p2tr_scripted(internal_key: InternalPk, merkle_root: impl IntoTapHash) -> Self {
        let output_key = internal_key.to_output_key(Some(merkle_root));
        Self::p2tr_tweaked(output_key)
    }

    pub fn p2tr_tweaked(output_key: XOnlyPublicKey) -> Self {
        // output key is 32 bytes long, so it's safe to use
        // `new_witness_program_unchecked` (Segwitv1)
        Self::with_witness_program_unchecked(WitnessVer::V1, &output_key.serialize())
    }

    pub fn is_p2tr(&self) -> bool {
        self.len() == 34 && self[0] == WitnessVer::V1.op_code() as u8 && self[1] == OP_PUSHBYTES_32
    }
}

/// invalid parity value {0} - must be 0 or 1
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Display, Error)]
#[display(doc_comments)]
pub struct InvalidParityValue(pub u8);

/// Represents the parity passed between FFI function calls.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Display)]
#[display(lowercase)]
#[derive(StrictType, StrictEncode, StrictDecode, StrictDumb)]
#[strict_type(lib = LIB_NAME_BITCOIN, tags = repr, into_u8, try_from_u8)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", rename_all = "camelCase")
)]
#[repr(u8)]
pub enum Parity {
    /// Even parity.
    #[strict_type(dumb)]
    Even = 0,
    /// Odd parity.
    Odd = 1,
}

impl Parity {
    /// Converts parity into an integer (byte) value.
    ///
    /// This returns `0` for even parity and `1` for odd parity.
    pub fn to_consensus_u8(self) -> u8 { self as u8 }

    /// Constructs a [`Parity`] from a byte.
    ///
    /// The only allowed values are `0` meaning even parity and `1` meaning odd.
    /// Other values result in error being returned.
    pub fn from_consensus_u8(parity: u8) -> Result<Parity, InvalidParityValue> {
        match parity {
            0 => Ok(Parity::Even),
            1 => Ok(Parity::Odd),
            invalid => Err(InvalidParityValue(invalid)),
        }
    }
}

/// Returns even parity if the operands are equal, odd otherwise.
impl BitXor for Parity {
    type Output = Parity;

    fn bitxor(self, rhs: Parity) -> Self::Output {
        // This works because Parity has only two values (i.e. only 1 bit of
        // information).
        if self == rhs {
            Parity::Even // 1^1==0 and 0^0==0
        } else {
            Parity::Odd // 1^0==1 and 0^1==1
        }
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
#[derive(StrictType, StrictEncode, StrictDecode, StrictDumb)]
#[strict_type(lib = LIB_NAME_BITCOIN)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", rename_all = "camelCase")
)]
pub struct ControlBlock {
    /// The tapleaf version.
    pub leaf_version: LeafVer,
    /// The parity of the output key (NOT THE INTERNAL KEY WHICH IS ALWAYS
    /// XONLY).
    pub output_key_parity: Parity,
    /// The internal key.
    pub internal_key: InternalPk,
    /// The merkle proof of a script associated with this leaf.
    pub merkle_branch: TapMerklePath,
}
