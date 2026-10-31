#!/usr/bin/env python3
"""Compare Node.js and js-rs on identical JavaScript workloads."""

from __future__ import annotations

import argparse
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

RESULT_PATTERN = re.compile(r"^BENCH_RESULT:([^:]+):([^:]+):(\d+)$", re.MULTILINE)
SUITES = ("arithmetic", "function_calls", "objects_arrays", "numeric_regions")

BENCHMARK_SOURCE = r'''
"use strict";
const ITERATIONS = __ITERATIONS__;
const MEASURE_ROUNDS = 3;

function arithmetic(n) {
    let state = 17;
    let sum = 0;
    for (let i = 0; i < n; i++) {
        state = (state * 48271 + 1) % 2147483647;
        sum += state;
    }
    return sum + state;
}

function step(accumulator, i) {
    return (accumulator + (i * 17 % 97) + (accumulator % 31)) % 1000003;
}

function functionCalls(n) {
    let accumulator = 1;
    for (let i = 0; i < n; i++) {
        accumulator = step(accumulator, i);
    }
    return accumulator;
}

function objectsAndArrays(n) {
    const values = [3, 5, 7, 11, 13, 17, 19, 23];
    const state = { checksum: 0 };
    for (let i = 0; i < n; i++) {
        const index = i % values.length;
        values[index] = (values[index] + i + state.checksum) % 10007;
        state.checksum += values[index];
    }
    return state.checksum + values[0] + values[values.length - 1];
}

function* numericRegionGenerator(n) {
    let total = 0;
    for (let i = 0; i < n; i++) {
        total = total + i * 2 - i / 2;
    }
    yield total;
}

function numericRegions(n) {
    return numericRegionGenerator(n).next().value;
}

function measure(name, workload) {
    workload(ITERATIONS);
    let checksum = 0;
    const start = Date.now();
    for (let round = 0; round < MEASURE_ROUNDS; round++) {
        checksum += workload(ITERATIONS);
    }
    const elapsed = Date.now() - start;
    console.log("BENCH_RESULT:" + name + ":" + checksum + ":" + elapsed);
}

measure("arithmetic", arithmetic);
measure("function_calls", functionCalls);
measure("objects_arrays", objectsAndArrays);
measure("numeric_regions", numericRegions);
'''


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def nonnegative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must not be negative")
    return parsed


def resolve_executable(name: str) -> str:
    executable = shutil.which(name)
    if executable is None:
        raise RuntimeError(f"executable not found: {name}")
    return executable


