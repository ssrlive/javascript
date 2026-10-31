"""Regression tests for benchmark generation, selection and result validation."""

import contextlib
import io
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

import compare_engines as bench


class BenchmarkTests(unittest.TestCase):
    def test_generated_source_measures_only_selected_workloads(self):
        source = bench.benchmark_source(101, ("json", "bigint"))
        self.assertNotIn("__ITERATIONS__", source)
        self.assertNotIn("__MEASURE_CALLS__", source)
        self.assertIn("const ITERATIONS = 101;", source)
        calls = [line for line in source.splitlines() if line.startswith("measure(")]
        self.assertEqual(calls, ['measure("json", jsonRoundtrip);', 'measure("bigint", bigintArithmetic);'])

    def test_every_registered_workload_has_a_function_and_measurement(self):
        source = bench.benchmark_source(1, bench.SUITES)
        for name, (function, _) in bench.WORKLOADS.items():
            self.assertIn(f"function {function}(n)", source)
            self.assertIn(f'measure("{name}", {function});', source)

    def run_output(self, output):
        result = subprocess.CompletedProcess([], 0, stdout=output, stderr="")
        with patch.object(bench.subprocess, "run", return_value=result):
            return bench.run_engine("engine", Path("benchmark.js"), 1, suites=("json",))

    def test_selected_output_is_accepted(self):
        results, wall, warmups = self.run_output("BENCH_RESULT:json:42:3\nBENCH_WARMUP:json:1\n")
        self.assertEqual(results, {"json": ("42", 3)})
        self.assertEqual(warmups, {"json": 1})
        self.assertGreaterEqual(wall, 0)

    def test_missing_duplicate_unexpected_and_nonfinite_results_fail(self):
        valid = "BENCH_RESULT:json:42:3\nBENCH_WARMUP:json:1\n"
        outputs = [
            "",
            valid + "BENCH_RESULT:json:42:3\n",
            valid + "BENCH_WARMUP:json:1\n",
            valid.replace("json", "strings"),
            valid.replace(":42:", ":NaN:"),
            valid.replace(":42:", ":Infinity:"),
            valid.replace(":42:", ":not-a-number:"),
            valid.replace("BENCH_WARMUP:json:1\n", ""),
        ]
        for output in outputs:
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                self.run_output(output)

    def test_list_suites_does_not_launch_engines(self):
        with patch("sys.argv", ["compare_engines.py", "--list-suites"]), \
                patch.object(bench, "resolve_executable") as resolve, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(bench.main(), 0)
            resolve.assert_not_called()
        self.assertIn("typed_arrays", output.getvalue())

    def test_unknown_or_empty_suite_fails_before_launch(self):
        for name in ["missing", "", "json,"]:
            with self.subTest(name=name), \
                    patch("sys.argv", ["compare_engines.py", "--suite", name]), \
                    patch.object(bench, "resolve_executable") as resolve, \
                    contextlib.redirect_stderr(io.StringIO()), \
                    self.assertRaises(SystemExit) as error:
                bench.main()
            self.assertEqual(error.exception.code, 2)
            resolve.assert_not_called()

    def test_comma_selection_and_repeated_flags_deduplicate(self):
        argv = ["compare_engines.py", "--suite", "json, bigint", "--suite", "json",
                "--runs", "1", "--warmup", "0"]
        results = {"json": ("42", 1), "bigint": ("7", 1)}
        with patch("sys.argv", argv), \
                patch.object(bench, "resolve_executable", side_effect=lambda name: name), \
                patch.object(bench, "version_of", return_value="test"), \
                patch.object(bench, "run_engine", return_value=(results, 2.0, {"json": 1, "bigint": 1})) as run, \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(bench.main(), 0)
        self.assertEqual(run.call_count, 2)
        for call in run.call_args_list:
            self.assertEqual(call.args[-1], ("json", "bigint"))


if __name__ == "__main__":
    unittest.main()
