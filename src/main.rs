mod appimage;
mod ast;
mod bundle;
mod bytecode;
mod cli;
mod compiler;
mod config;
mod console;
mod environment;
mod error;
mod installer;
mod interpreter;
mod lexer;
mod modules;
mod parser;
mod repl;
mod runtime;
mod updater;
mod value;
mod vm;

use std::process::ExitCode;

fn main() -> ExitCode {
    cli::run()
}
