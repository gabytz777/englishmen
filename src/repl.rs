//! Interactive REPL for Englishmen.
//!
//! Snippets run on the compiler and VM, just like `em <file.enm>`, so a REPL
//! input behaves exactly as it would inside a program. Globals live in the VM
//! and persist between inputs; only `:clear` starts a fresh one.

use crate::compiler;
use crate::environment::Environment;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::vm::Vm;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPL_NAME: &str = "repl";
/// Variable used by `:type` inside a scratch scope, never in the session VM.
const TYPE_PROBE: &str = "__repl_type_probe";
/// Guard against accidentally running a huge `:repeat` loop.
const MAX_REPEAT: usize = 1000;

/// A `:` meta command, or the plain `exit`/`quit` shorthand.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Help,
    Vars,
    Clear,
    History,
    Quit,
    Load(String),
    Save(String),
    Time(String),
    About,
    Echo(String),
    Source,
    Reset,
    Again,
    Eval(String),
    Type(String),
    Repeat { count: usize, source: String },
    /// A malformed command, with the message to show to the user.
    Invalid(String),
}

/// What one input line means to the REPL.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Input {
    Command(Command),
    Source(String),
    Blank,
}

struct Repl {
    vm: Vm,
    /// Source lines waiting for an `end` to close their blocks.
    buffer: String,
    /// Every non-empty line entered in this session, including meta commands.
    history: Vec<String>,
    /// Completed source snippets, used by `:save`.
    session_source: String,
    /// Most recently executed source, used by `:again`.
    last_source: String,
    quit: bool,
}

impl Repl {
    fn new() -> Self {
        Repl {
            vm: Vm::new(),
            buffer: String::new(),
            history: Vec::new(),
            session_source: String::new(),
            last_source: String::new(),
            quit: false,
        }
    }

    fn handle_line(&mut self, raw_line: &str) {
        let line = raw_line.trim_end();
        if !line.trim().is_empty() {
            self.history.push(line.to_string());
        }

        // While a block is open every line is source, including lines that
        // would otherwise look like a command. This preserves multiline input.
        match classify_input(line, !self.buffer.is_empty()) {
            Input::Blank => {}
            Input::Command(command) => self.run_command(command),
            Input::Source(part) => {
                self.buffer.push_str(&part);
                self.buffer.push('\n');
                if needs_more_input(&self.buffer) {
                    return;
                }
                let source = std::mem::take(&mut self.buffer);
                self.execute_source(&source, REPL_NAME, true);
            }
        }
    }

    /// Execute a snippet in the session VM. `record` controls whether it also
    /// becomes part of the source written by `:save`.
    fn execute_source(&mut self, source: &str, filename: &str, record: bool) -> bool {
        if source.trim().is_empty() {
            return false;
        }
        self.last_source = source.to_string();
        if record {
            self.session_source.push_str(source);
            if !self.session_source.ends_with('\n') {
                self.session_source.push('\n');
            }
        }
        run_snippet(&mut self.vm, source, filename)
    }

    /// `:again` re-runs the last snippet without adding it to `:save` output.
    fn repeat_last_source(&mut self) {
        if self.last_source.trim().is_empty() {
            println!("Nothing to repeat: no source entered yet.");
            return;
        }
        let source = self.last_source.clone();
        self.execute_source(&source, REPL_NAME, false);
    }

