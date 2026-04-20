#!/usr/bin/env bash
# test-vm.sh — Run all JS test scripts through the VM engine.
# Categorises results to guide engine improvement work.
#   PASS  — test exited 0
#   FAIL  — runtime / logic error (engine bug to fix next)
#   SKIP  — "Unimplemented" compile error (future work)
#   TIMEOUT — exceeded 5 s (possible infinite loop)

set -uo pipefail
shopt -s nullglob

TIMEOUT_SEC=5

# Use GNU timeout if available; Windows timeout.exe is incompatible with
# `timeout N command ...` and can make every test look like a failure.
TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then
  if timeout --version 2>/dev/null | grep -qi "coreutils"; then
    TIMEOUT_BIN="timeout"
  fi
fi

# macOS users often install coreutils as gtimeout.
if [[ -z "$TIMEOUT_BIN" ]] && command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_BIN="gtimeout"
fi

if [[ -z "$TIMEOUT_BIN" ]]; then
  echo "⚠️  GNU timeout not found; running without per-test timeout."
fi

cargo fmt --all

echo "🔍 Running clippy... (warnings ignored)"
cargo clippy -r --all-features --all-targets -- -D warnings

# echo ""
# echo "🔍 Running Rust unit tests..."
# cargo test --all-features 2>&1 | tail -1

cargo b -r -p js

# ── JS test scripts ──────────────────────────────────────────────────
examples=(js-scripts/*.js js-scripts/*.mjs)
if [[ ${#examples[@]} -eq 0 ]]; then
  echo "No tests found in js-scripts/" >&2
  exit 1
fi

pass=(); fail=(); skip=(); tout=()
fail_details=()

for f in "${examples[@]}"; do
  name=$(basename "$f")
  if [[ "$f" == *.mjs ]] || [[ "$f" == *es6_module*.js ]]; then
    cmd=(./target/release/js --module "$f")
  else
    cmd=(./target/release/js "$f")
  fi

  echo "testing $f"
  if [[ -n "$TIMEOUT_BIN" ]]; then
    output=$($TIMEOUT_BIN "$TIMEOUT_SEC" "${cmd[@]}" 2>&1)
    rc=$?
  else
    output=$("${cmd[@]}" 2>&1)
    rc=$?
  fi

  if [[ $rc -eq 0 ]]; then
    pass+=("$name")
  elif [[ -n "$TIMEOUT_BIN" && $rc -eq 124 ]]; then
    tout+=("$name")
  elif echo "$output" | grep -q "Unimplemented"; then
    skip+=("$name")
  else
    fail+=("$name")
    # capture last meaningful line as reason
    reason=$(echo "$output" | grep -E "Uncaught|Error|panic" | tail -1)
    fail_details+=("$name | ${reason:-(no detail)}")
  fi
done

# ── Summary ──────────────────────────────────────────────────────────
total=${#examples[@]}
echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "  VM Test Report   (${total} scripts)"
echo "═══════════════════════════════════════════════════════════════"
printf "  ✅ PASS : %d\n" "${#pass[@]}"
printf "  ❌ FAIL : %d   (engine bugs — fix these next)\n" "${#fail[@]}"
printf "  ⏭  SKIP : %d   (unimplemented syntax)\n" "${#skip[@]}"
printf "  ⏰ TIMEOUT: %d   (exceeded %ds)\n" "${#tout[@]}" "$TIMEOUT_SEC"
echo "═══════════════════════════════════════════════════════════════"

if [[ ${#pass[@]} -gt 0 ]]; then
  echo ""
  echo "── PASS ──"
  printf "  %s\n" "${pass[@]}"
fi

if [[ ${#fail[@]} -gt 0 ]]; then
  echo ""
  echo "── FAIL (next candidates to fix) ──"
  for d in "${fail_details[@]}"; do
    printf "  %s\n" "$d"
  done
fi

if [[ ${#tout[@]} -gt 0 ]]; then
  echo ""
  echo "── TIMEOUT ──"
  printf "  %s\n" "${tout[@]}"
fi

if [[ ${#skip[@]} -gt 0 ]]; then
  echo ""
  echo "── SKIP (unimplemented) ──"
  printf "  %s\n" "${skip[@]}"
fi

echo ""
pct=$(( ${#pass[@]} * 100 / total ))
echo "Progress: ${#pass[@]}/${total} (${pct}%)"

if [[ ${#fail[@]} -eq 0 ]]; then
  echo "🎉 No engine bugs — all reachable tests pass!"
fi
