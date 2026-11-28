//! Genuine self-compilation parity test.
//!
//! The host `em` binary is only a launcher here. `selfhost/full.enm` uses the
//! pure-Englishmen lexer, parser, AST→IR code generator and encoder to compile
//! `selfhost/bootstrap_compile.enm`, then runs the resulting IR on the
//! pure-Englishmen VM. The compiled compiler reads `examples/hello.enm` (the
//! argument after `--`) and must print exactly the same JSON summary line as a
//! native `em run selfhost/bootstrap_compile.enm examples/hello.enm`.
//!
//! This test is expected to fail until C1, the import bridge, and the parent
//! `--` pass-through land. See docs/fullselfhost/BOOTSTRAP.md for the contract
//! and the remaining host/import caveats.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BOOTSTRAP: &str = "selfhost/bootstrap_compile.enm";
const FULL: &str = "selfhost/full.enm";
const HELLO: &str = "examples/hello.enm";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "em_self_compile_{tag}_{}_{}",
        std::process::id(),
        n
    ))
}

/// Match the isolation convention in `tests/selfhost_full_pipeline.rs`.
fn isolated_env() -> [(&'static str, String); 3] {
    let dir = temp_dir("env");
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str], cwd: &Path) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .envs(isolated_env())
        .env_remove("EM_SELFTEST")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn require_success(output: &Output, label: &str) -> String {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        stderr_text(output)
    );
    stdout_text(output)
}

fn single_summary_line(output: &Output, label: &str) -> String {
    let stdout = require_success(output, label);
    let line = stdout.trim();
    assert!(!line.is_empty(), "{label} printed no JSON summary line");
    assert!(
        !line.contains('\n') && !line.contains('\r'),
        "{label} printed more than one line: {line}"
    );
    line.to_string()
}

