#!/usr/bin/env python3
"""Regenerate the round-4 no-Rust-bootstrap Englishmen scripts.

Based on the round-3 parent `regen.py`.  In addition to the original sources
this tool inlines every `selfhost/pure_*.enm` file (in dependency order), cuts
all guarded self-tests, rewrites the old `os.env("EM_SELFTEST", ...)` guard to
the global `env` builtin, and strips every non-guarded `import` statement.

The `host` import is deliberately preserved: it is the single primitive bridge
to the C runtime described in `bootstrap-c/ABI.md`, not a native module.  Native
modules (`fs`, `os`, `json`, `emb`, `bytes`, `project`, `math`, `cowsay`,
`string`) must be supplied by pure wrappers in `pure_*.enm` instead.

Usage:
    python3 tools/regen_selfhost.py \
        --root selfhost \
        --driver-from selfhost/full.enm \
        --marker '$- === combined full-self-hosting pipeline ===' \
        --out selfhost/full.enm

    python3 tools/regen_selfhost.py \
        --root selfhost \
        --driver-from selfhost/bootstrap_compile.enm \
        --marker '$- === compile-only bootstrap driver ===' \
        --out selfhost/bootstrap_compile.enm --no-vm-exec

The output is formatted with `em fmt` by default (best effort; important
because some sources contain literal control characters in string literals);
use `--no-format` to skip it. `--check` validates without writing.
"""
from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Iterable

SOURCES = [
    "compiler",
    "codegen_ir",
    "ir_encode",
    "lib_bytes",
    "lib_project",
    "lib_modules",
    "vm_core",
    "vm_runtime",
]

NATIVE_MODULES = frozenset(
    {"fs", "json", "os", "emb", "bytes", "project", "math", "cowsay", "string"}
)

IMPORT_RE = re.compile(r"^(\s*)import\s+([A-Za-z_][A-Za-z0-9_]*)\s*$")
SELFTEST_RE = re.compile(r"\bEM_SELFTEST\b")
REQUIRES_RE = re.compile(r"@(?:requires|depends)[:\s]+([A-Za-z0-9_,\s]+)")
PURE_IMPORT_RE = re.compile(r"^\s*import\s+(pure_[A-Za-z0-9_]+)\s*$")
GUARD_OLD = 'os.env("EM_SELFTEST", "")'
GUARD_NEW = 'env("EM_SELFTEST")'

COMPILER_DRIVER_PREFIXES = ("argv = args()", "argvFull = args()")
SELFTEST_GUARD_PREFIXES = (
    'if os.env("EM_SELFTEST", "") == "1"',
    'if env("EM_SELFTEST") == "1"',
    'if os.env("EM_SELFTEST", "") != "1"',
    'if env("EM_SELFTEST") != "1"',
)


def split_lines(text: str) -> list[str]:
    """Split on LF only.

    Some `.enm` string literals contain literal control characters (e.g. CR in
    `lib_modules.enm`'s JSON escaper).  `str.splitlines()` treats them as line
    breaks and corrupts those literals; Englishmen source lines are LF-based.
    """
    chunks = text.split("\n")
    lines = [chunk + "\n" for chunk in chunks[:-1]]
    if chunks[-1]:
        lines.append(chunks[-1])
    return lines


def _first_cut(lines: list[str], prefixes: Iterable[str]) -> int | None:
    wanted = tuple(prefixes)
    for i, line in enumerate(lines):
        stripped = line.lstrip()
        if not stripped.startswith(wanted):
            continue
        indent = len(line) - len(stripped)
        if indent == 0:
            return i
        # An inverted guard often sits inside a top-level self-test runner
        # function. Cut at the function header so its `end` is removed too.
        for j in range(i - 1, -1, -1):
            outer = lines[j].lstrip()
            if lines[j].startswith(outer) and (
                outer.startswith("function ") or outer.startswith("if ")
            ):
                return j
        return i
    return None


def cut_source(text: str, name: str) -> str:
    """Drop the CLI/driver tail and every guarded self-test.

    `compiler.enm` starts its driver at `argv = args()`; every other source
    keeps only the lines before its `EM_SELFTEST` guard.  Pure sources without
    a guard are kept in full.
    """
    lines = split_lines(text)
    if name == "compiler":
        cut = _first_cut(lines, COMPILER_DRIVER_PREFIXES)
    else:
        cut = _first_cut(lines, SELFTEST_GUARD_PREFIXES)
    return "".join(lines[:cut]) if cut is not None else text