    /// Unlike `:clear`, this also forgets history, session source and `:again`.
    fn reset_session(&mut self) {
        self.vm = Vm::new();
        self.buffer.clear();
        self.history.clear();
        self.session_source.clear();
        self.last_source.clear();
        println!("Session reset. Globals, history and source are cleared.");
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::Help => print_lines(&help_lines()),
            Command::Vars => print_lines(&var_lines(&self.vm.globals)),
            Command::Clear => {
                self.vm = Vm::new();
                self.buffer.clear();
                println!("Globals cleared. Session history and source are kept.");
            }
            Command::History => print_lines(&history_lines(&self.history)),
            Command::Quit => self.quit = true,
            Command::Load(path) => load_file(&mut self.vm, &path),
            Command::Save(path) => {
                save_source(&path, &self.session_source);
            }
            Command::Time(expression) => {
                self.last_source = source_for_time(&expression);
                time_expression(&mut self.vm, &expression);
            }
            Command::About => print_lines(&about_lines()),
            Command::Echo(text) => println!("{text}"),
            Command::Source => print_lines(&source_lines(&self.session_source)),
            Command::Reset => self.reset_session(),
            Command::Again => self.repeat_last_source(),
            Command::Eval(expression) => {
                let source = source_for_expression(&expression);
                self.execute_source(&source, REPL_NAME, false);
            }
            Command::Type(expression) => type_expression(&self.vm, &expression),
            Command::Repeat { count, source } => {
                for _ in 0..count {
                    self.execute_source(&source, REPL_NAME, false);
                }
                println!("Ran source {count} time{}.", if count == 1 { "" } else { "s" });
            }
            Command::Invalid(message) => println!("{message}"),
        }
    }
}

pub fn run_repl() {
    print_lines(&banner_lines());
    let mut repl = Repl::new();
    while !repl.quit {
        print!("{}", prompt_for(&repl.buffer));
        let _ = io::stdout().flush();
        let Some(line) = read_line() else {
            println!();
            break;
        };
        repl.handle_line(&line);
    }
}

/// Read one line, treating EOF (or a read error) as a clean exit.
fn read_line() -> Option<String> {
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line),
    }
}

fn prompt_for(buffer: &str) -> &'static str {
    if buffer.is_empty() {
        "> "
    } else {
        "...> "
    }
}

/// Split a meta command into its name and argument, if any.
fn split_command(body: &str) -> (&str, &str) {
    let body = body.trim();
    match body.find(char::is_whitespace) {
        Some(index) => (body[..index].trim(), body[index..].trim()),
        None => (body, ""),
    }
}

/// Accept a quoted path so `:load "my program.enm"` works too.
fn unquote_argument(argument: &str) -> &str {
    let argument = argument.trim();
    if argument.len() >= 2
        && ((argument.starts_with('"') && argument.ends_with('"'))
            || (argument.starts_with('\'') && argument.ends_with('\'')))
    {
        &argument[1..argument.len() - 1]
    } else {
        argument
    }
}

fn no_argument(name: &str, argument: &str, command: Command) -> Command {
    if argument.is_empty() {
        command
    } else {
        Command::Invalid(format!(
            "':{}' takes no arguments",
            name.to_ascii_lowercase()
        ))
    }
}

fn with_argument(
    name: &str,
    argument: &str,
    what: &str,
    unquote: bool,
    build: fn(String) -> Command,
) -> Command {
    let argument = argument.trim();
    let value = if unquote {
        unquote_argument(argument)
    } else {
        argument
    };
    if value.is_empty() {
        Command::Invalid(format!(
            "':{}' needs {what}",
            name.to_ascii_lowercase()
        ))
    } else {
        build(value.to_string())
    }
}

/// Parse `:repeat <count> <source>`; the source is a single statement.
fn repeat_command(argument: &str) -> Command {
    let (count_text, source) = split_command(argument);
    if count_text.is_empty() || source.is_empty() {
        return Command::Invalid("':repeat' needs a count and source".to_string());
    }
    match count_text.parse::<usize>() {
        Ok(0) => Command::Invalid("':repeat' count must be at least 1".to_string()),
        Ok(count) if count > MAX_REPEAT => Command::Invalid(format!(
            "':repeat' count must not exceed {MAX_REPEAT}"
        )),
        Ok(count) => Command::Repeat {
            count,
            source: source.to_string(),
        },
        Err(_) => Command::Invalid(format!(
            "':repeat' count must be a whole number, got '{count_text}'"
        )),
    }
}

