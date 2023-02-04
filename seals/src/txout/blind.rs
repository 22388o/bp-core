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

//! TxOut seals which are blinded with additional entropy.

use std::convert::{TryFrom, TryInto};
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use amplify::hex::FromHex;
use amplify::{hex, Bytes32, Wrapper};
use bp::{Outpoint, Sha256, Txid, Vout};
use commit_verify::{CommitVerify, Conceal};
use dbc::tapret::Lnpbp12;
use rand::{thread_rng, RngCore};

use super::{CloseMethod, MethodParseError, WitnessVoutError};
use crate::txout::{ExplicitSeal, TxoSeal};

/// Revealed seal definition which may point to a witness transactions and
/// contains blinding data.
///
/// Revealed seal means that the seal definition containing explicit information
/// about the bitcoin transaction output.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[derive(StrictType, StrictDumb, StrictEncode, StrictDecode)]
#[strict_type(lib = bp::LIB_NAME_BP)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
pub struct RevealedSeal {
    /// Commitment to the specific seal close method [`CloseMethod`] which must
    /// be used to close this seal.
    pub method: CloseMethod,

    /// Txid of the seal definition.
    ///
    /// It may be missed in situations when ID of a transaction is not known,
    /// but the transaction still can be identified by some other means (for
    /// instance it is a transaction spending specific outpoint, like other
    /// seal definition).
    pub txid: Option<Txid>,

    /// Tx output number, which should be always known.
    pub vout: Vout,

    /// Blinding factor providing confidentiality of the seal definition.
    /// Prevents rainbow table bruteforce attack based on the existing
    /// blockchain txid set.
    pub blinding: u64,
}

impl TryFrom<&RevealedSeal> for Outpoint {
    type Error = WitnessVoutError;

    #[inline]
    fn try_from(reveal: &RevealedSeal) -> Result<Self, Self::Error> {
        reveal
            .txid
            .map(|txid| Outpoint::new(txid, reveal.vout))
            .ok_or(WitnessVoutError)
    }
}

impl TryFrom<RevealedSeal> for Outpoint {
    type Error = WitnessVoutError;

    #[inline]
    fn try_from(reveal: RevealedSeal) -> Result<Self, Self::Error> {
        Outpoint::try_from(&reveal)
    }
}

impl From<&Outpoint> for RevealedSeal {
    #[inline]
    fn from(outpoint: &Outpoint) -> Self {
        Self {
            method: CloseMethod::TapretFirst,
            blinding: thread_rng().next_u64(),
            txid: Some(outpoint.txid),
            vout: outpoint.vout,
        }
    }
}

impl From<Outpoint> for RevealedSeal {
    #[inline]
    fn from(outpoint: Outpoint) -> Self { RevealedSeal::from(&outpoint) }
}

impl From<&ExplicitSeal> for RevealedSeal {
    #[inline]
    fn from(seal: &ExplicitSeal) -> Self {
        Self {
            method: seal.method,
            blinding: thread_rng().next_u64(),
            txid: seal.txid,
            vout: seal.vout,
        }
    }
}

impl From<ExplicitSeal> for RevealedSeal {
    #[inline]
    fn from(seal: ExplicitSeal) -> Self { RevealedSeal::from(&seal) }
}

impl Conceal for RevealedSeal {
    type Concealed = ConcealedSeal;

    #[inline]
    fn conceal(&self) -> Self::Concealed { ConcealedSeal::commit(self) }
}

impl TxoSeal for RevealedSeal {
    #[inline]
    fn method(&self) -> CloseMethod { self.method }

    #[inline]
    fn txid(&self) -> Option<Txid> { self.txid }

    #[inline]
    fn vout(&self) -> Vout { self.vout }

    #[inline]
    fn outpoint(&self) -> Option<Outpoint> { self.try_into().ok() }

    #[inline]
    fn txid_or(&self, default_txid: Txid) -> Txid {
        self.txid.unwrap_or(default_txid)
    }

    #[inline]
    fn outpoint_or(&self, default_txid: Txid) -> Outpoint {
        Outpoint::new(self.txid.unwrap_or(default_txid), self.vout)
    }
}

impl RevealedSeal {
    /// Constructs seal for the provided outpoint and seal closing method. Uses
    /// `thread_rng` to initialize blinding factor.
    #[inline]
    pub fn new(method: CloseMethod, outpoint: Outpoint) -> RevealedSeal {
        Self {
            method,
            blinding: thread_rng().next_u64(),
            txid: Some(outpoint.txid),
            vout: outpoint.vout,
        }
    }

    /// Constructs seal using the provided random number generator for creating
    /// blinding factor value
    #[inline]
    pub fn with(
        method: CloseMethod,
        txid: Option<Txid>,
        vout: impl Into<Vout>,
        rng: &mut impl RngCore,
    ) -> RevealedSeal {
        RevealedSeal {
            method,
            txid,
            vout: vout.into(),
            blinding: rng.next_u64(),
        }
    }

