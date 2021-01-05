// LNP/BP Core Library implementing LNPBP specifications & standards
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use bitcoin::blockdata::{opcodes::all::*, script};
use bitcoin::secp256k1::PublicKey;
use bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
use bitcoin::{OutPoint, Transaction, TxIn, TxOut};

use crate::bp::{
    chain::AssetId, HashLock, HashPreimage, IntoPk, LockScript, PubkeyScript,
    WitnessScript,
};
use crate::lnp::application::payment::{ExtensionId, TxType};
use crate::lnp::application::{channel, ChannelExtension, Extension, Messages};

use crate::SECP256K1_PUBKEY_DUMB;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HtlcKnown {
    pub preimage: HashPreimage,
    pub id: u64,
    pub cltv_expiry: u32,
    pub asset_id: Option<AssetId>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HtlcSecret {
    pub hashlock: HashLock,
    pub id: u64,
    pub cltv_expiry: u32,
    pub asset_id: Option<AssetId>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Htlc {
    offered_htlc: Vec<HtlcKnown>,
    received_htlc: Vec<HtlcSecret>,
    resolved_htlc: Vec<HtlcKnown>,
}

impl channel::State for Htlc {}

impl Extension for Htlc {
    type Identity = ExtensionId;

    fn identity(&self) -> Self::Identity {
        ExtensionId::Htlc
    }

    fn update_from_peer(
        &mut self,
        message: &Messages,
    ) -> Result<(), channel::Error> {
        match message {
            Messages::UpdateAddHtlc(_) => {}
            Messages::UpdateFulfillHtlc(_) => {}
            Messages::UpdateFailHtlc(_) => {}
            Messages::UpdateFailMalformedHtlc(_) => {}
            Messages::CommitmentSigned(_) => {}
            Messages::RevokeAndAck(_) => {}
            Messages::ChannelReestablish(_) => {}
            _ => {}
        }
        Ok(())
    }

    fn extension_state(&self) -> Box<dyn channel::State> {
        Box::new(self.clone())
    }
}

impl ChannelExtension for Htlc {
    fn channel_state(&self) -> Box<dyn channel::State> {
        Box::new(self.clone())
    }

    fn apply(
        &mut self,
        tx_graph: &mut channel::TxGraph,
    ) -> Result<(), channel::Error> {
        for (index, offered) in self.offered_htlc.iter().enumerate() {
            // TODO, Dummy structures used here
            // figure out how they should be used from HTLC data or global
            // channel state data

            let amount = 10u64;
            let revocationpubkey = *SECP256K1_PUBKEY_DUMB;
            let local_htlcpubkey = *SECP256K1_PUBKEY_DUMB;
            let remote_htlcpubkey = *SECP256K1_PUBKEY_DUMB;
            let payment_hash = HashLock::from(offered.preimage.clone()); // Calculation of payment hash not seems to be as per BOLT3

            let htlc_output = TxOut::ln_offered_htlc(
                amount,
                revocationpubkey,
                local_htlcpubkey,
                remote_htlcpubkey,
                payment_hash,
            );
            tx_graph.cmt_outs.push(htlc_output);

            //Dummy variables, Figure out how to get them for a HTLC
            // This will be an HTLC timeout transaction
            let outpoint = OutPoint::default();
            let cltv_expiry = offered.cltv_expiry;
            let revocationpubkey = *SECP256K1_PUBKEY_DUMB;
            let local_delayedpubkey = *SECP256K1_PUBKEY_DUMB;
            let to_self_delay = 0u16;

            let htlc_tx = Psbt::ln_htlc(
                amount,
                outpoint,
                cltv_expiry,
                revocationpubkey,
                local_delayedpubkey,
                to_self_delay,
            );
            tx_graph.insert_tx(TxType::HtlcTimeout, index as u64, htlc_tx);
        }

        for (index, recieved) in self.received_htlc.iter().enumerate() {
            // TODO, Dummy structures used here
            // figure out how they should be used from HTLC structure
            // Calculation of payment hash not seems to be as per BOLT3
            let amount = 10u64;
            let revocationpubkey = *SECP256K1_PUBKEY_DUMB;
            let local_htlcpubkey = *SECP256K1_PUBKEY_DUMB;
            let remote_htlcpubkey = *SECP256K1_PUBKEY_DUMB;
            let payment_hash = recieved.hashlock.clone();
            let cltv_expiry = 0;

            let htlc_output = TxOut::ln_received_htlc(
                amount,
                revocationpubkey,
                local_htlcpubkey,
                remote_htlcpubkey,
                cltv_expiry,
                payment_hash,
            );
            tx_graph.cmt_outs.push(htlc_output);

            //Dummy variables, Figure out how to get them for a HTLC
            // This will be an HTLC success transaction, so cltv_expiry is 0
            let outpoint = OutPoint::default();
            let cltv_expiry = 0;
            let revocationpubkey = *SECP256K1_PUBKEY_DUMB;
            let local_delayedpubkey = *SECP256K1_PUBKEY_DUMB;
            let to_self_delay = 0u16;

            let htlc_tx = Psbt::ln_htlc(
                amount,
                outpoint,
                cltv_expiry,
                revocationpubkey,
                local_delayedpubkey,
                to_self_delay,
            );
            tx_graph.insert_tx(TxType::HtlcSuccess, index as u64, htlc_tx);
        }

        Ok(())
    }
}

pub trait ScriptGenerators {
    fn ln_offered_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        payment_hash: HashLock,
    ) -> Self;

    fn ln_received_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        cltv_expiry: u32,
        payment_hash: HashLock,
    ) -> Self;

    fn ln_htlc_output(
        amount: u64,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self;
}

impl ScriptGenerators for LockScript {
    fn ln_offered_htlc(
        _: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        payment_hash: HashLock,
    ) -> Self {
        script::Builder::new()
            .push_opcode(OP_DUP)
            .push_opcode(OP_HASH160)
            .push_slice(&revocationpubkey.into_pk().pubkey_hash())
            .push_opcode(OP_EQUAL)
            .push_opcode(OP_IF)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ELSE)
            .push_key(&remote_htlcpubkey.into_pk())
            .push_opcode(OP_SWAP)
            .push_opcode(OP_SIZE)
            .push_int(32)
            .push_opcode(OP_EQUAL)
            .push_opcode(OP_NOTIF)
            .push_opcode(OP_DROP)
            .push_int(2)
            .push_opcode(OP_SWAP)
            .push_key(&local_htlcpubkey.into_pk())
            .push_int(2)
            .push_opcode(OP_CHECKMULTISIG)
            .push_opcode(OP_ELSE)
            .push_opcode(OP_HASH160)
            .push_slice(payment_hash.as_ref())
            .push_opcode(OP_EQUALVERIFY)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ENDIF)
            .push_opcode(OP_ENDIF)
            .into_script()
            .into()
    }

    fn ln_received_htlc(
        _: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        cltv_expiry: u32,
        payment_hash: HashLock,
    ) -> Self {
        script::Builder::new()
            .push_opcode(OP_DUP)
            .push_opcode(OP_HASH160)
            .push_slice(&revocationpubkey.into_pk().pubkey_hash())
            .push_opcode(OP_EQUAL)
            .push_opcode(OP_IF)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ELSE)
            .push_key(&remote_htlcpubkey.into_pk())
            .push_opcode(OP_SWAP)
            .push_opcode(OP_SIZE)
            .push_int(32)
            .push_opcode(OP_EQUAL)
            .push_opcode(OP_IF)
            .push_opcode(OP_HASH160)
            .push_slice(payment_hash.as_ref())
            .push_opcode(OP_EQUALVERIFY)
            .push_int(2)
            .push_opcode(OP_SWAP)
            .push_key(&local_htlcpubkey.into_pk())
            .push_int(2)
            .push_opcode(OP_CHECKMULTISIG)
            .push_opcode(OP_ELSE)
            .push_opcode(OP_DROP)
            .push_int(cltv_expiry as i64)
            .push_opcode(OP_CLTV)
            .push_opcode(OP_DROP)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ENDIF)
            .push_opcode(OP_ENDIF)
            .into_script()
            .into()
    }

    fn ln_htlc_output(
        _: u64,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        script::Builder::new()
            .push_opcode(OP_IF)
            .push_key(&revocationpubkey.into_pk())
            .push_opcode(OP_ELSE)
            .push_int(to_self_delay as i64)
            .push_opcode(OP_CSV)
            .push_opcode(OP_DROP)
            .push_key(&local_delayedpubkey.into_pk())
            .push_opcode(OP_ENDIF)
            .push_opcode(OP_CHECKSIG)
            .into_script()
            .into()
    }
}