/// Turn one input line into an action. `continuing` is true while a multiline
/// block is being collected; in that case the line is always source.
fn classify_input(line: &str, continuing: bool) -> Input {
    if continuing {
        return Input::Source(line.to_string());
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Input::Blank;
    }

    if let Some(body) = trimmed.strip_prefix(':') {
        let (name, argument) = split_command(body);
        let command = match name.to_ascii_lowercase().as_str() {
            "help" => no_argument(name, argument, Command::Help),
            "vars" => no_argument(name, argument, Command::Vars),
            "clear" => no_argument(name, argument, Command::Clear),
            "history" => no_argument(name, argument, Command::History),
            "quit" | "exit" => no_argument(name, argument, Command::Quit),
            "about" => no_argument(name, argument, Command::About),
            "echo" => with_argument(name, argument, "some text", false, Command::Echo),
            "source" => no_argument(name, argument, Command::Source),
            "reset" => no_argument(name, argument, Command::Reset),
            "again" => no_argument(name, argument, Command::Again),
            "eval" => with_argument(name, argument, "an expression", false, Command::Eval),
            "type" => with_argument(name, argument, "an expression", false, Command::Type),
            "repeat" => repeat_command(argument),
            "load" => with_argument(name, argument, "a file name", true, Command::Load),
            "save" => with_argument(name, argument, "a file name", true, Command::Save),
            // Expressions keep their quotes: `:time "hi"` must pass a string
            // literal through to the compiler, not strip it like a file path.
            "time" => with_argument(name, argument, "an expression", false, Command::Time),
            other => Command::Invalid(format!(
                "Unknown command ':{other}'. Type ':help' for the command list."
            )),
        };
        return Input::Command(command);
    }

    // Keep the original bare `exit`/`quit` spelling working.
    if trimmed == "exit" || trimmed == "quit" {
        return Input::Command(Command::Quit);
    }
    Input::Source(line.to_string())
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

fn banner_lines() -> Vec<String> {
    vec![
        format!("Englishmen REPL {VERSION}"),
        "An English-inspired, beginner-friendly programming language.".to_string(),
        "Type ':help' for commands, ':quit' to exit.".to_string(),
        "End a block with 'end'; the prompt becomes '...>' while it is open.".to_string(),
        "Use ':source' to review input, ':again' to re-run it, ':reset' to start over.".to_string(),
        String::new(),
    ]
}

fn help_lines() -> Vec<String> {
    vec![
        "REPL commands:".to_string(),
        "  :help            Show this list".to_string(),
        "  :vars            List globals with their values and types".to_string(),
        "  :clear           Clear all globals (start a fresh VM)".to_string(),
        "  :history         Show commands entered in this session".to_string(),
        "  :quit, :exit     Leave the REPL (or type 'quit')".to_string(),
        "  :load <file>     Load and run a .enm file in this VM".to_string(),
        "  :save <file>     Write entered session source to a .enm file".to_string(),
        "  :time <expr>     Run and time a single expression".to_string(),
        "  :about           Show version and language information".to_string(),
        "  :echo <text>     Print a line of text".to_string(),
        "  :source          Show numbered source entered in this session".to_string(),
        "  :again           Re-run the most recent source snippet".to_string(),
        "  :eval <expr>     Evaluate an expression and show its value".to_string(),
        "  :type <expr>     Show an expression's value and type".to_string(),
        "  :repeat <n> <src> Run one source statement n times".to_string(),
        "  :reset           Clear globals, history, source and prompt state".to_string(),
    ]
}

fn about_lines() -> Vec<String> {
    vec![
        format!("Englishmen {VERSION}"),
        "An English-inspired, beginner-friendly programming language.".to_string(),
        "Source files use .enm; compiled programs use .emb.".to_string(),
        "Type ':help' for REPL commands or 'em --help' for CLI usage.".to_string(),
    ]
}

fn var_lines(globals: &Environment) -> Vec<String> {
    let mut names = globals.names();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return vec!["No globals defined yet.".to_string()];
    }
    let mut lines = vec!["Globals:".to_string()];
    for name in names {
        if let Some(value) = globals.get(&name) {
            lines.push(format!(
                "  {name} = {} ({})",
                value.display(),
                value.type_name()
            ));
        }
    }
    lines
}

