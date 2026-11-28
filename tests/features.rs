//! Cross-backend feature coverage for the shipped language surface.
//!
//! These tests exercise language features rather than CLI plumbing (which is
//! covered in `tests/cli.rs`). Each source snippet is run on the compiler/VM
//! and on the reference interpreter, and both outputs must match exactly.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_features_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str], cwd: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(em());
    command
        .args(args)
        .envs(isolated_env())
        .stdin(Stdio::null());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.output().expect("failed to run em")
}

fn scratch(name: &str) -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "em_features_{name}_{}_{id}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn output_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn status_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run `source` on both backends, requiring identical successful output.
fn agree(name: &str, source: &str) -> String {
    let dir = scratch(name);
    let file = dir.join("program.enm");
    fs::write(&file, source).unwrap();
    let path = file.to_str().unwrap();

    let vm = run(&[path], None);
    let interpreted = run(&[path, "--interpret"], None);
    assert!(
        vm.status.success(),
        "VM failed for {name}: {}",
        status_text(&vm)
    );
    assert!(
        interpreted.status.success(),
        "interpreter failed for {name}: {}",
        status_text(&interpreted)
    );
    assert_eq!(
        vm.stdout, interpreted.stdout,
        "backends disagree for {name}"
    );
    assert!(
        status_text(&vm).is_empty(),
        "VM wrote diagnostics for {name}: {}",
        status_text(&vm)
    );
    assert!(
        status_text(&interpreted).is_empty(),
        "interpreter wrote diagnostics for {name}: {}",
        status_text(&interpreted)
    );
    let stdout = output_text(&vm);
    let _ = fs::remove_dir_all(&dir);
    stdout
}

#[test]
fn global_builtins_work_on_both_backends() {
    let out = agree(
        "globals",
        r#"say(type([1, 2]))
say(toNumber("3.5"))
say(toString(42))
say(toBool(""))
say(isNumber(3))
say(isString("x"))
say(isArray([1]))
say(isObject({a: 1}))
say(isBool(true))
say(isNull(null))
say(isFunction(len))
say(parseInt("ff", 16))
say(parseFloat("2.5e2"))
say(range(4))
say(sum([1, 2, 3]))
say(average([1, 2, 3]))
say(min(3, 1, 2))
say(max(3, 1, 2))
say(abs(-4))
say(clamp(15, 0, 10))
say(sorted([3, 1, 2]))
say(reversed([1, 2, 3]))
say(zip([1, 2], ["a", "b"]))
say(keys({a: 1, b: 2}))
say(values({a: 1, b: 2}))
say(first([1, 2]))
say(last([1, 2]))
say(slice([1, 2, 3], 1, 3))
say(pow(2, 8))
say(sqrt(25))
say(floor(2.7))
say(ceil(2.2))
say(round(2.5))
say(sign(-3))
say(format("Hello, {{0}}! You are {{1}}.", "Alex", 18))
say(trim("  hi  "))
say(upper("hi"))
say(lower("HI"))
say(split("a,b", ","))
say(join(["a", "b"], "-"))
say(replace("a-b", "-", "+"))
say(startsWith("hello", "he"))
say(endsWith("hello", "lo"))
say(repeat("ab", 2))
say(indexOf("héllo", "é"))
say(contains("hello", "ell"))
say(jsonEncode({a: 1}))
say(jsonDecode("[1, 2]"))
assert(2 > 1, "math still works")
say("assertions ok")
"#,
    );
    assert!(out.contains("Hello, Alex! You are 18."));
    assert!(out.contains("[1, 2]"));
    assert!(out.lines().count() >= 40, "{out}");
}

#[test]
fn runtime_methods_work_on_every_receiver_type() {
    let out = agree(
        "methods",
        r#"xs = [1, 2, 2, 3]
xs.add(4)
xs.push(5)
say(xs.length())
say(xs.contains(3))
say(xs.indexOf(2))
say(xs.lastIndexOf(2))
say(xs.count(2))
say(xs.first())
say(xs.last())
say(xs.slice(1, 3))
say(xs.join("-"))
say(xs.concat([9]))
xs.unique()
say(xs)
xs.swap(0, 1)
say(xs)
xs.clear()
say(xs.isEmpty())

nested = [[1, 2], [3]]
nested.flatten()
say(nested)

obj = {a: 1, b: 2}
say(obj.keys())
say(obj.values())
say(obj.has("a"))
say(obj.get("z", 9))
obj.set("c", 3)
say(obj.length())
say(obj.copy().merge({d: 4}))
obj.delete("a")
say(obj.has("a"))

text = "Hello"
say(text.length())
say(text.upper())
say(text.lower())
say(text.startsWith("He"))
say(text.endsWith("lo"))
say(text.substring(1, 3))
say(text.charAt(1))
say(text.slice(1, 4))
say(text.replace("l", "L"))
text = "a-b-a"
say(text.replaceAll("a", "x"))
say(text.split("-"))
say(text.repeat(2))
say("7".toNumber())
say("42".isDigit())
say("abc".isAlpha())
say("héllo".indexOf("é"))
say("  hi  ".trim().upper())

n = 16
say(n.isEven())
say(n.isOdd())
say(n.isInteger())
say(n.isFinite())
say((-3).sign())
say((-3).abs())
say(2.567.toFixed(2))
say(15.clamp(0, 10))
say(255.toHex())
say(5.toBinary())
say(2.7.floor())
say(2.2.ceil())
say(2.5.round())
say(9.sqrt())

flag = true
say(flag.toString())
say(flag.toggle())
say(flag.and(false))
say(flag.or(false))
say(flag.xor(true))
"#,
    );
    assert!(out.contains("a-b-aa-b-a"));
    assert!(out.contains("x-b-x"));
    assert!(out.contains("true"), "{out}");
}

