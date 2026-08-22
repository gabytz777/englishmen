use crate::compiler;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::vm::Vm;
use std::io::{self, Write};

pub fn run_repl() {
    println!("Englishmen REPL");
    println!("Type 'exit' or press Ctrl+C to quit.\n");

    // The REPL runs on the compiler and VM, like `em <file.em>`, so a snippet
    // behaves exactly as it would inside a program. Globals live in the VM and
    // persist between inputs.
    let mut vm = Vm::new();
    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() { "> " } else { ". " };
        print!("{prompt}");
        let _ = io::stdout().flush();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            println!();
            break;
        }
        let trimmed = line.trim_end();
        if buffer.is_empty() && (trimmed == "exit" || trimmed == "quit") {
            break;
        }

        buffer.push_str(trimmed);
        buffer.push('\n');

        if needs_more_input(&buffer) {
            continue;
        }

        let source = std::mem::take(&mut buffer);
        run_snippet(&mut vm, &source);
    }
}

fn needs_more_input(source: &str) -> bool {
    let openers = ["if ", "function ", "repeat ", "for "];
    let mut depth: i32 = 0;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if openers
            .iter()
            .any(|o| line == o.trim() || line.starts_with(o))
        {
            depth += 1;
        } else if line == "end" {
            depth -= 1;
        }
    }
    depth > 0
}

fn run_snippet(vm: &mut Vm, source: &str) {
    let tokens = match tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            println!("{}", e.render("repl", source));
            return;
        }
    };
    let program = match parse(tokens) {
        Ok(p) => p,
        Err(e) => {
            println!("{}", e.render("repl", source));
            return;
        }
    };
    let chunk = match compiler::compile(&program) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", e.render("repl", source));
            return;
        }
    };
    if let Err(e) = vm.run(chunk) {
        println!("{}", e.render("repl", source));
    }
}