fn history_lines(history: &[String]) -> Vec<String> {
    if history.is_empty() {
        return vec!["No commands entered yet.".to_string()];
    }
    history
        .iter()
        .enumerate()
        .map(|(index, entry)| format!("{:>4}  {entry}", index + 1))
        .collect()
}

fn source_lines(source: &str) -> Vec<String> {
    if source.trim().is_empty() {
        return vec!["Nothing entered yet.".to_string()];
    }
    let mut lines = vec!["Session source:".to_string()];
    for (index, line) in source.lines().enumerate() {
        lines.push(format!("{:>4}  {line}", index + 1));
    }
    lines
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_source_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("enm"))
}

/// Wrap an expression in `say` so its value is visible as usual.
fn source_for_expression(expression: &str) -> String {
    format!("say({})\n", expression.trim())
}

/// Wrap a timed expression in `say` so its value is visible as usual.
fn source_for_time(expression: &str) -> String {
    source_for_expression(expression)
}

/// Store a value in a scratch scope so `:type` can inspect it without
/// declaring a variable in the session VM.
fn source_for_type(expression: &str) -> String {
    format!("{TYPE_PROBE} = {}\n", expression.trim())
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1000.0)
}

fn load_file(vm: &mut Vm, path: &str) {
    if !is_source_file(path) {
        println!("Error: ':load' expects a .enm source file, got '{path}'");
        return;
    }
    let name = display_name(Path::new(path));
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            println!("Error: Could not read file '{path}': {error}");
            return;
        }
    };
    println!("Loaded {name}");
    run_snippet(vm, &source, &name);
}

fn save_source(path: &str, source: &str) -> bool {
    if !is_source_file(path) {
        println!("Error: ':save' expects a .enm file name, got '{path}'");
        return false;
    }
    if source.trim().is_empty() {
        println!("Nothing to save: no source entered yet.");
        return false;
    }
    match fs::write(path, source) {
        Ok(()) => {
            println!("Saved session source to {path}");
            true
        }
        Err(error) => {
            println!("Error: Could not write file '{path}': {error}");
            false
        }
    }
}

fn time_expression(vm: &mut Vm, expression: &str) {
    if expression.trim().is_empty() {
        println!("Error: ':time' needs an expression");
        return;
    }
    let source = source_for_time(expression);
    let started = Instant::now();
    run_snippet(vm, &source, REPL_NAME);
    println!("Time: {}", format_duration(started.elapsed()));
}

fn type_expression(vm: &Vm, expression: &str) {
    if expression.trim().is_empty() {
        println!("Error: ':type' needs an expression");
        return;
    }
    let source = source_for_type(expression);
    let mut scratch = Vm::new();
    scratch.globals = Environment::child(&vm.globals);
    if run_snippet(&mut scratch, &source, REPL_NAME) {
        match scratch.globals.get(TYPE_PROBE) {
            Some(value) => println!("{} ({})", value.debug_display(), value.type_name()),
            None => println!("Error: ':type' could not read the expression's value"),
        }
    }
}