def drop_vm_execution(text: str, name: str) -> str:
    """Remove every VM execution entry point for compile-only bootstrap files."""
    if name == "vm_core":
        marker = "function vmRun(vmRun_ir)"
        if marker in text:
            text = text[: text.index(marker)]
        text = text.replace(
            "   `vmRun(ir)` returns an array containing one entry per `Say` argument. On an\n"
            "   invalid IR or a runtime fault it returns a structured error object instead:\n"
            "   {__vmError: true, kind: \"...\", error: \"...\", op: \"...\", line: n, col: n}.\n"
            "   Use `vmIsError(value)` to distinguish it from a legitimate output object.\n",
            "   Execution entry points are omitted from this compile-only bootstrap;\n"
            "   the Rust host remains the oracle for validating generated bytecode.\n",
        )
    elif name == "vm_runtime":
        cuts = [
            pos
            for marker in ("function vmRunWithImports(", "function vmRunWithRuntime(")
            if (pos := text.find(marker)) != -1
        ]
        if cuts:
            text = text[: min(cuts)]
    return text


def canonicalize_guards(text: str) -> str:
    """Use the global `env` builtin for the self-test guard.

    This keeps `os` out of the guard itself, so a source needs no native
    module import merely to decide whether to run its self-tests.
    """
    return text.replace(GUARD_OLD, GUARD_NEW)


def unwrap_driver_selftest(text: str) -> str:
    """Drop a leftover top-level EM_SELFTEST wrapper from a bootstrap driver.

    Older generated drivers captured a self-test `if ... else ... end` block
    around their real body. The generated image must never execute that guard,
    because the self-hosted VM does not provide the global `env` builtin. Keep
    the `else` body and de-indent it.
    """
    lines = split_lines(text)
    guard = None
    for i, line in enumerate(lines):
        if line.startswith(SELFTEST_GUARD_PREFIXES):
            guard = i
            break
    if guard is None:
        return text
    else_i = None
    end_i = None
    for j in range(guard + 1, len(lines)):
        stripped = lines[j].lstrip()
        if lines[j].startswith(stripped):
            if stripped.startswith("else"):
                else_i = j
            elif stripped.startswith("end"):
                end_i = j
                break
    if else_i is None or end_i is None:
        return text
    body = [line[4:] if line.startswith("    ") else line for line in lines[else_i + 1:end_i]]
    return "".join(lines[:guard] + body + lines[end_i + 1:])


def rewrite_driver_modules(text: str) -> str:
    """Replace native module calls in the bootstrap drivers with globals.

    The drivers were written for the round-3 Rust host, where `fs.read` and
    `bytes.write` were available as native imports. Generated sources must not
    import native modules, so use the global `readFile`/`writeBytes` builtins
    supplied by both the Rust oracle and the C runtime.
    """
    text = text.replace("native bytes.write", "global writeBytes")
    text = text.replace("fs.read(", "readFile(")
    text = text.replace("bytes.write(", "writeBytes(")
    text = text.replace("math.pi", "pmtMathModule().pi")
    text = text.replace("math.e", "pmtMathModule().e")
    return text


def strip_unguarded_imports(text: str, *, keep: frozenset[str] = frozenset({"host"})) -> str:
    """Remove imports outside `EM_SELFTEST` guards.

    Indentation is used only to track guard nesting; Englishmen blocks are
    delimited by `if`/`end`, and `em fmt` canonicalises indentation later.
    """
    out: list[str] = []
    guard_indents: list[int] = []
    for line in split_lines(text):
        stripped = line.lstrip()
        indent = len(line) - len(stripped)
        if stripped.strip():
            while guard_indents and indent <= guard_indents[-1]:
                guard_indents.pop()
        match = IMPORT_RE.match(line)
        if match and not guard_indents and match.group(2) not in keep:
            continue
        out.append(line)
        if SELFTEST_RE.search(stripped) and stripped.lstrip().startswith("if "):
            guard_indents.append(indent)
    return "".join(out)