/// Read a non-negative integer field out of a compact JSON summary.
fn json_number(summary: &str, key: &str) -> u64 {
    let needle = format!("\"{key}\":");
    let mut found = summary.match_indices(&needle);
    let (at, _) = found
        .next()
        .unwrap_or_else(|| panic!("summary is missing {key}: {summary}"));
    assert!(
        found.next().is_none(),
        "summary has more than one {key}: {summary}"
    );
    let rest = &summary[at + needle.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    assert!(!digits.is_empty(), "{key} is not a number: {summary}");
    digits.parse().expect("integer field")
}

/// Mirror `json_escape` in `src/builtins.rs`; `say` always prints strings.
fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pull the `output` array out of a compact JSON summary.
fn output_array(summary: &str) -> &str {
    let start = summary
        .find("\"output\":[")
        .unwrap_or_else(|| panic!("summary has no output array: {summary}"))
        + "\"output\":".len();
    let bytes = summary.as_bytes();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && *byte == b'\\' {
            escaped = true;
            continue;
        }
        match *byte {
            b'"' => quoted = !quoted,
            b'[' if !quoted => depth += 1,
            b']' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return &summary[start..start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated output array in summary: {summary}");
}

fn nonzero_stats(summary: &str, label: &str) -> (u64, u64, u64) {
    let stats = (
        json_number(summary, "astStatements"),
        json_number(summary, "irInstructions"),
        json_number(summary, "encodedBytes"),
    );
    assert!(
        stats.0 > 0 && stats.1 > 0 && stats.2 > 0,
        "{label} has a zero statistic: astStatements={}, irInstructions={}, encodedBytes={}\n{summary}",
        stats.0,
        stats.1,
        stats.2
    );
    stats
}

fn stats_value(output: &str, key: &str) -> u64 {
    let line = output
        .lines()
        .find(|line| line.trim_start().starts_with(key))
        .unwrap_or_else(|| panic!("missing '{key}' in stats output: {output}"));
    line.trim_start()[key.len()..]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("invalid '{key}' in stats output: {output}"))
}

fn number_before(text: &str, marker: &str) -> u64 {
    let end = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing '{marker}' in output: {text}"));
    let before = text[..end].trim_end();
    let start = before
        .rfind(|c: char| !c.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    before[start..]
        .parse()
        .unwrap_or_else(|_| panic!("no number before '{marker}' in output: {text}"))
}

fn build_instructions_and_bytes(stdout: &str) -> (u64, u64) {
    (
        number_before(stdout, " instructions"),
        number_before(stdout, " bytes"),
    )
}

/// Count the elements of the JSON array whose opening `[` is at `start`.
fn array_element_count(json: &str, start: usize) -> usize {
    let bytes = json.as_bytes();
    assert_eq!(bytes[start], b'[');
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut separators = 0usize;
    let mut seen_value = false;
    for &byte in &bytes[start + 1..] {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && byte == b'\\' {
            escaped = true;
            continue;
        }
        match byte {
            b'"' => {
                quoted = !quoted;
                seen_value = true;
            }
            b'[' | b'{' if !quoted => {
                depth += 1;
                seen_value = true;
            }
            b']' | b'}' if !quoted => {
                if depth == 0 {
                    return if seen_value { separators + 1 } else { 0 };
                }
                depth -= 1;
            }
            b',' if !quoted && depth == 0 => separators += 1,
            _ if !quoted && depth == 0 && !byte.is_ascii_whitespace() => seen_value = true,
            _ => {}
        }
    }
    panic!("unterminated JSON array");
}

fn top_level_statements(ast_json: &str) -> u64 {
    let start = ast_json
        .find("\"statements\":[")
        .unwrap_or_else(|| panic!("AST JSON has no statements array: {ast_json}"))
        + "\"statements\":".len();
    array_element_count(ast_json, start) as u64
}

fn temp_emb(name: &str) -> PathBuf {
    let dir = temp_dir("emb");
    fs::create_dir_all(&dir).expect("create temp directory for .emb output");
    dir.join(name)
}

#[test]
fn self_compiled_compiler_prints_exactly_the_native_summary_line() {
    let root = manifest_dir();

    // 1. Native oracle: the host compiles and runs bootstrap_compile.enm,
    // which compiles examples/hello.enm and prints one summary line.
    let native = run(&["run", BOOTSTRAP, HELLO], &root);
    let native_summary = single_summary_line(&native, "em run selfhost/bootstrap_compile.enm");
    assert_eq!(
        output_array(&native_summary),
        "[]",
        "a compile-only bootstrap run must not execute the program it compiles: {native_summary}"
    );
    let native_stats = nonzero_stats(&native_summary, "native bootstrap summary");

    // Practical native stats-style checks for the program the generated
    // compiler reads: --stats reports instructions, and a build reports both
    // instructions and encoded bytes.
    let stats = run(&["build", HELLO, "--stats"], &root);
    let stats_stdout = require_success(&stats, "em build examples/hello.enm --stats");
    let native_program_instructions = stats_value(&stats_stdout, "instructions:");

    let built_path = temp_emb("hello.emb");
    let built_arg = built_path.to_str().expect("utf-8 temp path");
    let built = run(&["build", HELLO, "-o", built_arg], &root);
    let built_stdout = require_success(&built, "em build examples/hello.enm -o <tmp>");
    let (built_instructions, built_bytes) = build_instructions_and_bytes(&built_stdout);
    let _ = fs::remove_file(&built_path);

    let ast = run(&["build", HELLO, "--emit", "ast-json"], &root);
    let ast_stdout = require_success(&ast, "em build examples/hello.enm --emit ast-json");

    assert_eq!(
        native_stats.0,
        top_level_statements(&ast_stdout),
        "native bootstrap summary disagrees with the Rust AST for {HELLO}"
    );
    assert_eq!(
        native_stats.1, native_program_instructions,
        "native bootstrap summary disagrees with `em build --stats` for {HELLO}"
    );
    assert_eq!(
        native_stats.1, built_instructions,
        "native bootstrap summary disagrees with the build instruction count for {HELLO}"
    );
    assert_eq!(
        native_stats.2, built_bytes,
        "native bootstrap summary disagrees with the native .emb byte count for {HELLO}"
    );

    // Native reference numbers for the compiler source itself. The Rust and
    // pure code generators are not guaranteed to emit identical instruction
    // sequences, so only the parser-level statement count is compared against
    // the self-hosted summary below; the other counts are diagnostic context.
    let compiler_ast = run(&["build", BOOTSTRAP, "--emit", "ast-json"], &root);
    let compiler_ast_stdout = require_success(
        &compiler_ast,
        "em build selfhost/bootstrap_compile.enm --emit ast-json",
    );
    let compiler_statements = top_level_statements(&compiler_ast_stdout);

    let compiler_stats_out = run(&["build", BOOTSTRAP, "--stats"], &root);
    let compiler_stats_stdout = require_success(
        &compiler_stats_out,
        "em build selfhost/bootstrap_compile.enm --stats",
    );
    let compiler_native_instructions = stats_value(&compiler_stats_stdout, "instructions:");
    let compiler_emb = temp_emb("bootstrap.emb");
    let compiler_emb_arg = compiler_emb.to_str().expect("utf-8 temp path");
    let compiler_built = run(&["build", BOOTSTRAP, "-o", compiler_emb_arg], &root);
    let compiler_built_stdout = require_success(
        &compiler_built,
        "em build selfhost/bootstrap_compile.enm -o <tmp>",
    );
    let (compiler_built_instructions, compiler_built_bytes) =
        build_instructions_and_bytes(&compiler_built_stdout);
    let _ = fs::remove_file(&compiler_emb);
    assert_eq!(
        compiler_native_instructions, compiler_built_instructions,
        "native --stats and build disagree for {BOOTSTRAP}"
    );

    // 2. Self-hosted run: full.enm compiles the compiler source with the pure
    // compiler, encodes it, and executes that IR on the pure Englishmen VM.
    let self_hosted = run(&["run", FULL, BOOTSTRAP, "--", HELLO], &root);
    let full_summary = single_summary_line(&self_hosted, "em run selfhost/full.enm -- ...");
    let full_stats = nonzero_stats(&full_summary, "self-hosted full.enm summary");

    assert_eq!(
        full_stats.0, compiler_statements,
        "pure parser disagrees with the Rust AST for {BOOTSTRAP}; native compiler stats: \
         instructions={compiler_native_instructions}, encoded_bytes={compiler_built_bytes}"
    );
    assert!(
        full_stats.1 > 0 && full_stats.2 > 0,
        "self-hosted compiler stats must be nonzero; native compiler stats: \
         instructions={compiler_native_instructions}/{compiler_built_instructions}, \
         encoded_bytes={compiler_built_bytes}"
    );

    // 3. The code produced by the pure compiler, running on the pure VM, must
    // print exactly the native oracle line — including all three statistics.
    let expected_output = format!("[{}]", json_escape(&native_summary));
    assert_eq!(
        output_array(&full_summary),
        expected_output.as_str(),
        "the self-compiled compiler did not reproduce the native summary line\n\
         native:      {native_summary}\n\
         self-hosted: {full_summary}"
    );
}
