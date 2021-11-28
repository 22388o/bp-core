// BP Core Library implementing LNP/BP specifications & standards related to
// bitcoin protocol
//
// Written in 2020-2021 by
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

//! # LNPBP-2
//!
//! Module implementing LNPBP-2 standard:
//! Deterministic embedding of LNPBP1-type commitments into `scriptPubkey` of a
//! transaction output
//! [LNPBP-2](https://github.com/LNP-BP/lnpbps/blob/master/lnpbp-0002.md)
//!
//! The standard defines an algorithm for deterministic embedding and
//! verification of cryptographic commitments based on elliptic-curve public and
//! private key modifications (tweaks) inside all the existing types of Bitcoin
//! transaction output and arbitrary complex Bitcoin scripts.

use core::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

use bitcoin::hashes::{hash160, sha256, Hash, Hmac};
use bitcoin::secp256k1;
use bitcoin_scripts::LockScript;
use commit_verify::EmbedCommitVerify;
use miniscript::Segwitv0;

use super::{Container, Error, KeysetCommitment, Proof, ScriptEncodeData};
use crate::KeysetContainer;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct LockscriptContainer {
    pub script: LockScript,
    pub pubkey: secp256k1::PublicKey,
    /// Single SHA256 hash of the protocol-specific tag
    pub tag: sha256::Hash,
    /// Tweaking factor stored after [`LockscriptCommitment::embed_commit`]
    /// procedure
    pub tweaking_factor: Option<Hmac<sha256::Hash>>,
}

impl Container for LockscriptContainer {
    /// Out supplement is a protocol-specific tag in its hashed form
    type Supplement = sha256::Hash;

    type Host = Option<()>;

    fn reconstruct(
        proof: &Proof,
        supplement: &Self::Supplement,
        _: &Self::Host,
    ) -> Result<Self, Error> {
        if let ScriptEncodeData::LockScript(ref script) = proof.source {
            Ok(Self {
                pubkey: proof.pubkey,
                script: script.clone(),
                tag: *supplement,
                tweaking_factor: None,
            })
        } else {
            Err(Error::InvalidProofStructure)
        }
    }

    #[inline]
    fn deconstruct(self) -> (Proof, Self::Supplement) {
        (
            Proof {
                source: ScriptEncodeData::LockScript(self.script),
                pubkey: self.pubkey,
            },
            self.tag,
        )
    }

    #[inline]
    fn to_proof(&self) -> Proof {
        Proof {
            source: ScriptEncodeData::LockScript(self.script.clone()),
            pubkey: self.pubkey,
        }
    }

    #[inline]
    fn into_proof(self) -> Proof {
        Proof {
            source: ScriptEncodeData::LockScript(self.script),
            pubkey: self.pubkey,
        }
    }
}

/// [`LockScript`] containing public keys which sum is commit to some message
/// according to LNPBP-2
#[derive(
    Wrapper, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug,
    Display, From
)]
#[display(inner)]
#[wrapper(LowerHex, UpperHex)]
pub struct LockscriptCommitment(LockScript);

