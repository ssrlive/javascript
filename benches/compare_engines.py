#!/usr/bin/env python3
"""Compare Node.js and js-rs on identical JavaScript workloads."""

from __future__ import annotations

import argparse
import math
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
WORKLOADS = {
    "arithmetic": ("arithmetic", "numeric arithmetic and remainder"),
    "function_calls": ("functionCalls", "inline-eligible numeric calls"),
    "objects_arrays": ("objectsAndArrays", "non-escaping scalar objects and arrays"),
    "numeric_regions": ("numericRegions", "numeric loop inside a generator"),
    "closures": ("closures", "calls with mutable captured state"),
    "method_calls": ("methodCalls", "receiver-sensitive object method calls"),
    "property_access": ("propertyAccess", "own, inherited and accessor properties"),
    "property_own": ("propertyOwn", "own data property reads and writes"),
    "property_inherited": ("propertyInherited", "inherited data property reads"),
    "property_accessor": ("propertyAccessor", "accessor getter invocation"),
    "array_methods": ("arrayMethods", "push/pop and map/reduce callbacks"),
    "array_push_pop": ("arrayPushPop", "array push/pop without callback methods"),
    "array_map": ("arrayMap", "Array.map callback and result handling"),
    "array_reduce": ("arrayReduce", "Array.reduce callback and accumulator handling"),
    "array_callback_control": ("arrayCallbackControl", "direct callback calls matching array callback counts"),
    "typed_arrays": ("typedArrays", "Uint32Array indexed loads and stores"),
    "strings": ("strings", "concatenation, slicing and character access"),
    "regexp": ("regexp", "RegExp execution and capture groups"),
    "map_set": ("mapSet", "Map/Set insertion, lookup and deletion"),
    "json": ("jsonRoundtrip", "JSON stringify/parse; ceil(iterations/10) roundtrips"),
    "bigint": ("bigintArithmetic", "bounded BigInt arithmetic; ceil(iterations/10) steps"),
    "exceptions": ("exceptions", "throw/optional catch/finally; one throw per 16 steps"),
    "generators": ("generators", "generator resume and for-of iteration"),
}
SUITES = tuple(WORKLOADS)

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

function closures(n) {
    let state = 1;
    const update = function(value) {
        state = (state + value) % 100003;
        return state;
    };
    let checksum = 0;
    for (let i = 0; i < n; i++) checksum += update(i % 97);
    return checksum + state;
}

function methodCalls(n) {
    const counter = {
        value: 1,
        update(delta) { this.value = (this.value + delta) % 100003; return this.value; }
    };
    let checksum = 0;
    for (let i = 0; i < n; i++) checksum += counter.update(i % 97);
    return checksum + counter.value;
}

function propertyAccess(n) {
    const prototype = { bias: 7 };
    const state = Object.create(prototype);
    state.value = 1;
    Object.defineProperty(state, "doubled", { get() { return this.value * 2; } });
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        state.value = i % 101;
        checksum += state.value + state.bias + state.doubled;
    }
    return checksum;
}

function propertyOwn(n) {
    const state = { value: 1 };
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        state.value = i % 101;
        checksum += state.value;
    }
    return checksum;
}

function propertyInherited(n) {
    const prototype = { bias: 7 };
    const state = Object.create(prototype);
    state.value = 1;
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        state.value = i % 101;
        checksum += state.bias;
    }
    return checksum;
}

function propertyAccessor(n) {
    const state = { value: 1 };
    Object.defineProperty(state, "doubled", { get() { return this.value * 2; } });
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        state.value = i % 101;
        checksum += state.doubled;
    }
    return checksum;
}

function arrayMethods(n) {
    const values = [];
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        values.push(i % 101);
        if (values.length === 16) {
            checksum += values.map(value => value * 3 + 1).reduce((sum, value) => sum + value, 0);
            while (values.length) checksum += values.pop();
        }
    }
    return checksum + values.reduce((sum, value) => sum + value, 0);
}

function arrayPushPop(n) {
    const values = [];
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        values.push(i % 101);
        checksum += values.pop();
    }
    return checksum + values.length;
}

function arrayMap(n) {
    const values = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let checksum = 0;
    for (let i = 0; i < n; i += 16) {
        const mapped = values.map(value => value * 3 + 1);
        checksum += mapped[0];
    }
    return checksum;
}

function arrayReduce(n) {
    const values = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let checksum = 0;
    for (let i = 0; i < n; i += 16) {
        checksum += values.reduce((sum, value) => sum + value, 0);
    }
    return checksum;
}

function arrayCallbackControl(n) {
    const callback = value => value * 3 + 1;
    let checksum = 0;
    for (let i = 0; i < n; i += 16) {
        for (let value = 0; value < 16; value++) checksum += callback(value);
    }
    return checksum;
}