#[test]
fn standard_modules_work_on_both_backends() {
    let out = agree(
        "modules",
        r#"import math
say(math.sqrt(25))
say(math.pow(2, 8))
say(math.gcd(12, 18))
say(math.clamp(15, 0, 10))
say(math.roundTo(2.345, 2))
say(math.pi)

import string
say(string.upper("hi"))
say(string.length("héllo"))
say(string.pad("hi", 5, "*", "right"))
say(string.join(["a", "b"], "-"))
say(string.isDigit("7"))

import json
data = {name: "Alex", scores: [9, 7]}
text = json.encode(data)
say(json.isValid(text))
say(json.get(json.decode(text), "scores.1"))
say(json.pretty({ok: true}).length() > 0)

import collections
say(collections.sorted([3, 1, 2]))
say(collections.range(4))
say(collections.sum([1, 2, 3]))
say(collections.zip([1, 2], ["a", "b"]))
say(collections.chunk(range(5), 2))
say(collections.get({a: 1}, "a", 0))

import random
random.seed(11)
say(random.int(1, 100))
say(random.choice(["a", "b", "c"]))
say(random.uuid().length())

import time
say(time.format("%Y", 0).length())
say(type(time.now()))
"#,
    );
    assert!(out.contains("***hi"));
    assert!(out.contains("[[0, 1], [2, 3], [4]]"));
    assert!(out.contains("36"));
}

#[test]
fn all_eleven_modules_can_be_imported() {
    let out = agree(
        "all_modules",
        r#"import math
import time
import qrc
import cowsay
import string
import json
import fs
import os
import random
import collections
import gfx

say(type(math.pi))
say(type(time.now()))
say(type(qrc.make))
say(type(cowsay.say))
say(type(string.upper))
say(type(json.encode))
say(type(fs.read))
say(os.platform().length() > 0)
say(type(random.float))
say(type(collections.sorted))
canvas = gfx.canvas(2, 3)
say(canvas.width())
say(canvas.height())
say(fs.exists("em_features_definitely_missing_file"))
"#,
    );
    assert!(out.lines().count() == 13, "{out}");
    assert!(out.contains("2") && out.contains("3"), "{out}");
    assert!(out.contains("false"), "{out}");
}

