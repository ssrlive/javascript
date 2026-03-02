#!/usr/bin/env python3
"""
test262 runner for custom JavaScript engines.

Usage:
    # Run all Array.isArray tests
    python3 run.py "test/built-ins/Array/isArray/**/*.js"

    # Run a single test
    python3 run.py test/built-ins/Array/isArray/15.4.3.2-0-1.js

    # Run with a specific engine path
    python3 run.py --engine /path/to/my-engine "test/built-ins/Array/**/*.js"

    # Run with more workers for speed
    python3 run.py -j 8 "test/language/expressions/**/*.js"

    # Show only failures
    python3 run.py --failures-only "test/built-ins/Array/**/*.js"

    # Skip features your engine doesn't support yet
    python3 run.py --skip-features Atomics,SharedArrayBuffer "test/**/*.js"
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import yaml
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Optional


# ─── Constants ────────────────────────────────────────────────────────────────

HARNESS_DIR = Path(__file__).parent / "harness"
DEFAULT_TIMEOUT = 10  # seconds per test


# ─── Metadata parsing ────────────────────────────────────────────────────────

FRONTMATTER_RE = re.compile(r"/\*---(.*?)---\*/", re.DOTALL)


class Phase(Enum):
    PARSE = "parse"
    RESOLUTION = "resolution"
    RUNTIME = "runtime"


@dataclass
class Negative:
    phase: Phase
    type: str


@dataclass
class TestMeta:
    description: str = ""
    includes: list = field(default_factory=list)
    flags: list = field(default_factory=list)
    negative: Optional[Negative] = None
    features: list = field(default_factory=list)
    locale: list = field(default_factory=list)
    raw: bool = False
    strict_only: bool = False
    no_strict: bool = False
    module: bool = False
    async_test: bool = False


def parse_frontmatter(source: str) -> TestMeta:
    """Parse the /*--- ... ---*/ YAML frontmatter from a test file."""
    m = FRONTMATTER_RE.search(source)
    if not m:
        return TestMeta()

    try:
        data = yaml.safe_load(m.group(1))
    except yaml.YAMLError:
        return TestMeta()

    if not isinstance(data, dict):
        return TestMeta()

    meta = TestMeta()
    meta.description = data.get("description", "")
    meta.includes = data.get("includes", [])
    meta.features = data.get("features", [])
    meta.locale = data.get("locale", [])

    flags = data.get("flags", [])
    meta.flags = flags
    meta.raw = "raw" in flags
    meta.strict_only = "onlyStrict" in flags
    meta.no_strict = "noStrict" in flags
    meta.module = "module" in flags
    meta.async_test = "async" in flags

    neg = data.get("negative")
    if neg and isinstance(neg, dict):
        meta.negative = Negative(
            phase=Phase(neg.get("phase", "runtime")),
            type=neg.get("type", ""),
        )

    return meta


# ─── Test assembly ────────────────────────────────────────────────────────────

def load_harness(name: str) -> str:
    """Load a harness file by name."""
    path = HARNESS_DIR / name
    if not path.exists():
        raise FileNotFoundError(f"Harness file not found: {path}")
    return path.read_text(encoding="utf-8")


def assemble_test(source: str, meta: TestMeta, strict: bool = False) -> str:
    """
    Assemble the final script to execute:
      1. (Optional) "use strict";
      2. sta.js + assert.js  (unless raw)
      3. print polyfill      (for async tests)
      4. doneprintHandle.js  (if async)
      5. extra includes
      6. the test source
    """
    parts = []

    if strict:
        parts.append('"use strict";\n')

    if not meta.raw:
        # Always include sta.js and assert.js (per spec)
        parts.append(load_harness("sta.js"))
        parts.append(load_harness("assert.js"))

        # The engine doesn't have a global print(), define it
        parts.append("if (typeof print === 'undefined') { var print = function(s) { console.log(s); }; }\n")

        # Async tests need doneprintHandle.js
        if meta.async_test:
            parts.append(load_harness("doneprintHandle.js"))

        # Additional includes
        for inc in meta.includes:
            parts.append(load_harness(inc))

    parts.append(source)
    return "\n".join(parts)


# ─── Test execution ──────────────────────────────────────────────────────────

class Result(Enum):
    PASS = "PASS"
    FAIL = "FAIL"
    SKIP = "SKIP"
    TIMEOUT = "TIMEOUT"


@dataclass
class TestResult:
    path: str
    result: Result
    mode: str = ""      # "strict" / "non-strict" / "module" / ""
    message: str = ""
    duration: float = 0.0


def run_one(engine: str, script: str, meta: TestMeta, timeout: int,
            test_dir: str) -> tuple:
    """
    Run a single assembled script.
    Returns (exit_code, stdout, stderr).
    """
    # Write to a temp file in the test's directory (for module imports)
    fd, tmp_path = tempfile.mkstemp(suffix=".js", dir=test_dir)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(script)

        cmd = [engine]
        if meta.module:
            cmd.append("--module")
        cmd.append(tmp_path)

        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=test_dir,
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return -1, "", "TIMEOUT"
    finally:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass


def check_negative(meta: TestMeta, exit_code: int, stdout: str,
                   stderr: str) -> tuple:
    """Check if a negative test produced the expected error."""
    neg = meta.negative

    if exit_code == 0:
        return Result.FAIL, f"Expected {neg.type} but test passed without error"

    # Check error type in stderr
    combined = stderr + stdout
    if neg.type in combined:
        return Result.PASS, ""

    return Result.FAIL, (
        f"Expected {neg.type} ({neg.phase.value} phase), "
        f"got: {combined.strip()[:200]}"
    )


def check_async(stdout: str, stderr: str, exit_code: int) -> tuple:
    """Check async test result."""
    if "Test262:AsyncTestComplete" in stdout:
        return Result.PASS, ""
    if "Test262:AsyncTestFailure:" in stdout:
        msg = stdout[stdout.index("Test262:AsyncTestFailure:"):].strip()
        return Result.FAIL, msg
    if exit_code != 0:
        return Result.FAIL, f"Async test error: {stderr.strip()[:200]}"
    return Result.FAIL, "Async test did not call $DONE / print"


def execute_test(engine: str, test_path: str, timeout: int,
                 skip_features: set) -> list:
    """
    Execute a single test262 test file. Returns a list of TestResult
    (may be 1 or 2 depending on strict/non-strict modes).
    """
    path = Path(test_path)

    # Skip fixture files
    if "_FIXTURE" in path.name:
        return [TestResult(test_path, Result.SKIP, message="Fixture file")]

    source = path.read_text(encoding="utf-8")
    meta = parse_frontmatter(source)

    # Skip if features not supported
    if skip_features and set(meta.features) & skip_features:
        skipped = set(meta.features) & skip_features
        return [TestResult(test_path, Result.SKIP,
                           message=f"Skipped features: {skipped}")]

    test_dir = str(path.parent)
    results = []

    # Determine which modes to run
    if meta.raw:
        modes = [("raw", False)]
    elif meta.module:
        modes = [("module", False)]
    elif meta.strict_only:
        modes = [("strict", True)]
    elif meta.no_strict:
        modes = [("non-strict", False)]
    else:
        modes = [("non-strict", False), ("strict", True)]

    for mode_name, strict in modes:
        t0 = time.monotonic()

        try:
            script = assemble_test(source, meta, strict=strict)
        except FileNotFoundError as e:
            results.append(TestResult(test_path, Result.SKIP, mode_name,
                                       str(e)))
            continue

        exit_code, stdout, stderr = run_one(engine, script, meta, timeout,
                                            test_dir)
        duration = time.monotonic() - t0

        if exit_code == -1:  # timeout
            results.append(TestResult(test_path, Result.TIMEOUT, mode_name,
                                       "Timed out", duration))
            continue

        # Evaluate result
        if meta.negative:
            result, msg = check_negative(meta, exit_code, stdout, stderr)
        elif meta.async_test:
            result, msg = check_async(stdout, stderr, exit_code)
        else:
            if exit_code == 0:
                result, msg = Result.PASS, ""
            else:
                result = Result.FAIL
                msg = (stderr.strip() or stdout.strip())[:300]

        results.append(TestResult(test_path, result, mode_name, msg, duration))

    return results


# ─── Worker function (for multiprocessing) ────────────────────────────────────

def _worker(args):
    engine, test_path, timeout, skip_features = args
    try:
        return execute_test(engine, test_path, timeout, skip_features)
    except Exception as e:
        return [TestResult(test_path, Result.FAIL, message=f"Runner error: {e}")]


# ─── Collect test files ──────────────────────────────────────────────────────

def collect_tests(patterns: list, base_dir: str,
                  exclude_patterns: list = None) -> list:
    """Resolve glob patterns to test file paths, optionally excluding some."""
    files = []
    for pattern in patterns:
        # Make pattern relative to base_dir
        full_pattern = os.path.join(base_dir, pattern)
        matched = sorted(glob.glob(full_pattern, recursive=True))
        files.extend(f for f in matched if f.endswith(".js"))

    if exclude_patterns:
        excluded = set()
        for ep in exclude_patterns:
            full_ep = os.path.join(base_dir, ep)
            excluded.update(glob.glob(full_ep, recursive=True))
        files = [f for f in files if f not in excluded]

    return files


# ─── Colored output ──────────────────────────────────────────────────────────

def color(text, code):
    if sys.stdout.isatty():
        return f"\033[{code}m{text}\033[0m"
    return text


def green(t): return color(t, 32)
def red(t): return color(t, 31)
def yellow(t): return color(t, 33)
def cyan(t): return color(t, 36)
def bold(t): return color(t, 1)


# ─── Main ────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="test262 runner for custom JS engines",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "patterns", nargs="+",
        help='Glob patterns for test files, e.g. "test/built-ins/Array/**/*.js"',
    )
    parser.add_argument(
        "--engine", "-e", default="js",
        help="Path to your JS engine executable (default: js)",
    )
    parser.add_argument(
        "--timeout", "-t", type=int, default=DEFAULT_TIMEOUT,
        help=f"Timeout per test in seconds (default: {DEFAULT_TIMEOUT})",
    )
    parser.add_argument(
        "--jobs", "-j", type=int, default=4,
        help="Number of parallel workers (default: 4)",
    )
    parser.add_argument(
        "--failures-only", "-f", action="store_true",
        help="Only show failing tests",
    )
    parser.add_argument(
        "--skip-features", default="",
        help="Comma-separated list of features to skip",
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true",
        help="Show each test result as it completes",
    )
    parser.add_argument(
        "--json", dest="json_file", default="",
        help="Write results to a JSON file (for CI reporting)",
    )
    parser.add_argument(
        "--gh-annotations", action="store_true",
        help="Emit GitHub Actions annotations (::error) for failures",
    )
    parser.add_argument(
        "--summary", dest="summary_file", default="",
        help="Write a Markdown summary file (for GitHub Actions job summary)",
    )
    parser.add_argument(
        "--exclude", default="",
        help="Comma-separated glob patterns to exclude from collected tests",
    )

    args = parser.parse_args()
    base_dir = str(Path(__file__).parent)
    skip_features = set(f.strip() for f in args.skip_features.split(",") if f.strip())
    exclude_patterns = [p.strip() for p in args.exclude.split(",") if p.strip()] if args.exclude else []

    # Also honour the EXCLUDE_PATTERN env-var (avoids shell glob-expansion
    # that destroys '**' patterns when $EXCLUDE_ARG is unquoted).
    exclude_env = os.environ.get("EXCLUDE_PATTERN", "")
    if exclude_env:
        exclude_patterns.extend(p.strip() for p in exclude_env.split(",") if p.strip())

    # Collect tests
    test_files = collect_tests(args.patterns, base_dir, exclude_patterns)
    if not test_files:
        print(red("No test files found matching the given patterns."))
        sys.exit(1)

    print(bold(f"Running {len(test_files)} test files with {args.jobs} workers..."))
    print(f"Engine: {args.engine}")
    if skip_features:
        print(f"Skipping features: {skip_features}")
    print()

    # Execute
    counts = {Result.PASS: 0, Result.FAIL: 0, Result.SKIP: 0, Result.TIMEOUT: 0}
    failures = []
    total_results = 0
    t_start = time.monotonic()

    work_items = [
        (args.engine, tf, args.timeout, skip_features)
        for tf in test_files
    ]

    with ProcessPoolExecutor(max_workers=args.jobs) as pool:
        futures = {pool.submit(_worker, item): item[1] for item in work_items}
        done_count = 0

        for future in as_completed(futures):
            done_count += 1
            results = future.result()

            for r in results:
                total_results += 1
                counts[r.result] += 1

                if r.result == Result.FAIL or r.result == Result.TIMEOUT:
                    failures.append(r)

                # Determine whether to show this result inline:
                # - failures and timeouts are always shown
                # - passes and skips are shown unless --failures-only
                show = (r.result in (Result.FAIL, Result.TIMEOUT)
                        or args.verbose
                        or not args.failures_only)

                if show:
                    if r.result == Result.PASS:
                        icon = green("✓")
                        rel = os.path.relpath(r.path, base_dir)
                        mode = f" ({r.mode})" if r.mode else ""
                        print(f"  {icon} {rel}{mode}")
                    elif r.result == Result.SKIP:
                        icon = yellow("⊘")
                        rel = os.path.relpath(r.path, base_dir)
                        print(f"  {icon} {rel} — {r.message}")
                    elif r.result == Result.TIMEOUT:
                        icon = yellow("⏱")
                        rel = os.path.relpath(r.path, base_dir)
                        mode = f" ({r.mode})" if r.mode else ""
                        print(f"  {icon} {rel}{mode} — TIMEOUT")
                    else:
                        icon = red("✗")
                        rel = os.path.relpath(r.path, base_dir)
                        mode = f" ({r.mode})" if r.mode else ""
                        print(f"  {icon} {rel}{mode}")
                        if r.message:
                            # Indent error message
                            for line in r.message.split("\n")[:5]:
                                print(f"      {line}")

            # Progress indicator
            if not args.verbose and done_count % 100 == 0:
                elapsed = time.monotonic() - t_start
                rate = done_count / elapsed if elapsed > 0 else 0
                print(f"\r  [{done_count}/{len(test_files)} files, "
                      f"{rate:.0f} files/s] ", end="", flush=True)

    elapsed = time.monotonic() - t_start

    # Summary
    print()
    print(bold("═" * 60))
    print(bold("  Results Summary"))
    print(bold("═" * 60))
    print(f"  {green('PASS')}: {counts[Result.PASS]}")
    print(f"  {red('FAIL')}: {counts[Result.FAIL]}")
    print(f"  {yellow('SKIP')}: {counts[Result.SKIP]}")
    print(f"  {yellow('TIMEOUT')}: {counts[Result.TIMEOUT]}")
    print(f"  Total: {total_results} results from {len(test_files)} files")
    print(f"  Time:  {elapsed:.1f}s")

    if counts[Result.PASS] + counts[Result.SKIP] > 0:
        pass_rate = counts[Result.PASS] / (total_results - counts[Result.SKIP]) * 100 if (total_results - counts[Result.SKIP]) > 0 else 0
    else:
        pass_rate = 0.0
    print(f"  Pass rate: {pass_rate:.1f}%")
    print(bold("═" * 60))

    # Print failure summary
    if failures:
        print()
        print(bold(red(f"  {len(failures)} failure(s):")))
        for r in failures[:50]:
            rel = os.path.relpath(r.path, base_dir)
            mode = f" ({r.mode})" if r.mode else ""
            print(f"    {red('✗')} {rel}{mode}")
            if r.message:
                print(f"        {r.message[:150]}")
        if len(failures) > 50:
            print(f"    ... and {len(failures) - 50} more")

    # ── GitHub Actions annotations ────────────────────────────────────────
    if args.gh_annotations and failures:
        for r in failures:
            rel = os.path.relpath(r.path, base_dir)
            mode = f" ({r.mode})" if r.mode else ""
            msg = (r.message or "test failed").replace('\n', ' ')[:200]
            print(f"::error file={rel},title=test262 {r.result.value}{mode}::{msg}")

    # ── JSON report ───────────────────────────────────────────────────────
    if args.json_file:
        all_results = []
        # Re-collect — we need all results, rebuild from failures + passes
        # Actually, let's collect during execution. We stored failures;
        # rebuild a full report from the pool results.
        # For simplicity, just output the summary + failures.
        report = {
            "summary": {
                "pass": counts[Result.PASS],
                "fail": counts[Result.FAIL],
                "skip": counts[Result.SKIP],
                "timeout": counts[Result.TIMEOUT],
                "total": total_results,
                "files": len(test_files),
                "duration_s": round(elapsed, 1),
                "pass_rate": round(pass_rate, 1) if (total_results - counts[Result.SKIP]) > 0 else 0,
            },
            "failures": [
                {
                    "path": os.path.relpath(r.path, base_dir),
                    "mode": r.mode,
                    "result": r.result.value,
                    "message": r.message[:500],
                }
                for r in failures
            ],
        }
        with open(args.json_file, "w", encoding="utf-8") as jf:
            json.dump(report, jf, indent=2, ensure_ascii=False)
        print(f"\n  JSON report written to {args.json_file}")

    # ── Markdown summary (for $GITHUB_STEP_SUMMARY) ──────────────────────
    if args.summary_file:
        with open(args.summary_file, "w", encoding="utf-8") as mf:
            mf.write("## test262 Results\n\n")
            mf.write("| Metric | Count |\n|--------|-------|\n")
            mf.write(f"| :white_check_mark: Pass | {counts[Result.PASS]} |\n")
            mf.write(f"| :x: Fail | {counts[Result.FAIL]} |\n")
            mf.write(f"| :fast_forward: Skip | {counts[Result.SKIP]} |\n")
            mf.write(f"| :hourglass: Timeout | {counts[Result.TIMEOUT]} |\n")
            mf.write(f"| **Total** | **{total_results}** |\n")
            pr = round(pass_rate, 1) if (total_results - counts[Result.SKIP]) > 0 else 0
            mf.write(f"\n**Pass rate: {pr}%** | Duration: {elapsed:.1f}s\n")
            if failures:
                mf.write(f"\n<details><summary>:x: {len(failures)} failure(s)</summary>\n\n")
                for r in failures[:100]:
                    rel = os.path.relpath(r.path, base_dir)
                    mode = f" ({r.mode})" if r.mode else ""
                    msg = (r.message or "").replace('\n', ' ')[:120]
                    mf.write(f"- `{rel}`{mode}: {msg}\n")
                if len(failures) > 100:
                    mf.write(f"- ... and {len(failures) - 100} more\n")
                mf.write("\n</details>\n")
        print(f"  Markdown summary written to {args.summary_file}")

    sys.exit(1 if counts[Result.FAIL] > 0 else 0)


if __name__ == "__main__":
    main()
