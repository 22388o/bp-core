// Deterministic bitcoin commitments library.
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2019-2024 by
//     Dr Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// Copyright (C) 2019-2024 LNP/BP Standards Association. All rights reserved.
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

//! Signature tweaking-based deterministic commitment scheme.
//!
//! **Sign-commit:**
//! a) `PrivateKey, Msg -> ecdsa::Signature`;
//! b) `KeyPair, Msg -> bip340::Signature`;
//! **Convolve-commit:**
//! c) `psbt::Input, PrivateKey, Msg -> psbt::Input'`;
//! d) `psbt::Input, KeyPair, Msg -> psbt::Input'`;
