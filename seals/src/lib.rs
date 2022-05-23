// BP Core Library implementing LNP/BP specifications & standards related to
// bitcoin protocol
//
// Written in 2020-2022 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the Apache 2.0 License
// along with this software.
// If not, see <https://opensource.org/licenses/Apache-2.0>.

// Coding conventions
#![recursion_limit = "256"]
#![deny(dead_code, /* missing_docs, */ warnings)]

#[macro_use]
extern crate amplify;
#[macro_use]
extern crate strict_encoding;
#[cfg(feature = "async")]
// #[macro_use]
extern crate async_trait;
#[cfg(feature = "serde")]
#[macro_use]
extern crate serde_crate as serde;

pub mod txout_blind;

use std::str::FromStr;

/// Method of single-use-seal closing.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
#[derive(StrictEncode, StrictDecode)]
#[strict_encoding(by_value)]
#[repr(u8)]
#[non_exhaustive]
pub enum TxoutMethod {
    /// Seal is closed over the message in form of OP_RETURN commitment present
    /// in the first OP_RETURN-containing transaction output.
    #[display("opret1st")]
    OpretFirst = 0x00,

    /// Seal is closed over the message in form of Taproot-based OP_RETURN
    /// commitment present in the first Taproot transaction output.
    #[display("tapret1st")]
    TapretFirst = 0x01,
}

/// wrong transaction ouput-based single-use-seal closing method id '{0}'.
#[derive(Clone, PartialEq, Eq, Debug, Display, Error, From)]
#[display(doc_comments)]
pub struct MethodParseError(String);

impl FromStr for TxoutMethod {
    type Err = MethodParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase() {
            s if s == TxoutMethod::OpretFirst.to_string() => {
                TxoutMethod::OpretFirst
            }
            s if s == TxoutMethod::TapretFirst.to_string() => {
                TxoutMethod::TapretFirst
            }
            _ => return Err(MethodParseError(s.to_owned())),
        })
    }
}
