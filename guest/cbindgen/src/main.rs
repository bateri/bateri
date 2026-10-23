//! Writes the C bindings of a world of the page interface: `cbindgen <wit-dir> <world> <out-dir>`.
//!
//! The C half of what `wit_bindgen::generate!` is for the Rust modules: the interpreter module
//! (`luau/`) is C++ and links to the header and the source this writes, and to the object that
//! carries the world's description, which is what `componentize` reads the interface from. It is
//! the generator `wit-bindgen c` is, as a library call, so that the build needs no tool
//! installed beside the compiler.

use std::process::ExitCode;

use wit_bindgen_core::{Files, wit_parser::Resolve};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [wit, world, out] = args.as_slice() else {
        eprintln!("usage: cbindgen <wit-dir> <world> <out-dir>");
        return ExitCode::from(64);
    };
    match generate(wit, world, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("cbindgen: {why:#}");
            ExitCode::FAILURE
        }
    }
}

fn generate(wit: &str, world: &str, out: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(wit)?;
    let world = resolve.select_world(&[package], Some(world))?;
    let mut files = Files::default();
    wit_bindgen_c::Opts::default()
        .build()
        .generate(&resolve, world, &mut files)?;
    std::fs::create_dir_all(out)?;
    for (name, contents) in files.iter() {
        std::fs::write(std::path::Path::new(out).join(name), contents)?;
    }
    Ok(())
}