    /// Converts revealed seal into concealed.
    #[inline]
    pub fn to_concealed_seal(&self) -> ConcealedSeal { self.conceal() }
}

/// Errors happening during parsing string representation of different forms of
/// single-use-seals
#[derive(Clone, PartialEq, Eq, Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum ParseError {
    /// single-use-seal must start with method name (e.g. 'tapret1st' etc)
    MethodRequired,

    /// full transaction id is required for the seal specification
    TxidRequired,

    /// blinding factor must be specified after `#`
    BlindingRequired,

    /// wrong seal close method id
    #[display(inner)]
    #[from]
    WrongMethod(MethodParseError),

    /// unable to parse blinding value; it must be a hexadecimal string
    /// starting with `0x`
    WrongBlinding,

    /// unable to parse transaction id value; it must be 64-character
    /// hexadecimal string
    WrongTxid,

    /// unable to parse transaction vout value; it must be a decimal unsigned
    /// integer
    WrongVout,

    /// wrong structure of seal string representation
    WrongStructure,

    /// blinding secret must be represented by a 64-bit hexadecimal value
    /// starting with `0x` and not with a decimal
    NonHexBlinding,

    /// wrong representation of the blinded TxOut seal – {0}
    #[from]
    Hex(hex::Error),
}

impl FromStr for RevealedSeal {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split(&[':', '#'][..]);
        match (
            split.next(),
            split.next(),
            split.next(),
            split.next(),
            split.next(),
        ) {
            (Some("~"), ..) | (Some(""), ..) => Err(ParseError::MethodRequired),
            (Some(_), Some(""), ..) => Err(ParseError::TxidRequired),
            (Some(_), Some(_), None, ..) if s.contains(':') => {
                Err(ParseError::BlindingRequired)
            }
            (Some(_), Some(_), Some(_), Some(blinding), None)
                if !blinding.starts_with("0x") =>
            {
                Err(ParseError::NonHexBlinding)
            }
            (Some(method), Some("~"), Some(vout), Some(blinding), None) => {
                Ok(RevealedSeal {
                    method: method.parse()?,
                    blinding: u64::from_str_radix(
                        blinding.trim_start_matches("0x"),
                        16,
                    )
                    .map_err(|_| ParseError::WrongBlinding)?,
                    txid: None,
                    vout: vout.parse().map_err(|_| ParseError::WrongVout)?,
                })
            }
            (Some(method), Some(txid), Some(vout), Some(blinding), None) => {
                Ok(RevealedSeal {
                    method: method.parse()?,
                    blinding: u64::from_str_radix(
                        blinding.trim_start_matches("0x"),
                        16,
                    )
                    .map_err(|_| ParseError::WrongBlinding)?,
                    txid: Some(
                        txid.parse().map_err(|_| ParseError::WrongTxid)?,
                    ),
                    vout: vout.parse().map_err(|_| ParseError::WrongVout)?,
                })
            }
            _ => Err(ParseError::WrongStructure),
        }
    }
}

impl Display for RevealedSeal {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}#{:#010x}",
            self.method,
            self.txid
                .as_ref()
                .map(Txid::to_string)
                .unwrap_or_else(|| s!("~")),
            self.vout,
            self.blinding
        )
    }
}

static MIDSTATE_CONCEALED_SEAL: [u8; 32] = [
    250, 13, 163, 5, 178, 220, 248, 173, 139, 222, 67, 198, 134, 127, 63, 153,
    147, 236, 172, 33, 17, 167, 176, 30, 70, 99, 185, 129, 217, 110, 183, 27,
];

/// Blind version of transaction outpoint-based single-use-seal
#[derive(Wrapper, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, From)]
#[wrapper(Index, RangeOps, BorrowSlice, Hex)]
#[derive(StrictType, StrictDumb, StrictEncode, StrictDecode)]
#[strict_type(lib = bp::LIB_NAME_BP)]
pub struct ConcealedSeal(
    #[from]
    #[from([u8; 32])]
    Bytes32,
);

impl FromStr for ConcealedSeal {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ConcealedSeal::from_hex(s)?)
    }
}

impl From<Outpoint> for ConcealedSeal {
    #[inline]
    fn from(outpoint: Outpoint) -> Self {
        RevealedSeal::from(outpoint).conceal()
    }
}

impl CommitVerify<RevealedSeal, Lnpbp12> for ConcealedSeal {
    fn commit(reveal: &RevealedSeal) -> Self {
        let mut engine = Sha256::from_tag(MIDSTATE_CONCEALED_SEAL);
        // TODO: Use tag
        engine.input(&[reveal.method as u8]);
        engine.input(&reveal.txid.unwrap_or_else(|| Txid::from([0u8; 32]))[..]);
        engine.input(&reveal.vout.into_u32().to_le_bytes()[..]);
        engine.input(&reveal.blinding.to_le_bytes()[..]);
        ConcealedSeal::from_inner(engine.finish().into())
    }
}

