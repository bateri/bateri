//! Turns a module built with the SDK (`wasm32-unknown-unknown`) into a component:
//! `componentize <module.wasm> <component.wasm>`.
//!
//! The module carries its interface (the SDK's bindings embed it); this is the step that
//! `wasm-tools component new` is, as a library call, so that building a page needs no tool
//! installed beside the compiler. Nothing is adapted or added: a module that imports anything
//! besides the page's interface (the SDK's modules import nothing else) keeps the import, and
//! the sandbox refuses to start it.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: componentize <module.wasm> <component.wasm>");
        return ExitCode::from(64);
    };
    match componentize(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("componentize: {input}: {why}");
            ExitCode::FAILURE
        }
    }
}

fn componentize(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let module = std::fs::read(input)?;
    let component = wit_component::ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    std::fs::write(output, component)?;
    Ok(())
}
