// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// Modifications Copyright (c) 2024 Digc Labs
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use anyhow::Context;
use clap::Parser;
use move_binary_format::{errors::VMError, file_format::CompiledModule};
use move_bytecode_verifier::{dependencies, verify_module_unmetered};
use move_command_line_common::files::{
    DEBUG_INFO_EXTENSION, MOVE_COMPILED_EXTENSION, MOVE_IR_EXTENSION,
};
use move_ir_compiler::util;
use move_ir_to_bytecode::parser::parse_module;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
    /// Do not automatically run the bytecode verifier
    #[clap(long = "no-verify")]
    pub no_verify: bool,
    /// Path to the Move IR source to compile
    pub source_path: PathBuf,
    /// Instead of compiling the source, emit a dependency list of the compiled source
    #[clap(short = 'l', long = "list-dependencies")]
    pub list_dependencies: bool,
    /// Path to the list of modules that we want to link with
    #[clap(short = 'd', long = "deps")]
    pub deps_path: Option<String>,

    #[clap(long = "src-map")]
    pub output_source_maps: bool,
}

fn print_error_and_exit(verification_error: &VMError) -> ! {
    println!("Verification failed:");
    println!("{:?}", verification_error);
    std::process::exit(1);
}

fn do_verify_module(module: &CompiledModule, dependencies: &[CompiledModule]) {
    verify_module_unmetered(module).unwrap_or_else(|err| print_error_and_exit(&err));
    if let Err(err) = dependencies::verify_module(module, dependencies) {
        print_error_and_exit(&err);
    }
}

fn write_output(path: &Path, buf: &[u8]) {
    let mut f = fs::File::create(path)
        .with_context(|| format!("Unable to open output file {:?}", path))
        .unwrap();
    f.write_all(buf)
        .with_context(|| format!("Unable to write to output file {:?}", path))
        .unwrap();
}

fn main() {
    let args = Args::parse();

    let source_path = Path::new(&args.source_path);
    let mvir_extension = MOVE_IR_EXTENSION;
    let mv_extension = MOVE_COMPILED_EXTENSION;
    let debug_info_extension = DEBUG_INFO_EXTENSION;
    let extension = source_path
        .extension()
        .expect("Missing file extension for input source file");
    if extension != mvir_extension {
        println!(
            "Bad source file extension {:?}; expected {}",
            extension, mvir_extension
        );
        std::process::exit(1);
    }

    if args.list_dependencies {
        let source = fs::read_to_string(args.source_path.clone()).expect("Unable to read file");
        let dependency_list = {
            let module = parse_module(&source).expect("Unable to parse module");
            module.get_external_deps()
        };
        println!(
            "{}",
            serde_json::to_string(&dependency_list).expect("Unable to serialize dependencies")
        );
        return;
    }

    let deps_owned = {
        if let Some(path) = args.deps_path {
            let deps = fs::read_to_string(path).expect("Unable to read dependency file");
            let deps_list: Vec<Vec<u8>> =
                serde_json::from_str(deps.as_str()).expect("Unable to parse dependency file");
            deps_list
                .into_iter()
                .map(|module_bytes| {
                    let module = CompiledModule::deserialize_with_defaults(module_bytes.as_slice())
                        .expect("Downloaded module blob can't be deserialized");
                    verify_module_unmetered(&module)
                        .expect("Downloaded module blob failed verifier");
                    module
                })
                .collect()
        } else {
            vec![]
        }
    };

    let (compiled_module, source_map) = util::do_compile_module(&args.source_path, &deps_owned);
    if !args.no_verify {
        do_verify_module(&compiled_module, &deps_owned);
    }

    if args.output_source_maps {
        let source_map_bytes =
            bcs::to_bytes(&source_map).expect("Unable to serialize source maps for module");
        write_output(
            &source_path.with_extension(debug_info_extension),
            &source_map_bytes,
        );
    }

    let mut module = vec![];
    compiled_module
        .serialize_with_version(compiled_module.version, &mut module)
        .expect("Unable to serialize module");
    write_output(&source_path.with_extension(mv_extension), &module);
}