impl<MSG> EmbedCommitVerify<MSG> for LockscriptCommitment
where
    MSG: AsRef<[u8]>,
{
    type Container = LockscriptContainer;
    type Error = Error;

    /// Function implements commitment procedure according to LNPBP-2.
    ///
    /// ## LNPBP-2 Specification extract:
    ///
    /// 1. The provided script MUST be parsed with Miniscript parser; if the
    ///    parser fails the procedure MUST fail.
    /// 2. Iterate over all branches of the abstract syntax tree generated by
    ///    the Miniscript parser, running the following algorithm for each node:
    ///    - if a public key hash is met (`pk_h` Miniscript command) and it
    ///      can't be resolved against known public keys or other public keys
    ///      extracted from the script, fail the procedure;
    ///    - if a public key is found (`pk`) add it to the list of the collected
    ///      public keys;
    ///    - for all other types of Miniscript commands iterate over their
    ///      branches.
    /// 3. Select unique public keys (i.e. if some public key is repeated in
    ///    different parts of the script/in different script branches, pick a
    ///    single instance of it). Compressed and uncompressed versions of the
    ///    same public key must be treaded as the same public key under this
    ///    procedure.
    /// 4. If no public keys were found fail the procedure; return the collected
    ///    keys otherwise.
    ///
    /// **NB: SUBJECT TO CHANGE UPON RELEASE**
    /// By "miniscript" we mean usage of `rust-miniscript` library at commit
    /// `a5ba1219feb8b5a289c8f12176d632635eb8a959`
    /// which may be found on
    /// <https://github.com/LNP-BP/rust-miniscript/commit/a5ba1219feb8b5a289c8f12176d632635eb8a959>
    // #[consensus_critical]
    // #[standard_critical("LNPBP-1")]
    fn embed_commit(
        container: &mut Self::Container,
        msg: &MSG,
    ) -> Result<Self, Self::Error> {
        let original_hash = hash160::Hash::hash(
            &bitcoin::PublicKey {
                compressed: true,
                key: container.pubkey,
            }
            .to_bytes(),
        );

        let (keys, hashes) =
            container.script.extract_pubkey_hash_set::<Segwitv0>()?;
        if keys.is_empty() && hashes.is_empty() {
            return Err(Error::LockscriptContainsNoKeys);
        }

        let mut key_hashes: HashSet<hash160::Hash> = keys
            .iter()
            .map(|key| hash160::Hash::hash(&key.to_bytes()))
            .collect();
        key_hashes.insert(original_hash);
        let keys: BTreeSet<_> = keys.into_iter().map(|pk| pk.key).collect();

        if hashes.is_empty() {
            keys.get(&container.pubkey)
                .ok_or(Error::LockscriptKeyNotFound)?;
        } else if hashes.into_iter().any(|hash| !key_hashes.contains(&hash)) {
            return Err(Error::LockscriptContainsUnknownHashes);
        }

        let mut keyset_container = KeysetContainer {
            pubkey: container.pubkey,
            keyset: keys,
            tag: container.tag,
            tweaking_factor: None,
        };

        let tweaked_pubkey =
            KeysetCommitment::embed_commit(&mut keyset_container, msg)?;

        container.tweaking_factor = keyset_container.tweaking_factor;

        let tweaked_hash = bitcoin::PublicKey {
            key: *tweaked_pubkey,
            compressed: true,
        }
        .pubkey_hash();

        let found = RefCell::new(0);

        // ! [CONSENSUS-CRITICAL]:
        // ! [STANDARD-CRITICAL]: Iterate over all branches of the abstract
        //                        syntax tree generated by the Miniscript
        // parser,                        running the following
        // algorithm for each node:
        let lockscript = container
            .script
            .replace_pubkeys_and_hashes::<Segwitv0, _, _>(
                |pubkey: &bitcoin::PublicKey| match pubkey.key
                    == container.pubkey
                {
                    true => {
                        *found.borrow_mut() += 1;
                        bitcoin::PublicKey {
                            compressed: true,
                            key: *tweaked_pubkey,
                        }
                    }
                    false => *pubkey,
                },
                |hash: &hash160::Hash| match *hash == original_hash {
                    true => {
                        *found.borrow_mut() += 1;
                        tweaked_hash.as_hash()
                    }
                    false => *hash,
                },
            )?;

        Ok(lockscript.into())
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use bitcoin::hashes::{hash160, sha256, Hash};
    use bitcoin::PubkeyHash;
    use miniscript::{Miniscript, Segwitv0};

    use super::*;
    use crate::Error;

    macro_rules! ms_str {
        ($($arg:tt)*) => (Miniscript::<bitcoin::PublicKey, Segwitv0>::from_str_insane(&format!($($arg)*)).unwrap())
    }

    macro_rules! policy_str {
        ($($arg:tt)*) => (miniscript::policy::Concrete::<bitcoin::PublicKey>::from_str(&format!($($arg)*)).unwrap())
    }

    fn pubkeys(n: usize) -> Vec<bitcoin::PublicKey> {
        let mut ret = Vec::with_capacity(n);
        let mut sk = [0; 32];
        for i in 1..n + 1 {
            sk[0] = i as u8;
            sk[1] = (i >> 8) as u8;
            sk[2] = (i >> 16) as u8;

            let pk = bitcoin::PublicKey {
                key: secp256k1::PublicKey::from_secret_key(
                    &secp256k1::SECP256K1,
                    &secp256k1::SecretKey::from_slice(&sk[..])
                        .expect("secret key"),
                ),
                compressed: true,
            };
            ret.push(pk);
        }
        ret
    }

    fn gen_test_data(
    ) -> (Vec<bitcoin::PublicKey>, Vec<PubkeyHash>, Vec<hash160::Hash>) {
        let keys = pubkeys(13);
        let key_hashes =
            keys.iter().map(bitcoin::PublicKey::pubkey_hash).collect();
        let dummy_hashes = (1..13)
            .map(|i| hash160::Hash::from_inner([i; 20]))
            .collect();
        (keys, key_hashes, dummy_hashes)
    }

    #[test]
    fn test_no_keys_and_hashes() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, dummy_hashes) = gen_test_data();
        let sha_hash = sha256::Hash::hash(&"(nearly)random string".as_bytes());

        let ms = vec![
            ms_str!("older(921)"),
            ms_str!("sha256({})", sha_hash),
            ms_str!("hash256({})", sha_hash),
            ms_str!("hash160({})", dummy_hashes[0]),
            ms_str!("ripemd160({})", dummy_hashes[1]),
            ms_str!("hash160({})", dummy_hashes[2]),
        ];

        ms.into_iter()
            .map(|ms: Miniscript<_, _>| LockScript::from(ms.encode()))
            .for_each(|ls| {
                assert_eq!(
                    LockscriptCommitment::embed_commit(
                        &mut LockscriptContainer {
                            script: ls,
                            pubkey: keys[0].key,
                            tag,
                            tweaking_factor: None
                        },
                        &"Test message"
                    )
                    .err(),
                    Some(Error::LockscriptContainsNoKeys)
                );
            });
    }

    #[test]
    fn test_unknown_key() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let mut uncompressed = keys[5];
        uncompressed.compressed = false;
        let ms = vec![
            ms_str!("c:pk_k({})", keys[1]),
            ms_str!("c:pk_k({})", keys[2]),
            ms_str!("c:pk_k({})", keys[3]),
            ms_str!("c:pk_k({})", keys[4]),
            //ms_str!("c:pk({})", uncompressed),
        ];

        ms.into_iter()
            .map(|ms| LockScript::from(ms.encode()))
            .for_each(|ls| {
                assert_eq!(
                    LockscriptCommitment::embed_commit(
                        &mut LockscriptContainer {
                            script: ls,
                            pubkey: keys[0].key,
                            tag,
                            tweaking_factor: None
                        },
                        &"Test message"
                    )
                    .err(),
                    Some(Error::LockscriptKeyNotFound)
                );
            });
    }

    #[test]
    fn test_unknown_hash() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let ms = vec![
            ms_str!("c:pk_h({})", keys[1].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[2].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[3].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[4].pubkey_hash()),
        ];

        ms.into_iter()
            .map(|ms| LockScript::from(ms.encode()))
            .for_each(|ls| {
                assert_eq!(
                    LockscriptCommitment::embed_commit(
                        &mut LockscriptContainer {
                            script: ls,
                            pubkey: keys[0].key,
                            tag,
                            tweaking_factor: None
                        },
                        &"Test message"
                    )
                    .err(),
                    Some(Error::LockscriptContainsUnknownHashes)
                );
            });
    }

    #[test]
    fn test_known_key() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let mut uncompressed = keys[5];
        uncompressed.compressed = false;
        let ms = vec![
            ms_str!("c:pk_k({})", keys[0]),
            ms_str!("c:pk_k({})", keys[1]),
            ms_str!("c:pk_k({})", keys[2]),
            ms_str!("c:pk_k({})", keys[3]),
            //ms_str!("c:pk_k({})", uncompressed),
        ];

        ms.into_iter()
            .map(|ms| LockScript::from(ms.encode()))
            .enumerate()
            .for_each(|(idx, ls)| {
                let container = LockscriptContainer {
                    script: ls,
                    pubkey: keys[idx].key,
                    tag,
                    tweaking_factor: None,
                };
                let msg = "Test message";
                let commitment = LockscriptCommitment::embed_commit(
                    &mut container.clone(),
                    &msg,
                )
                .unwrap();
                assert!(commitment.verify(&container, &msg).unwrap());
            });
    }

    #[test]
    fn test_known_hash() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let ms = vec![
            ms_str!("c:pk_h({})", keys[0].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[1].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[2].pubkey_hash()),
            ms_str!("c:pk_h({})", keys[3].pubkey_hash()),
        ];

        ms.into_iter()
            .map(|ms| LockScript::from(ms.encode()))
            .enumerate()
            .for_each(|(idx, ls)| {
                let container = LockscriptContainer {
                    script: ls,
                    pubkey: keys[idx].key,
                    tag,
                    tweaking_factor: None,
                };
                let msg = "Test message";
                let commitment = LockscriptCommitment::embed_commit(
                    &mut container.clone(),
                    &msg,
                )
                .unwrap();
                assert!(commitment.verify(&container, &msg).unwrap())
            });
    }

    #[test]
    fn test_multisig() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let ms: Vec<Miniscript<_, Segwitv0>> = vec![
            policy_str!("thresh(2,pk({}),pk({}))", keys[0], keys[1],),
            policy_str!(
                "thresh(3,pk({}),pk({}),pk({}),pk({}),pk({}))",
                keys[0],
                keys[1],
                keys[2],
                keys[3],
                keys[4]
            ),
        ]
        .into_iter()
        .map(|p| p.compile().unwrap())
        .collect();

        ms.into_iter()
            .map(|ms| LockScript::from(ms.encode()))
            .for_each(|ls| {
                let container = LockscriptContainer {
                    script: ls,
                    pubkey: keys[1].key,
                    tag,
                    tweaking_factor: None,
                };
                let msg = "Test message";
                let commitment = LockscriptCommitment::embed_commit(
                    &mut container.clone(),
                    &msg,
                )
                .unwrap();
                assert!(commitment.verify(&container, &msg).unwrap())
            });
    }

    #[test]
    fn test_complex_scripts_unique_key() {
        let tag = sha256::Hash::hash(b"TEST_TAG");
        let (keys, _, _) = gen_test_data();

        let ms = policy_str!(
            "or(thresh(3,pk({}),pk({}),pk({})),and(thresh(2,pk({}),pk({})),\
             older(10000)))",
            keys[0],
            keys[1],
            keys[2],
            keys[3],
            keys[4],
        )
        .compile::<Segwitv0>()
        .unwrap();

        let container = LockscriptContainer {
            script: LockScript::from(ms.encode()),
            pubkey: keys[1].key,
            tag,
            tweaking_factor: None,
        };
        let msg = "Test message";
        let commitment =
            LockscriptCommitment::embed_commit(&mut container.clone(), &msg)
                .unwrap();
        assert!(commitment.verify(&container, &msg).unwrap())
    }
}