function typedArrays(n) {
    const values = new Uint32Array(256);
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        const index = i & 255;
        values[index] = (values[index] + i * 17 + 3) >>> 0;
        checksum += values[index];
    }
    return checksum + values[0];
}

function strings(n) {
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        const text = "item-" + (i % 1000) + "-value";
        const part = text.slice(5, -6);
        checksum += part.length + part.charCodeAt(0) + text.indexOf("value");
    }
    return checksum;
}

function regexp(n) {
    const pattern = /^item-(\d+)-([a-z]+)$/;
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        const match = pattern.exec("item-" + (i % 1000) + "-value");
        checksum += Number(match[1]) + match[2].length + match[0].length;
    }
    return checksum;
}

function mapSet(n) {
    const map = new Map();
    const set = new Set();
    let checksum = 0;
    for (let i = 0; i < n; i++) {
        const key = i % 256;
        map.set(key, i % 100003);
        set.add(key);
        checksum += map.get(key) + (set.has(key) ? 1 : 0);
        if ((i & 7) === 0) {
            checksum += map.delete(key) ? 3 : 0;
            checksum += set.delete(key) ? 5 : 0;
        }
    }
    return checksum + map.size + set.size;
}

function jsonRoundtrip(n) {
    let checksum = 0;
    for (let i = 0; i < n; i += 10) {
        const text = JSON.stringify({ id: i % 1000, label: "entry", values: [i % 97, true, null] });
        const value = JSON.parse(text);
        checksum += value.id + value.label.length + value.values[0] + text.length;
    }
    return checksum;
}

function bigintArithmetic(n) {
    let state = 17n;
    const modulus = 1000000007n;
    for (let i = 0; i < n; i += 10) state = (state * 48271n + BigInt(i % 97)) % modulus;
    return Number(state);
}

function exceptions(n) {
    let checksum = 0;
    let cleanups = 0;
    for (let i = 0; i < n; i++) {
        try {
            if ((i & 15) === 0) throw i % 97;
            checksum += i % 31;
        } catch {
            checksum += 7;
        } finally {
            cleanups++;
        }
    }
    return checksum + cleanups;
}

function* sequence(n) {
    for (let i = 0; i < n; i++) yield (i * 17) % 97;
}

function generators(n) {
    let checksum = 0;
    for (const value of sequence(n)) checksum += value;
    return checksum;
}

function measure(name, workload) {
    const warmupStart = Date.now();
    workload(ITERATIONS);
    const warmupElapsed = Date.now() - warmupStart;
    let checksum = 0;
    const start = Date.now();
    for (let round = 0; round < MEASURE_ROUNDS; round++) {
        checksum += workload(ITERATIONS);
    }
    const elapsed = Date.now() - start;
    console.log("BENCH_RESULT:" + name + ":" + checksum + ":" + elapsed);
    console.log("BENCH_WARMUP:" + name + ":" + warmupElapsed);
}