impl ScriptGenerators for WitnessScript {
    #[inline]
    fn ln_offered_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        payment_hash: HashLock,
    ) -> Self {
        LockScript::ln_offered_htlc(
            amount,
            revocationpubkey,
            local_htlcpubkey,
            remote_htlcpubkey,
            payment_hash,
        )
        .into()
    }

    #[inline]
    fn ln_received_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        cltv_expiry: u32,
        payment_hash: HashLock,
    ) -> Self {
        LockScript::ln_received_htlc(
            amount,
            revocationpubkey,
            local_htlcpubkey,
            remote_htlcpubkey,
            cltv_expiry,
            payment_hash,
        )
        .into()
    }

    #[inline]
    fn ln_htlc_output(
        amount: u64,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        LockScript::ln_htlc_output(
            amount,
            revocationpubkey,
            local_delayedpubkey,
            to_self_delay,
        )
        .into()
    }
}

impl ScriptGenerators for PubkeyScript {
    #[inline]
    fn ln_offered_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        payment_hash: HashLock,
    ) -> Self {
        WitnessScript::ln_offered_htlc(
            amount,
            revocationpubkey,
            local_htlcpubkey,
            remote_htlcpubkey,
            payment_hash,
        )
        .to_p2wsh()
    }

    #[inline]
    fn ln_received_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        cltv_expiry: u32,
        payment_hash: HashLock,
    ) -> Self {
        WitnessScript::ln_received_htlc(
            amount,
            revocationpubkey,
            local_htlcpubkey,
            remote_htlcpubkey,
            cltv_expiry,
            payment_hash,
        )
        .to_p2wsh()
    }

    #[inline]
    fn ln_htlc_output(
        amount: u64,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        WitnessScript::ln_htlc_output(
            amount,
            revocationpubkey,
            local_delayedpubkey,
            to_self_delay,
        )
        .to_p2wsh()
    }
}