#[cfg(test)]
mod test {
    use amplify::Wrapper;

    use super::*;

    #[test]
    fn outpoint_hash_is_sha256d() {
        let reveal = RevealedSeal {
            method: CloseMethod::TapretFirst,
            blinding: 54683213134637,
            txid: Some(Txid::from_hex("646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839").unwrap()),
            vout: Vout::from(2),
        };
        let outpoint_hash = reveal.to_concealed_seal();
        let mut engine = Sha256::from_tag(MIDSTATE_CONCEALED_SEAL);
        engine.input(&[reveal.method as u8]);
        engine.input(&reveal.txid.unwrap()[..]);
        engine.input(&reveal.vout.into_u32().to_le_bytes()[..]);
        engine.input(&reveal.blinding.to_le_bytes()[..]);
        assert_eq!(outpoint_hash.as_inner().as_slice(), &engine.finish())
    }

    /* TODO: replace with Baid58 test
    #[test]
    fn outpoint_hash_bech32() {
        let outpoint_hash = RevealedSeal {
            method: CloseMethod::TapretFirst,
            blinding: 54683213134637,
            txid: Some(Txid::from_hex("646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839").unwrap()),
            vout: Vout::from(2),
        }.to_concealed_seal();

        let bech32 =
            "txob1a9peq6yx9x6ajt584qp5ge4jk9v7tmtgs3x2gntk2nf425cvpdgszt65je";
        assert_eq!(bech32, outpoint_hash.to_string());
        assert_eq!(outpoint_hash.to_string(), outpoint_hash.to_bech32_string());
        let reconstructed = ConcealedSeal::from_str(bech32).unwrap();
        assert_eq!(reconstructed, outpoint_hash);
    }
     */

    #[test]
    fn outpoint_reveal_str() {
        let mut outpoint_reveal = RevealedSeal {
            method: CloseMethod::TapretFirst,
            blinding: 54683213134637,
            txid: Some(Txid::from_hex("646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839").unwrap()),
            vout: Vout::from(21),
        };

        let s = outpoint_reveal.to_string();
        assert_eq!(
            &s,
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:\
             21#0x31bbed7e7b2d"
        );
        // round-trip
        assert_eq!(RevealedSeal::from_str(&s).unwrap(), outpoint_reveal);

        outpoint_reveal.txid = None;
        let s = outpoint_reveal.to_string();
        assert_eq!(&s, "tapret1st:~:21#0x31bbed7e7b2d");
        // round-trip
        assert_eq!(RevealedSeal::from_str(&s).unwrap(), outpoint_reveal);

        // wrong method
        assert_eq!(RevealedSeal::from_str(
            "tapret:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:0x765#0x78ca95"
        ), Err(ParseError::WrongMethod(MethodParseError(s!("tapret")))));

        // wrong vout value
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:0x765#0x78ca95"
        ), Err(ParseError::WrongVout));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:i9#0x78ca95"
        ), Err(ParseError::WrongVout));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:-5#0x78ca95"
        ), Err(ParseError::WrongVout));

        // wrong blinding secret value
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:5#0x78cs"
        ), Err(ParseError::WrongBlinding));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:5#78ca95"
        ), Err(ParseError::NonHexBlinding));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:5#857"
        ), Err(ParseError::NonHexBlinding));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:5#-5"
        ), Err(ParseError::NonHexBlinding));

        // wrong txid value
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d607719dfd820551fb773e4dc8c4ed67965a8d1fae839:5#0x78ca69"
        ), Err(ParseError::WrongTxid));
        assert_eq!(
            RevealedSeal::from_str("tapret1st:rvgbdg:5#0x78ca69"),
            Err(ParseError::WrongTxid)
        );
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:10@646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:5#0x78ca69"
        ), Err(ParseError::WrongTxid));

        // wrong structure
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:1"
        ), Err(ParseError::WrongStructure));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839#0x78ca"
        ), Err(ParseError::WrongStructure));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839"
        ), Err(ParseError::BlindingRequired));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839##0x78ca"
        ), Err(ParseError::WrongVout));
        assert_eq!(RevealedSeal::from_str(
            "tapret1st:646ca5c1062619e2a2d60771c9dfd820551fb773e4dc8c4ed67965a8d1fae839:#0x78ca95"
        ), Err(ParseError::WrongVout));
        assert_eq!(
            RevealedSeal::from_str("tapret1st:_:5#0x78ca"),
            Err(ParseError::WrongTxid)
        );
        assert_eq!(
            RevealedSeal::from_str(":5#0x78ca"),
            Err(ParseError::MethodRequired)
        );
        assert_eq!(
            RevealedSeal::from_str("~:5#0x78ca"),
            Err(ParseError::MethodRequired)
        );
    }
}