def pure_dependencies(path: Path, stems: set[str]) -> set[str]:
    text = path.read_text()
    deps: set[str] = set()
    for line in split_lines(text):
        match = REQUIRES_RE.search(line)
        if match:
            for token in re.split(r"[,\s]+", match.group(1)):
                if token in stems:
                    deps.add(token)
        match = PURE_IMPORT_RE.match(line)
        if match and match.group(1) in stems:
            deps.add(match.group(1))
    deps.discard(path.stem)
    return deps


def discover_pure_sources(root: Path) -> list[Path]:
    """Topologically sort `pure_*.enm`, alphabetically for deterministic ties."""
    paths = sorted(root.glob("pure_*.enm"))
    stems = {path.stem for path in paths}
    deps = {path.stem: pure_dependencies(path, stems) for path in paths}
    remaining = set(stems)
    ordered: list[str] = []
    while remaining:
        ready = sorted(name for name in remaining if deps[name] <= set(ordered))
        if not ready:
            raise SystemExit(
                "cyclic pure_*.enm dependencies: "
                + ", ".join(sorted(remaining))
            )
        ordered.append(ready[0])
        remaining.remove(ready[0])
    by_stem = {path.stem: path for path in paths}
    return [by_stem[name] for name in ordered]


def extract_driver(text: str, marker: str) -> str:
    if marker not in text:
        raise SystemExit(f"marker not found: {marker}")
    return text[text.index(marker):]


def format_output(path: Path) -> None:
    """Run `em fmt` when a binary is available; never fail the regen for it."""
    candidates = []
    env_bin = os.environ.get("EM_BIN")
    if env_bin:
        candidates.append(Path(env_bin))
    target = os.environ.get("CARGO_TARGET_DIR")
    if target:
        candidates.append(Path(target) / "debug" / "em")
    candidates.append(Path("target") / "debug" / "em")
    candidates.append(Path(shutil.which("em") or "/nonexistent/em"))
    for binary in candidates:
        if not binary.is_file():
            continue
        result = subprocess.run(
            [str(binary), "fmt", str(path)],
            cwd=Path(__file__).resolve().parent.parent,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            print(result.stdout.strip())
            return
        print(f"warning: {binary} fmt failed: {result.stderr.strip()}", file=sys.stderr)
        return
    print("warning: no em binary found; output left unformatted", file=sys.stderr)


def verify_no_native_imports(text: str) -> None:
    guard_indents: list[int] = []
    for lineno, line in enumerate(split_lines(text), 1):
        stripped = line.lstrip()
        indent = len(line) - len(stripped)
        if stripped.strip():
            while guard_indents and indent <= guard_indents[-1]:
                guard_indents.pop()
        match = IMPORT_RE.match(line)
        if match and not guard_indents and match.group(2) in NATIVE_MODULES:
            raise SystemExit(
                f"unguarded native import at line {lineno}: {stripped}"
            )
        if SELFTEST_RE.search(stripped) and stripped.lstrip().startswith("if "):
            guard_indents.append(indent)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path, help="selfhost directory")
    parser.add_argument("--driver-from", required=True, type=Path)
    parser.add_argument("--marker", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--no-vm-exec", action="store_true")
    parser.add_argument(
        "--no-format",
        action="store_true",
        help="leave literal control characters in .enm string literals untouched",
    )
    parser.add_argument(
        "--format",
        action="store_true",
        help="accepted for compatibility; formatting is the default",
    )
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    driver = extract_driver(args.driver_from.read_text(), args.marker)

    parts: list[str] = []
    for name in SOURCES:
        source = args.root / f"{name}.enm"
        if not source.exists():
            continue
        text = cut_source(source.read_text(), name)
        if args.no_vm_exec:
            text = drop_vm_execution(text, name)
        text = canonicalize_guards(text)
        parts.append(f"$- === inlined selfhost/{name}.enm === -$\n")
        parts.append(strip_unguarded_imports(text))

    for source in discover_pure_sources(args.root):
        text = canonicalize_guards(cut_source(source.read_text(), source.stem))
        parts.append(f"$- === inlined selfhost/{source.name} === -$\n")
        parts.append(strip_unguarded_imports(text))

    driver = unwrap_driver_selftest(canonicalize_guards(driver))
    driver = strip_unguarded_imports(rewrite_driver_modules(driver))
    output = "".join(parts) + driver
    verify_no_native_imports(output)
    if args.check:
        return
    args.out.write_text(output)
    print(f"wrote {args.out}")
    if not args.no_format:
        format_output(args.out)


if __name__ == "__main__":
    main()