impl ScriptGenerators for TxOut {
    #[inline]
    fn ln_offered_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        payment_hash: HashLock,
    ) -> Self {
        TxOut {
            value: amount,
            script_pubkey: PubkeyScript::ln_offered_htlc(
                amount,
                revocationpubkey,
                local_htlcpubkey,
                remote_htlcpubkey,
                payment_hash,
            )
            .into(),
        }
    }

    #[inline]
    fn ln_received_htlc(
        amount: u64,
        revocationpubkey: PublicKey,
        local_htlcpubkey: PublicKey,
        remote_htlcpubkey: PublicKey,
        cltv_expiry: u32,
        payment_hash: HashLock,
    ) -> Self {
        TxOut {
            value: amount,
            script_pubkey: PubkeyScript::ln_received_htlc(
                amount,
                revocationpubkey,
                local_htlcpubkey,
                remote_htlcpubkey,
                cltv_expiry,
                payment_hash,
            )
            .into(),
        }
    }

    #[inline]
    fn ln_htlc_output(
        amount: u64,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        TxOut {
            value: amount,
            script_pubkey: PubkeyScript::ln_htlc_output(
                amount,
                revocationpubkey,
                local_delayedpubkey,
                to_self_delay,
            )
            .into(),
        }
    }
}

pub trait TxGenerators {
    fn ln_htlc(
        amount: u64,
        outpoint: OutPoint,
        cltv_expiry: u32,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self;
}

impl TxGenerators for Transaction {
    /// NB: For HTLC Success transaction always set `cltv_expiry` parameter
    ///     to zero!
    fn ln_htlc(
        amount: u64,
        outpoint: OutPoint,
        cltv_expiry: u32,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        Transaction {
            version: 2,
            lock_time: cltv_expiry,
            input: vec![TxIn {
                previous_output: outpoint,
                script_sig: none!(),
                sequence: 0,
                witness: empty!(),
            }],
            output: vec![TxOut::ln_htlc_output(
                amount,
                revocationpubkey,
                local_delayedpubkey,
                to_self_delay,
            )],
        }
    }
}

impl TxGenerators for Psbt {
    fn ln_htlc(
        amount: u64,
        outpoint: OutPoint,
        cltv_expiry: u32,
        revocationpubkey: PublicKey,
        local_delayedpubkey: PublicKey,
        to_self_delay: u16,
    ) -> Self {
        Psbt::from_unsigned_tx(Transaction::ln_htlc(
            amount,
            outpoint,
            cltv_expiry,
            revocationpubkey,
            local_delayedpubkey,
            to_self_delay,
        ))
        .expect("Tx has empty sigs so PSBT creation does not faile")
    }
}
