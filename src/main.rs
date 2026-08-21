mod ast;
mod cli;
mod config;
mod environment;
mod error;
mod installer;
mod interpreter;
mod lexer;
mod modules;
mod parser;
mod repl;
mod runtime;
mod value;

use std::process::ExitCode;

fn main() -> ExitCode {
    cli::run()
}