def version_of(executable: str) -> str:
    result = subprocess.run(
        [executable, "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    return (result.stdout or result.stderr).strip().splitlines()[0]


def run_engine(
    executable: str,
    script_path: Path,
    timeout: float,
    extra_args: tuple[str, ...] = (),
) -> tuple[dict[str, tuple[str, int]], float]:
    start = time.perf_counter()
    try:
        result = subprocess.run(
            [executable, *extra_args, str(script_path)],
            check=True,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"{executable} timed out after {timeout:g}s") from error
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() or error.stdout.strip()
        raise RuntimeError(f"{executable} exited with {error.returncode}: {detail}") from error
    wall_ms = (time.perf_counter() - start) * 1000

    results = {
        name: (checksum, int(elapsed))
        for name, checksum, elapsed in RESULT_PATTERN.findall(result.stdout)
    }
    if set(results) != set(SUITES):
        raise RuntimeError(
            f"{executable} did not emit all benchmark results; "
            f"expected {', '.join(SUITES)}. stdout: {result.stdout!r}"
        )
    return results, wall_ms


def median(values: list[float]) -> float:
    return statistics.median(values)


def format_ratio(js_rs_ms: float, node_ms: float) -> str:
    if node_ms == 0:
        return "n/a"
    return f"{js_rs_ms / node_ms:.2f}x"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--node", default=os.environ.get("NODE_BIN", "node"), help="Node.js executable")
    parser.add_argument("--js-rs", default=os.environ.get("JS_RS_BIN", "js-rs"), help="js-rs executable")
    parser.add_argument("--runs", type=positive_int, default=7, help="measured process launches per engine")
    parser.add_argument("--warmup", type=nonnegative_int, default=1, help="discarded process launches per engine")
    parser.add_argument("--iterations", type=positive_int, default=100_000, help="iterations per workload round")
    parser.add_argument("--timeout", type=positive_int, default=120, help="timeout in seconds per process launch")
    parser.add_argument("--compare-jit", action="store_true", help="also measure required JIT against ordinary js-rs")
    args = parser.parse_args()

    try:
        node = resolve_executable(args.node)
        js_rs = resolve_executable(args.js_rs)
        print(f"Node.js: {version_of(node)} ({node})")
        print(f"js-rs:   {version_of(js_rs)} ({js_rs})")
        print(f"Runs: {args.runs}, process warmups: {args.warmup}, iterations: {args.iterations:,}")

        source = BENCHMARK_SOURCE.replace("__ITERATIONS__", str(args.iterations))
        commands = [("node", node, ()), ("js-rs", js_rs, ())]
        if args.compare_jit:
            commands.append(("js-rs-jit", js_rs, ("--jit-required",)))
        wall_times: dict[str, list[float]] = {engine: [] for engine, _, _ in commands}
        kernel_times: dict[str, dict[str, list[float]]] = {
            engine: {suite: [] for suite in SUITES} for engine in wall_times
        }
        checksums: dict[str, dict[str, str]] = {engine: {} for engine in wall_times}

        with tempfile.TemporaryDirectory(prefix="js-engine-bench-") as temp_dir:
            script_path = Path(temp_dir) / "benchmark.js"
            script_path.write_text(source, encoding="utf-8")

            for _ in range(args.warmup):
                for engine, executable, extra_args in commands:
                    results, _ = run_engine(executable, script_path, args.timeout, extra_args)
                    for suite, (checksum, _) in results.items():
                        checksums[engine].setdefault(suite, checksum)

            for run_index in range(args.runs):
                order = commands if run_index % 2 == 0 else tuple(reversed(commands))
                for engine, executable, extra_args in order:
                    results, wall_ms = run_engine(executable, script_path, args.timeout, extra_args)
                    wall_times[engine].append(wall_ms)
                    for suite, (checksum, kernel_ms) in results.items():
                        expected = checksums[engine].setdefault(suite, checksum)
                        if checksum != expected:
                            raise RuntimeError(f"{engine} produced inconsistent {suite} checksums")
                        kernel_times[engine][suite].append(float(kernel_ms))

        for suite in SUITES:
            for engine in wall_times:
                if checksums["node"][suite] != checksums[engine][suite]:
                    raise RuntimeError(
                        f"{suite} checksum mismatch: Node.js={checksums['node'][suite]}, "
                        f"{engine}={checksums[engine][suite]}"
                    )

        print("\nMedian in-script execution time (lower is faster; js-rs/Node > 1 means js-rs is slower)")
        print(f"{'workload':<18} {'Node.js':>12} {'js-rs':>12} {'js-rs/Node':>14}")
        for suite in SUITES:
            node_ms = median(kernel_times["node"][suite])
            js_rs_ms = median(kernel_times["js-rs"][suite])
            print(f"{suite:<18} {node_ms:>9.1f} ms {js_rs_ms:>9.1f} ms {format_ratio(js_rs_ms, node_ms):>14}")

        node_wall = median(wall_times["node"])
        js_rs_wall = median(wall_times["js-rs"])
        print("\nMedian end-to-end process time (startup + parse + execution)")
        print(f"Node.js: {node_wall:.1f} ms")
        print(f"js-rs:   {js_rs_wall:.1f} ms")
        print(f"js-rs/Node: {format_ratio(js_rs_wall, node_wall)}")
        if args.compare_jit:
            print("\nRequired JIT vs ordinary js-rs (ratio > 1 means JIT is slower)")
            print(f"{'workload':<18} {'ordinary':>12} {'JIT':>12} {'JIT/ordinary':>14}")
            for suite in SUITES:
                ordinary_ms = median(kernel_times["js-rs"][suite])
                jit_ms = median(kernel_times["js-rs-jit"][suite])
                print(f"{suite:<18} {ordinary_ms:>9.1f} ms {jit_ms:>9.1f} ms {format_ratio(jit_ms, ordinary_ms):>14}")
            jit_wall = median(wall_times["js-rs-jit"])
            print(f"JIT end-to-end: {jit_wall:.1f} ms")
            print(f"JIT/ordinary end-to-end: {format_ratio(jit_wall, js_rs_wall)}")
        print("\nChecksums matched for all workloads.")
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
