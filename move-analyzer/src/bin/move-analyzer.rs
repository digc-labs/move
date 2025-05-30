// Copyright (c) Mysten Labs, Inc.
// Modifications Copyright (c) 2024 Digc Labs
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use move_analyzer::analyzer;
use std::collections::BTreeMap;

#[derive(Parser)]
#[clap(author, version, about)]
struct Options {}

#[allow(deprecated)]
fn main() {
    // For now, move-analyzer only responds to options built-in to clap,
    // such as `--help` or `--version`.
    Options::parse();
    analyzer::run(BTreeMap::new());
}