__MEASURE_CALLS__
'''


def benchmark_source(iterations: int, suites: tuple[str, ...]) -> str:
    calls = "\n".join(f'measure("{name}", {WORKLOADS[name][0]});' for name in suites)
    return BENCHMARK_SOURCE.replace("__ITERATIONS__", str(iterations)).replace("__MEASURE_CALLS__", calls)


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
    suites: tuple[str, ...] = SUITES,
) -> tuple[dict[str, tuple[str, int]], float, dict[str, int]]:
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

    result_rows = RESULT_PATTERN.findall(result.stdout)
    results = {
        name: (checksum, int(elapsed))
        for name, checksum, elapsed in result_rows
    }
    if len(result_rows) != len(suites) or set(results) != set(suites):
        raise RuntimeError(
            f"{executable} did not emit all benchmark results; "
            f"expected {', '.join(suites)}. stdout: {result.stdout!r}"
        )
    for name, (checksum, _) in results.items():
        try:
            finite = math.isfinite(float(checksum))
        except ValueError as error:
            raise RuntimeError(f"{executable} emitted an invalid {name} checksum: {checksum}") from error
        if not finite:
            raise RuntimeError(f"{executable} emitted a non-finite {name} checksum: {checksum}")
    warmup_rows = re.findall(r"^BENCH_WARMUP:([^:]+):(\d+)$", result.stdout, re.MULTILINE)
    warmups = {
        name: int(elapsed)
        for name, elapsed in warmup_rows
    }
    if len(warmup_rows) != len(suites) or set(warmups) != set(suites):
        raise RuntimeError(f"{executable} did not emit all first-invocation timings. stdout: {result.stdout!r}")
    return results, wall_ms, warmups


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
    parser.add_argument("--report-warmup", action="store_true", help="also report first in-process workload invocations")
    parser.add_argument("--suite", action="append", help="select workloads (repeatable, comma-separated; default: all)")
    parser.add_argument("--list-suites", action="store_true", help="list workloads and exit without launching engines")
    args = parser.parse_args()
    if args.list_suites:
        for name, (_, description) in WORKLOADS.items():
            print(f"{name:<18} {description}")
        return 0
    suites = tuple(dict.fromkeys(name.strip() for group in args.suite for name in group.split(","))) if args.suite else SUITES
    unknown = [name for name in suites if name not in WORKLOADS]
    if unknown:
        parser.error(f"unknown workload(s): {', '.join(unknown)}; use --list-suites")

    try:
        node = resolve_executable(args.node)
        js_rs = resolve_executable(args.js_rs)
        print(f"Node.js: {version_of(node)} ({node})")
        print(f"js-rs:   {version_of(js_rs)} ({js_rs})")
        print(f"Runs: {args.runs}, process warmups: {args.warmup}, iterations: {args.iterations:,}")

        print(f"Workloads: {', '.join(suites)}")
        source = benchmark_source(args.iterations, suites)
        commands = [("node", node, ()), ("js-rs", js_rs, ())]
        if args.compare_jit:
            commands.append(("js-rs-jit", js_rs, ("--jit-required",)))
        wall_times: dict[str, list[float]] = {engine: [] for engine, _, _ in commands}
        kernel_times: dict[str, dict[str, list[float]]] = {
            engine: {suite: [] for suite in suites} for engine in wall_times
        }
        checksums: dict[str, dict[str, str]] = {engine: {} for engine in wall_times}
        warmup_times: dict[str, dict[str, list[float]]] = {
            engine: {suite: [] for suite in suites} for engine in wall_times
        }

        with tempfile.TemporaryDirectory(prefix="js-engine-bench-") as temp_dir:
            script_path = Path(temp_dir) / "benchmark.js"
            script_path.write_text(source, encoding="utf-8")

            for _ in range(args.warmup):
                for engine, executable, extra_args in commands:
                    results, _, _ = run_engine(executable, script_path, args.timeout, extra_args, suites)
                    for suite, (checksum, _) in results.items():
                        expected = checksums[engine].setdefault(suite, checksum)
                        if checksum != expected:
                            raise RuntimeError(f"{engine} produced inconsistent {suite} warmup checksums")

            for run_index in range(args.runs):
                order = commands if run_index % 2 == 0 else tuple(reversed(commands))
                for engine, executable, extra_args in order:
                    results, wall_ms, warmups = run_engine(executable, script_path, args.timeout, extra_args, suites)
                    wall_times[engine].append(wall_ms)
                    for suite, warmup_ms in warmups.items():
                        warmup_times[engine][suite].append(float(warmup_ms))
                    for suite, (checksum, kernel_ms) in results.items():
                        expected = checksums[engine].setdefault(suite, checksum)
                        if checksum != expected:
                            raise RuntimeError(f"{engine} produced inconsistent {suite} checksums")
                        kernel_times[engine][suite].append(float(kernel_ms))

        for suite in suites:
            for engine in wall_times:
                if checksums["node"][suite] != checksums[engine][suite]:
                    raise RuntimeError(
                        f"{suite} checksum mismatch: Node.js={checksums['node'][suite]}, "
                        f"{engine}={checksums[engine][suite]}"
                    )

        print("\nMedian in-script execution time (lower is faster; js-rs/Node > 1 means js-rs is slower)")
        print(f"{'workload':<18} {'Node.js':>12} {'js-rs':>12} {'js-rs/Node':>14}")
        for suite in suites:
            node_ms = median(kernel_times["node"][suite])
            js_rs_ms = median(kernel_times["js-rs"][suite])
            print(f"{suite:<18} {node_ms:>9.1f} ms {js_rs_ms:>9.1f} ms {format_ratio(js_rs_ms, node_ms):>14}")

        node_wall = median(wall_times["node"])
        js_rs_wall = median(wall_times["js-rs"])
        print("\nMedian end-to-end process time (startup + parse + execution)")
        print(f"Node.js: {node_wall:.1f} ms")
        print(f"js-rs:   {js_rs_wall:.1f} ms")
        print(f"js-rs/Node: {format_ratio(js_rs_wall, node_wall)}")
        if args.report_warmup:
            print("\nMedian first workload invocation (included in process time, not the measured kernel)")
            engines = list(wall_times)
            print(f"{'workload':<18}" + "".join(f" {engine:>12}" for engine in engines))
            for suite in suites:
                print(f"{suite:<18}" + "".join(f" {median(warmup_times[engine][suite]):>9.1f} ms" for engine in engines))
        if args.compare_jit:
            print("\nRequired JIT vs ordinary js-rs (ratio > 1 means JIT is slower)")
            print(f"{'workload':<18} {'ordinary':>12} {'JIT':>12} {'JIT/ordinary':>14}")
            for suite in suites:
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
