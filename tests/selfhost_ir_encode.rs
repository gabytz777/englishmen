//! Byte-parity tests for the pure-Englishmen `emb` IR encoder.
//!
//! A generated Englishmen program concatenates `selfhost/ir_encode.enm` with a
//! fixture table. For every fixture it prints `label`, `jsonEncode(ieEncode(ir))`
//! and `jsonEncode(emb.encode(ir))`; this test asserts the last two lines are
//! identical. Native `emb` is only the oracle — the implementation under test
//! is the `.enm` file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const OPS: &[&str] = &[
    "LoadConst",
    "Pop",
    "GetVar",
    "SetVar",
    "DefineVar",
    "GetProp",
    "SetProp",
    "GetVarOrObject",
    "GetPropOrObject",
    "GetIndex",
    "SetIndex",
    "MakeArray",
    "MakeObject",
    "MakeClosure",
    "Call",
    "MethodCall",
    "Say",
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Mod",
    "FloorDiv",
    "Pow",
    "BitAnd",
    "BitOr",
    "BitXor",
    "Shl",
    "Shr",
    "Eq",
    "NotEq",
    "Gt",
    "Lt",
    "GtEq",
    "LtEq",
    "Neg",
    "Not",
    "BitNot",
    "ToBool",
    "Jump",
    "JumpIfFalse",
    "JumpIfFalsePeek",
    "JumpIfTruePeek",
    "RepeatSetup",
    "RepeatNext",
    "IterSetup",
    "IterNext",
    "Return",
    "ReturnNull",
    "Import",
    "Dup",
    "Rotate",
    "GetPropKeep",
    "GetIndexKeep",
    "JumpIfNullPeek",
];

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn scratch() -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let dir = base
        .join("em_selfhost_ir_encode_tests")
        .join(std::process::id().to_string());
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn isolated_env() -> [(&'static str, String); 4] {
    let dir = scratch().join("home");
    [
        ("EM_HOME", dir.display().to_string()),
        (
            "EM_PROJECTS_DIR",
            scratch().join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
        // Keep the guarded self-tests in the concatenated source from printing
        // an extra `ok` line before the fixture table.
        ("EM_SELFTEST", "0".to_string()),
    ]
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Escape a Rust string for embedding in an Englishmen string literal.
fn enm_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Englishmen strings interpolate `{...}`, so JSON braces must be
            // escaped even inside a string literal.
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn constant_number(value: &str) -> String {
    format!("{{\"tag\":\"number\",\"value\":{value}}}")
}

fn constant_string(value: &str) -> String {
    format!("{{\"tag\":\"string\",\"value\":{}}}", json_string(value))
}

fn constant_bool(value: bool) -> String {
    format!("{{\"tag\":\"bool\",\"value\":{value}}}")
}

fn constant_null() -> String {
    "{\"tag\":\"null\"}".to_string()
}

fn instruction(op: &str, a: u32, b: u32, line: u32, col: u32) -> String {
    format!(
        "{{\"op\":{},\"a\":{a},\"b\":{b},\"line\":{line},\"col\":{col}}}",
        json_string(op)
    )
}

fn op_field(op: &str) -> String {
    format!("\"op\":{}", json_string(op))
}

fn num_field(name: &str, value: u32) -> String {
    format!("\"{name}\":{value}")
}

fn instruction_fields(fields: &[String]) -> String {
    format!("{{{}}}", fields.join(","))
}

fn chunk(constants: &[String], functions: &[String], code: &[String]) -> String {
    format!(
        "{{\"constants\":[{}],\"functions\":[{}],\"code\":[{}]}}",
        constants.join(","),
        functions.join(","),
        code.join(",")
    )
}

fn program(constants: &[String], functions: &[String], code: &[String]) -> String {
    format!(
        "{{\"version\":2,\"constants\":[{}],\"functions\":[{}],\"code\":[{}]}}",
        constants.join(","),
        functions.join(","),
        code.join(",")
    )
}

fn prototype(name: &str, params: &[&str], body: &str) -> String {
    let params: Vec<String> = params.iter().map(|p| json_string(p)).collect();
    format!(
        "{{\"name\":{},\"params\":[{}],\"chunk\":{body}}}",
        json_string(name),
        params.join(",")
    )
}

fn nested_functions(depth: usize) -> String {
    assert!(depth > 0);
    let mut outer = prototype("deep0", &[], &chunk(&[], &[], &[]));
    for level in 1..depth {
        outer = prototype(&format!("deep{level}"), &[], &chunk(&[], &[outer], &[]));
    }
    outer
}

struct Fixture {
    label: &'static str,
    expression: String,
}

fn json_fixture(label: &'static str, ir: &str) -> Fixture {
    Fixture {
        label,
        expression: format!("jsonDecode({})", enm_string(ir)),
    }
}

fn enm_fixture(label: &'static str, expression: String) -> Fixture {
    Fixture { label, expression }
}

fn fixtures() -> Vec<Fixture> {
    let mut all = Vec::new();

    all.push(json_fixture("empty", &program(&[], &[], &[])));

    all.push(json_fixture(
        "hello loadable",
        &program(
            &[constant_string("hello from pure encoder")],
            &[],
            &[
                instruction_fields(&[op_field("LoadConst"), num_field("a", 0)]),
                instruction_fields(&[op_field("Say"), num_field("a", 1)]),
                instruction_fields(&[op_field("ReturnNull")]),
            ],
        ),
    ));

    let constants = vec![
        constant_number("0.0"),
        constant_number("-0.0"),
        constant_number("1.0"),
        constant_number("-1.0"),
        constant_number("1.5"),
        constant_number("-3.25"),
        constant_number("3.141592653589793"),
        constant_number("9007199254740991.0"),
        constant_number("0.1"),
        constant_number("-0.1"),
        constant_number("1e-300"),
        constant_number("-1e-300"),
        constant_number("1e100"),
        constant_number("-1e100"),
        constant_number(&format!("{:?}", f64::from_bits(1))),
        constant_number(&format!("{:?}", f64::MIN_POSITIVE)),
        constant_number(&format!("{:?}", f64::MAX)),
        constant_number(&format!("{:?}", f64::from_bits(0x000f_ffff_ffff_ffff))),
        constant_string(""),
        constant_string("plain ascii"),
        constant_string("héllo 😀"),
        constant_string("line\nquote\"tab\t\\"),
        constant_bool(true),
        constant_bool(false),
        constant_null(),
    ];
    all.push(json_fixture("constants", &program(&constants, &[], &[])));

    all.push(enm_fixture(
        "non-finite constants",
        format!(
            "{{version: 2, constants: [\
             {{tag: \"number\", value: parseFloat(\"NaN\")}}, \
             {{tag: \"number\", value: parseFloat(\"inf\")}}, \
             {{tag: \"number\", value: -parseFloat(\"inf\")}}], \
             functions: [], code: []}}"
        ),
    ));

    let all_ops: Vec<String> = OPS.iter().map(|op| instruction(op, 2, 3, 4, 5)).collect();
    all.push(json_fixture(
        "all 55 opcode tags",
        &program(&[], &[], &all_ops),
    ));

    all.push(json_fixture(
        "operand shapes and defaults",
        &program(
            &[constant_string("name")],
            &[],
            &[
                instruction("LoadConst", 1, 99, 1, 1),
                instruction("Add", 99, 99, 1, 1),
                instruction("Call", 2, 3, 7, 8),
                instruction("IterNext", 4, 5, 9, 10),
                instruction_fields(&[op_field("Return")]),
                instruction_fields(&[op_field("Jump"), num_field("a", 3)]),
                instruction_fields(&[op_field("MethodCall"), num_field("a", 0)]),
                instruction_fields(&[op_field("Add"), num_field("a", 7), num_field("b", 9)]),
            ],
        ),
    ));

    let inner = chunk(
        &[constant_string("inner"), constant_number("2.5")],
        &[],
        &[instruction("Return", 0, 0, 9, 3)],
    );
    let middle = chunk(
        &[],
        &[prototype("função 😀", &["α", "b"], &inner)],
        &[instruction("GetVar", 0, 0, 1, 1)],
    );
    all.push(json_fixture(
        "recursive and unicode functions",
        &program(
            &[constant_string("name")],
            &[
                prototype("f", &["a"], &middle),
                prototype("", &[], &chunk(&[], &[], &[])),
            ],
            &[
                instruction("MakeClosure", 0, 0, 1, 1),
                instruction("Call", 2, 0, 7, 8),
                instruction("ReturnNull", 0, 0, 1, 1),
            ],
        ),
    ));

    all.push(json_fixture(
        "32 nested functions",
        &program(&[], &[nested_functions(32)], &[]),
    ));

    all.push(json_fixture(
        "wide operands",
        &program(
            &[],
            &[],
            &[
                instruction("Call", u32::MAX, u32::MAX, u32::MAX, 0),
                instruction("Jump", u32::MAX - 1, 0, 0, u32::MAX),
            ],
        ),
    ));

    let many: Vec<String> = (0..200)
        .map(|i| {
            instruction(
                OPS[i % OPS.len()],
                (i * 3) as u32,
                (i * 7) as u32,
                i as u32,
                (i % 13) as u32,
            )
        })
        .collect();
    all.push(json_fixture(
        "200 mixed instructions",
        &program(&[constant_number("1.25")], &[], &many),
    ));

    all
}

fn harness_source(fixtures: &[Fixture]) -> String {
    // The library source keeps native `emb` inside its EM_SELFTEST guard so it
    // can be inlined into the import-free bootstrap image. This native-only
    // comparison harness supplies the oracle import explicitly.
    let mut source = String::from("import emb\n");
    source.push_str(include_str!("../selfhost/ir_encode.enm"));
    source.push_str("\nieFixtures = [\n");
    for (index, fixture) in fixtures.iter().enumerate() {
        let comma = if index + 1 == fixtures.len() { "" } else { "," };
        source.push_str(&format!(
            "    [{}, {}]{}\n",
            enm_string(fixture.label),
            fixture.expression,
            comma
        ));
    }
    source.push_str("]\n");
    source.push_str(
        "for ieFixture in ieFixtures\n\
         \x20   say(ieFixture[0])\n\
         \x20   say(jsonEncode(ieEncode(ieFixture[1])))\n\
         \x20   say(jsonEncode(emb.encode(ieFixture[1])))\n\
         end\n",
    );
    source
}

fn compare_on_backend(fixtures: &[Fixture], extra: &[&str]) {
    let script = scratch().join("compare.enm");
    fs::write(&script, harness_source(fixtures)).unwrap();

    let mut args = vec!["run", script.to_str().unwrap()];
    args.extend_from_slice(extra);
    let output = run(&args);
    assert!(
        output.status.success(),
        "fixture comparison failed on backend {extra:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        fixtures.len() * 3,
        "expected three lines per fixture; stdout was:\n{stdout}"
    );
    for (index, fixture) in fixtures.iter().enumerate() {
        let at = index * 3;
        assert_eq!(lines[at], fixture.label, "fixture order changed");
        assert_eq!(
            lines[at + 1],
            lines[at + 2],
            "ieEncode disagrees with emb.encode for '{}'",
            fixture.label
        );
    }
}

#[test]
fn pure_encoder_matches_native_bytes_on_every_fixture() {
    let fixtures = fixtures();
    assert!(fixtures.len() >= 10, "fixture set unexpectedly small");
    assert_eq!(OPS.len(), 55, "every opcode tag must be covered");
    compare_on_backend(&fixtures, &[]);
    compare_on_backend(&fixtures, &["--interpret"]);
}

fn decode_bytes(json: &str) -> Vec<u8> {
    let inner = json.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .map(|part| part.trim().parse::<u8>().unwrap())
        .collect()
}

#[test]
fn pure_encoder_bytes_load_and_run_on_the_vm() {
    let fixtures = fixtures();
    let script = scratch().join("loadable.enm");
    fs::write(&script, harness_source(&fixtures)).unwrap();
    let output = run(&["run", script.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "fixture comparison failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let hello = fixtures
        .iter()
        .position(|fixture| fixture.label == "hello loadable")
        .expect("hello fixture");
    let bytes = decode_bytes(lines[hello * 3 + 1]);
    assert_eq!(&bytes[..6], b"EMBC\x02\x00");

    let image = scratch().join("pure.emb");
    fs::write(&image, &bytes).unwrap();
    let executed = run(&["run-fixed", image.to_str().unwrap()]);
    assert!(
        executed.status.success(),
        "bytecode::deserialize rejected ieEncode output: {}",
        String::from_utf8_lossy(&executed.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&executed.stdout).trim(),
        "hello from pure encoder"
    );
}

#[test]
fn encoder_script_is_canonically_formatted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(bin())
        .arg("fmt")
        .arg(root.join("selfhost/ir_encode.enm"))
        .arg("--check")
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em fmt --check");
    assert!(
        output.status.success(),
        "ir_encode.enm is not formatted: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