#[test]
fn loops_operators_and_compact_forms_work_on_both_backends() {
    let out = agree(
        "syntax",
        r#"a = 7
b = 2
say(a + b)
say(a - b)
say(a * b)
say(a / b)
say(a % b)
say(a ** b)
say(a // b)
say(a div b)
say(a & b)
say(a | b)
say(a ^ b)
say(~a)
say(a << 1)
say(a >> 1)
say(a == b)
say(a != b)
say(a > b)
say(a < b)
say(a >= b)
say(a <= b)
say(a and b)
say(a or b)
say(not false)
say(a && b)
say(a || b)
say(!false)
say(0xff)
say(0b1010)
say(1.5e3)
say(a > b ? "big" : "small")
say(null ?? "fallback")
a += 1
say(a)
a -= 1
say(a)
a *= 2
say(a)
a /= 2
say(a)
a %= 3
say(a)
a **= 2
say(a)
a //= 2
say(a)
a++
say(a)
a--
say(a)

flag = false
if flag
    say("yes")
else if a == 2
    say("else-if")
else
    say("no")
end

n = 0
while n < 5
    n += 1
    if n == 2
        continue
    end
    if n == 4
        break
    end
end
say(n)

total = 0
repeat 3
    total += 2
end
say(total)

people = ["a", "b"]
for person in people
    say(person)
end
"#,
    );
    assert!(out.contains("49"), "{out}");
    assert!(out.contains("big"), "{out}");
    assert!(out.contains("fallback"), "{out}");
    assert!(out.lines().count() >= 40, "{out}");
}

#[test]
fn keyword_named_builtins_and_members_are_callable() {
    let out = agree(
        "keyword_names",
        r#"say(repeat("ab", 2))
text = "ha"
say(text.repeat(3))
flag = true
say(flag.and(false))
say(flag.or(true))
say(flag.toString())
obj = {}
obj.set("end", 7)
say(obj.end)
"#,
    );
    assert_eq!(out.lines().collect::<Vec<_>>(), vec![
        "abab", "hahaha", "false", "true", "true", "7"
    ]);
}

#[test]
fn formatter_keeps_keyword_named_calls_hugged() {
    let dir = scratch("fmt_keyword_calls");
    let file = dir.join("program.enm");
    fs::write(
        &file,
        "say(repeat(\"ab\", 2))\nrepeat(\"ab\", 2)\ntext = \"ha\"\nsay(text.repeat(2))\nflag = true\nsay(flag.and(false))\nrepeat (2)\n    say(1)\nend\n",
    )
    .unwrap();
    let path = file.to_str().unwrap();

    let formatted = run(&["fmt", path], Some(&dir));
    assert!(formatted.status.success(), "{}", status_text(&formatted));
    let source = fs::read_to_string(&file).unwrap();
    assert!(source.contains("repeat(\"ab\", 2)"), "{source}");
    assert!(source.contains("text.repeat(2)"), "{source}");
    assert!(source.contains("flag.and(false)"), "{source}");
    assert!(!source.contains("repeat (\"ab\""), "{source}");
    assert!(source.contains("repeat (2)"), "{source}");

    let check = run(&["fmt", path, "--check"], Some(&dir));
    assert!(check.status.success(), "{}", status_text(&check));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bundled_feature_examples_agree_across_backends() {
    let expected = [
        (
            "strings",
            "Hello, Alex!\nALEX\n1\nhahaha\n***hi\none | two | three\nx-b-x\ntrimmed\n[\"line1\", \"line2\"]\n",
        ),
        (
            "arrays",
            "[9, 4, 1, 2, 3, 4, 5, 7]\n8\n6\n2\n[1, 2, 3]\n4-1-2-3-4-5-7\n[1, 2, 2, 3]\n[3, 2, 1]\n[[1, \"a\"], [2, \"b\"]]\n[1, 2, 3, 4]\n",
        ),
        (
            "objects-api",
            "Alex\n[\"age\", \"city\", \"name\", \"tags\"]\n[18, \"London\", \"Alex\", [\"dev\", \"em\"]]\ntrue\nunknown\n4\nfalse\nage: 18\ncity: London\nname: Alex\ntags: [\"dev\", \"em\"]\n10\nfalse\n",
        ),
        (
            "files",
            "true\n[\"first line\", \"second line\"]\n23\n23\ntrue\nfalse\ntrue\ntrue\ntrue\n",
        ),
        (
            "json",
            "{\"active\":true,\"name\":\"Alex\",\"scores\":[9,7,10]}\ntrue\nfalse\nAlex\n7\n[9, 8, 10]\n{\n  \"items\": [\n    1,\n    2\n  ],\n  \"ok\": true\n}\n",
        ),
    ];
    for (name, expected_output) in expected {
        let dir = scratch(&format!("example_{name}"));
        let file = dir.join(format!("{name}.enm"));
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(format!("{name}.enm"));
        fs::copy(&source, &file).unwrap();

        let vm = run(&[file.to_str().unwrap()], Some(&dir));
        let interpreted = run(&[file.to_str().unwrap(), "--interpret"], Some(&dir));
        assert!(
            vm.status.success(),
            "VM failed for {name}: {}",
            status_text(&vm)
        );
        assert!(
            interpreted.status.success(),
            "interpreter failed for {name}: {}",
            status_text(&interpreted)
        );
        assert_eq!(vm.stdout, interpreted.stdout, "examples disagree for {name}");
        assert_eq!(
            output_text(&vm),
            expected_output,
            "example {name} produced unexpected output"
        );
        assert!(
            status_text(&vm).is_empty(),
            "VM wrote diagnostics for {name}: {}",
            status_text(&vm)
        );
        assert!(
            status_text(&interpreted).is_empty(),
            "interpreter wrote diagnostics for {name}: {}",
            status_text(&interpreted)
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

fn registered_builtin_count() -> usize {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/builtins.rs");
    let source = fs::read_to_string(path).unwrap();
    let block = source
        .split("const EXPECTED_BUILTINS: &[&str] = &[")
        .nth(1)
        .expect("builtin catalogue not found")
        .split("];")
        .next()
        .unwrap();
    block
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.starts_with('"') && line.ends_with("\",")
        })
        .count()
}

#[test]
fn doc_reference_lists_the_current_catalogue() {
    let output = run(&["doc"], None);
    assert!(output.status.success(), "{}", status_text(&output));
    let stdout = output_text(&output);
    for name in [
        "math", "time", "qrc", "cowsay", "string", "json", "fs", "os", "random",
        "collections", "gfx",
    ] {
        assert!(stdout.contains(name), "em doc is missing module '{name}'");
    }
    let count = registered_builtin_count();
    for text in [
        "while / break / continue",
        &format!("{count} global builtins"),
        "**",
        "//",
        "??",
    ] {
        assert!(stdout.contains(text), "em doc is missing '{text}'");
    }
    let guide = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guides/10-builtins.md"),
    )
    .unwrap();
    assert!(
        guide.contains(&format!("all {count} registered functions")),
        "builtins guide has a stale catalogue count"
    );
}