/// Compile and run a snippet in the REPL's VM. Returns whether it succeeded.
fn run_snippet(vm: &mut Vm, source: &str, filename: &str) -> bool {
    let tokens = match tokenize(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            println!("{}", error.render(filename, source));
            return false;
        }
    };
    let program = match parse(tokens) {
        Ok(program) => program,
        Err(error) => {
            println!("{}", error.render(filename, source));
            return false;
        }
    };
    let chunk = match compiler::compile(&program) {
        Ok(chunk) => chunk,
        Err(error) => {
            println!("{}", error.render(filename, source));
            return false;
        }
    };
    match vm.run(chunk) {
        Ok(()) => true,
        Err(error) => {
            println!("{}", error.render(filename, source));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn command(input: &str) -> Command {
        match classify_input(input, false) {
            Input::Command(c) => c,
            other => panic!("expected a command from {input:?}, got {other:?}"),
        }
    }

    fn temp_path(tag: &str, extension: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("em-repl-{tag}-{}-{nanos}.{extension}", std::process::id()))
    }

    #[test]
    fn every_command_is_recognized() {
        assert_eq!(command(":help"), Command::Help);
        assert_eq!(command(":vars"), Command::Vars);
        assert_eq!(command(":clear"), Command::Clear);
        assert_eq!(command(":history"), Command::History);
        assert_eq!(command(":quit"), Command::Quit);
        assert_eq!(command(":exit"), Command::Quit);
        assert_eq!(command(":about"), Command::About);
        assert_eq!(command("quit"), Command::Quit);
        assert_eq!(command("exit"), Command::Quit);
        assert_eq!(command(":load main.enm"), Command::Load("main.enm".to_string()));
        assert_eq!(command(":save out.enm"), Command::Save("out.enm".to_string()));
        assert_eq!(command(":time 2 + 2"), Command::Time("2 + 2".to_string()));
        assert_eq!(command(":echo hello there"), Command::Echo("hello there".to_string()));
        assert_eq!(command(":source"), Command::Source);
        assert_eq!(command(":reset"), Command::Reset);
        assert_eq!(command(":again"), Command::Again);
        assert_eq!(command(":eval 2 + 2"), Command::Eval("2 + 2".to_string()));
        assert_eq!(command(":type \"hi\""), Command::Type("\"hi\"".to_string()));
        assert_eq!(
            command(":repeat 3 say(1)"),
            Command::Repeat { count: 3, source: "say(1)".to_string() }
        );
    }

    #[test]
    fn commands_ignore_case_and_extra_spaces() {
        assert_eq!(command("  :HELP   "), Command::Help);
        assert_eq!(command(":  help  "), Command::Help);
        assert_eq!(command(":\tvars"), Command::Vars);
        assert_eq!(command(":Load   \"my program.enm\"  "), Command::Load("my program.enm".to_string()));
        assert_eq!(command(":time   'x * 2'"), Command::Time("'x * 2'".to_string()));
        assert_eq!(command(":time \"hello\""), Command::Time("\"hello\"".to_string()));
        assert_eq!(command(":ECHO   hello  "), Command::Echo("hello".to_string()));
        assert_eq!(
            command(":RePeAt 2 x = x + 1"),
            Command::Repeat {
                count: 2,
                source: "x = x + 1".to_string()
            }
        );
        assert_eq!(command(":eval \"a + b\""), Command::Eval("\"a + b\"".to_string()));
    }

    #[test]
    fn unknown_commands_and_bad_arguments_are_reported() {
        assert!(matches!(command(":nope"), Command::Invalid(message) if message.contains("Unknown command")));
        assert!(matches!(command(":load"), Command::Invalid(message) if message.contains("needs a file name")));
        assert!(matches!(command(":save"), Command::Invalid(message) if message.contains("needs a file name")));
        assert!(matches!(command(":time"), Command::Invalid(message) if message.contains("needs an expression")));
        assert!(matches!(command(":vars extra"), Command::Invalid(message) if message.contains("takes no arguments")));
        assert!(matches!(command(":echo"), Command::Invalid(message) if message.contains("needs some text")));
        assert!(matches!(command(":eval"), Command::Invalid(message) if message.contains("needs an expression")));
        assert!(matches!(command(":type"), Command::Invalid(message) if message.contains("needs an expression")));
        assert!(matches!(command(":source x"), Command::Invalid(message) if message.contains("takes no arguments")));
        assert!(matches!(command(":reset x"), Command::Invalid(message) if message.contains("takes no arguments")));
        assert!(matches!(command(":again x"), Command::Invalid(message) if message.contains("takes no arguments")));
        assert!(matches!(command(":repeat"), Command::Invalid(message) if message.contains("needs a count and source")));
        assert!(matches!(command(":repeat 3"), Command::Invalid(message) if message.contains("needs a count and source")));
        assert!(matches!(command(":repeat lots say(1)"), Command::Invalid(message) if message.contains("whole number")));
        assert!(matches!(command(":repeat 0 say(1)"), Command::Invalid(message) if message.contains("at least 1")));
        assert!(matches!(command(":repeat 1001 say(1)"), Command::Invalid(message) if message.contains("must not exceed")));
    }

    #[test]
    fn source_and_blank_lines_are_classified() {
        assert_eq!(classify_input("x = 1", false), Input::Source("x = 1".to_string()));
        assert_eq!(classify_input("   ", false), Input::Blank);
        assert_eq!(classify_input("", false), Input::Blank);
    }

    #[test]
    fn continuation_lines_are_never_commands() {
        assert_eq!(
            classify_input(":quit", true),
            Input::Source(":quit".to_string())
        );
        assert_eq!(
            classify_input("exit", true),
            Input::Source("exit".to_string())
        );
        assert_eq!(prompt_for(""), "> ");
        assert_eq!(prompt_for("if true\n"), "...> ");
    }

    #[test]
    fn multiline_buffering_waits_for_end() {
        assert!(needs_more_input("function greet()\n"));
        assert!(needs_more_input("if true\n  say(1)\n"));
        assert!(needs_more_input("for item in things\n"));
        assert!(needs_more_input("repeat 3\n"));
        assert!(needs_more_input("if true\n  if false\n    say(1)\n"));
        assert!(!needs_more_input("if true\n  say(1)\nend\n"));
        assert!(!needs_more_input("say(1)\n"));
    }

    #[test]
    fn timed_expression_is_wrapped_in_say() {
        assert_eq!(source_for_time(" 2 + 2 "), "say(2 + 2)\n");
        assert_eq!(source_for_time("\"hi\""), "say(\"hi\")\n");
        assert_eq!(source_for_expression(" x + 1 "), "say(x + 1)\n");
        assert_eq!(source_for_type(" x + 1 "), "__repl_type_probe = x + 1\n");
        assert_eq!(format_duration(Duration::from_micros(1500)), "1.500 ms");
        assert_eq!(format_duration(Duration::from_secs(1)), "1000.000 ms");
    }

    #[test]
    fn variable_listing_is_sorted_and_typed() {
        let environment = Environment::new();
        environment.define("b", Value::Number(2.0));
        environment.define("a", Value::String("hello".to_string()));
        environment.define("flag", Value::Bool(true));
        let lines = var_lines(&environment);
        assert_eq!(lines.first().map(String::as_str), Some("Globals:"));
        let a = lines.iter().position(|line| line == "  a = hello (string)").unwrap();
        let b = lines.iter().position(|line| line == "  b = 2 (number)").unwrap();
        let flag = lines.iter().position(|line| line == "  flag = true (boolean)").unwrap();
        assert!(a < b && b < flag);
    }

    #[test]
    fn builtin_globals_are_listed() {
        let lines = var_lines(&Environment::new());
        assert_eq!(lines.first().map(String::as_str), Some("Globals:"));
        assert!(
            lines.iter().any(|line| line.starts_with("  print = ")),
            "the global print builtin must appear in :vars"
        );
        assert!(lines.iter().skip(1).all(|line| line.contains("(function)")));
    }

    #[test]
    fn history_is_numbered_and_keeps_commands() {
        let history = vec!["x = 1".to_string(), ":vars".to_string()];
        let lines = history_lines(&history);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("1  x = 1"));
        assert!(lines[1].ends_with("2  :vars"));
        assert_eq!(history_lines(&[]), vec!["No commands entered yet.".to_string()]);
    }

    #[test]
    fn help_banner_and_about_cover_the_feature_set() {
        let help = help_lines().join("\n");
        for name in [
            ":help", ":vars", ":clear", ":history", ":quit", ":exit", ":load", ":save", ":time",
            ":about", ":echo", ":source", ":again", ":eval", ":type", ":repeat", ":reset",
        ] {
            assert!(help.contains(name), "missing {name} from help");
        }
        let banner = banner_lines().join("\n");
        assert!(banner.contains(":help"));
        assert!(banner.contains("...>"));
        assert!(banner.contains(VERSION));
        let about = about_lines().join("\n");
        assert!(about.contains(VERSION));
        assert!(about.contains(".enm"));
    }

    #[test]
    fn only_enm_files_are_accepted_for_load_and_save() {
        assert!(is_source_file("main.enm"));
        assert!(is_source_file("MAIN.ENM"));
        assert!(is_source_file("dir/main.enm"));
        assert!(!is_source_file("main.em"));
        assert!(!is_source_file("main.emb"));
        assert!(!is_source_file("main"));
        assert_eq!(display_name(Path::new("dir/main.enm")), "main.enm");
        assert_eq!(display_name(Path::new("")), "");
    }

    #[test]
    fn save_writes_the_entered_session_source() {
        let path = temp_path("save", "enm");
        let source = "x = 1\nsay(x)\n";
        assert!(save_source(path.to_str().unwrap(), source));
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        fs::remove_file(&path).unwrap();

        let bad = temp_path("save-empty", "enm");
        assert!(!save_source(bad.to_str().unwrap(), "  \n"));
        assert!(!bad.exists());
        assert!(!save_source("session.txt", source));
    }

    #[test]
    fn load_runs_a_file_in_the_current_vm() {
        let path = temp_path("load", "enm");
        fs::write(&path, "x = 41\n").unwrap();
        let mut repl = Repl::new();
        repl.run_command(Command::Load(path.to_str().unwrap().to_string()));
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 41.0));
        assert!(run_snippet(&mut repl.vm, "x = x + 1\n", REPL_NAME));
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 42.0));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn clear_freshens_the_vm_but_keeps_history_and_source() {
        let mut repl = Repl::new();
        repl.handle_line("x = 1\n");
        repl.handle_line(":vars\n");
        assert_eq!(repl.session_source, "x = 1\n");
        assert_eq!(repl.history.len(), 2);
        repl.run_command(Command::Clear);
        assert!(repl.vm.globals.get("x").is_none());
        assert!(repl.buffer.is_empty());
        assert_eq!(repl.history.len(), 2);
        assert_eq!(repl.session_source, "x = 1\n");
    }

    #[test]
    fn quit_command_ends_the_session() {
        let mut repl = Repl::new();
        assert!(!repl.quit);
        repl.run_command(Command::Quit);
        assert!(repl.quit);
    }

    #[test]
    fn timed_and_invalid_commands_do_not_disturb_the_vm() {
        let mut repl = Repl::new();
        repl.handle_line("x = 7\n");
        repl.run_command(Command::Time("x + 1".to_string()));
        repl.run_command(Command::Invalid("oops".to_string()));
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 7.0));
        assert_eq!(repl.session_source, "x = 7\n");
        assert_eq!(repl.last_source, "say(x + 1)\n");
    }

    #[test]
    fn quoted_time_expression_keeps_string_literals() {
        let mut repl = Repl::new();
        let Command::Time(expression) = command(":time \"hello\"") else {
            panic!("expected :time command");
        };
        assert_eq!(expression, "\"hello\"");
        let source = source_for_time(&expression);
        assert_eq!(source, "say(\"hello\")\n");
        assert!(run_snippet(&mut repl.vm, &source, REPL_NAME));
    }

    #[test]
    fn snippet_errors_are_reported_instead_of_panicking() {
        let mut vm = Vm::new();
        assert!(!run_snippet(&mut vm, "x = 1 / 0\n", REPL_NAME));
        assert!(!run_snippet(&mut vm, ":not englishmen\n", REPL_NAME));
        assert!(run_snippet(&mut vm, "x = 1\n", REPL_NAME));
        assert!(matches!(vm.globals.get("x"), Some(Value::Number(n)) if n == 1.0));
    }

    #[test]
    fn source_listing_is_numbered_and_reports_empty_state() {
        assert_eq!(source_lines("  \n"), vec!["Nothing entered yet.".to_string()]);
        let lines = source_lines("x = 1\nsay(x)\n");
        assert_eq!(lines.first().map(String::as_str), Some("Session source:"));
        assert!(lines[1].ends_with("1  x = 1"));
        assert!(lines[2].ends_with("2  say(x)"));
    }

    #[test]
    fn again_reruns_last_source_without_adding_it_to_saved_source() {
        let mut repl = Repl::new();
        repl.handle_line("x = 1\n");
        repl.handle_line("x = x + 1\n");
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 2.0));
        assert_eq!(repl.session_source, "x = 1\nx = x + 1\n");

        repl.run_command(Command::Again);
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 3.0));
        assert_eq!(repl.session_source, "x = 1\nx = x + 1\n");
        assert_eq!(repl.last_source, "x = x + 1\n");

        // `:again` in a fresh session is harmless.
        Repl::new().run_command(Command::Again);
    }

    #[test]
    fn eval_runs_an_expression_without_recording_it_as_session_source() {
        let mut repl = Repl::new();
        repl.handle_line("x = 4\n");
        repl.run_command(Command::Eval("x * 2".to_string()));
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 4.0));
        assert_eq!(repl.session_source, "x = 4\n");
        assert_eq!(repl.last_source, "say(x * 2)\n");
    }

    #[test]
    fn type_inspects_a_value_in_a_scratch_scope() {
        let mut repl = Repl::new();
        repl.handle_line("x = 1\n");
        repl.run_command(Command::Type("x + 1".to_string()));
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 1.0));
        assert!(
            !repl.vm.globals.has(TYPE_PROBE),
            "the :type probe must not leak into session globals"
        );

        let vm = Vm::new();
        type_expression(&vm, "undefined_name");
        assert!(!vm.globals.has(TYPE_PROBE));
    }

    #[test]
    fn repeat_runs_source_the_requested_number_of_times() {
        let mut repl = Repl::new();
        repl.handle_line("x = 0\n");
        repl.run_command(Command::Repeat {
            count: 3,
            source: "x = x + 1".to_string(),
        });
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 3.0));
        assert_eq!(repl.session_source, "x = 0\n");
        assert_eq!(repl.last_source, "x = x + 1");
    }

    #[test]
    fn reset_clears_all_session_state_and_stays_usable() {
        let mut repl = Repl::new();
        repl.handle_line("x = 1\n");
        repl.handle_line(":vars\n");
        repl.run_command(Command::Reset);
        assert!(repl.vm.globals.get("x").is_none());
        assert!(repl.history.is_empty());
        assert!(repl.session_source.is_empty());
        assert!(repl.last_source.is_empty());
        assert!(repl.buffer.is_empty());
        assert!(!repl.quit);

        repl.handle_line("y = 2\n");
        assert!(matches!(repl.vm.globals.get("y"), Some(Value::Number(n)) if n == 2.0));
        assert_eq!(repl.history.len(), 1);
    }

    #[test]
    fn echo_does_not_change_session_state() {
        let mut repl = Repl::new();
        repl.handle_line("x = 1\n");
        repl.run_command(Command::Echo("hello".to_string()));
        assert_eq!(repl.session_source, "x = 1\n");
        assert!(matches!(repl.vm.globals.get("x"), Some(Value::Number(n)) if n == 1.0));
    }
}
