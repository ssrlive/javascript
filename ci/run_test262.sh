#!/usr/bin/env bash
set -euo pipefail

LIMIT=100
FAIL_ON_FAILURE=false
# Comma-separated list of features to skip (default: Intl)
SKIP_FEATURES="${SKIP_FEATURES:-Intl}"
# Cap multiplier (LIMIT * CAP_MULTIPLIER) can be set via env or CLI; default 5
CAP_MULTIPLIER="${CAP_MULTIPLIER:-5}"
# Focus (comma-separated) e.g., language,built-ins,intl - can be set via env or CLI
FOCUS="${FOCUS:-}"
# Per-test timeout in seconds (can be overridden via --timeout or TEST_TIMEOUT env variable)
TIMEOUT_SECS="${TEST_TIMEOUT:-10}"

usage() {
  cat <<EOF
Usage: $0 [--limit N] [--fail-on-failure] [--cap-multiplier N] [--focus name] [--timeout N]

--limit N            Run at most N tests (default: 100)
--fail-on-failure    Exit non-zero if any test fails (default: false)
--cap-multiplier N   Cap multiplier used when collecting candidates (search cap = LIMIT * CAP_MULTIPLIER). Can also set env CAP_MULTIPLIER (default: 5)
--focus name         Comma-separated focus areas (language,built-ins,intl) or subdirs under test/; can also set env FOCUS
--timeout N          Per-test timeout in seconds (default: 10). Can also set TEST_TIMEOUT env var.
EOF
} 

while [[ $# -gt 0 ]]; do
  case $1 in
    --limit)
      LIMIT="$2"; shift 2;;
    --fail-on-failure)
      FAIL_ON_FAILURE=true; shift;;
    --cap-multiplier)
      CAP_MULTIPLIER="$2"; shift 2;;
    --focus)
      FOCUS="$2"; shift 2;;
    --timeout)
      TIMEOUT_SECS="$2"; shift 2;;
    --help)
      usage; exit 0;;
    *)
      echo "Unknown argument: $1"; usage; exit 1;;
  esac
done

REPO_DIR=test262
RESULTS_FILE=test262-results.log
: > "$RESULTS_FILE"

if [[ ! -d "$REPO_DIR" ]]; then
  echo "Cloning test262..."
  git clone --depth 1 https://github.com/tc39/test262.git "$REPO_DIR"
fi

n=0; pass=0; fail=0; skip=0

echo "Building engine example..."
cargo build --example js --all-features

# Locate example binary
if [[ -x "target/debug/examples/js" ]]; then
  BIN="target/debug/examples/js"
elif [[ -x "target/debug/js" ]]; then
  BIN="target/debug/js"
else
  BIN=""
fi

echo "JS engine binary: ${BIN}"

# Cache for feature probes (feature -> true|false)
declare -A FEATURE_SUPPORTED

if [[ -n "$BIN" ]]; then
  RUN_CMD="$BIN"
else
  echo "Warning: example binary not found, will use 'cargo run --all-features --example js --' (slower)"
  RUN_CMD="cargo run --all-features --example js --"
fi

# Build harness index to speed up include/harness lookups (fast and local to harness)
declare -A HARNESS_INDEX
while IFS= read -r -d '' p; do
  base=$(basename "$p")
  HARNESS_INDEX["$base"]="$p"
done < <(find "$REPO_DIR/harness" -type f -print0 | sort -z)

# Build the collection cap and support focused searches
CAP=$((LIMIT * CAP_MULTIPLIER))

# Prepare search directories based on FOCUS (env or CLI)
SEARCH_DIRS=()
if [[ -n "$FOCUS" ]]; then
  IFS=',' read -ra TOKS <<< "$FOCUS"
  for tok in "${TOKS[@]}"; do
    tok="${tok// /}"
    case "$tok" in
      language) SEARCH_DIRS+=("$REPO_DIR/test/language") ;;
      built-ins|builtins) SEARCH_DIRS+=("$REPO_DIR/test/built-ins") ;;
      intl) SEARCH_DIRS+=("$REPO_DIR/test/intl402") ;;
      all) SEARCH_DIRS+=("$REPO_DIR/test") ;;
      *)
        if [[ -d "$REPO_DIR/test/$tok" ]]; then
          SEARCH_DIRS+=("$REPO_DIR/test/$tok")
        elif [[ -d "$tok" ]]; then
          SEARCH_DIRS+=("$tok")
        fi
        ;;
    esac
  done
else
  SEARCH_DIRS+=("$REPO_DIR/test")
fi

echo "Collecting up to $CAP candidate tests (LIMIT=$LIMIT, CAP_MULTIPLIER=$CAP_MULTIPLIER). Search dirs: ${SEARCH_DIRS[*]}"

basic=()
other=()
intl_tests=()
for dir in "${SEARCH_DIRS[@]}"; do
  if [[ ! -d "$dir" ]]; then
    continue
  fi
  while IFS= read -r -d '' f; do
    meta=$(awk '/\/\*---/{flag=1; next} /---\*\//{flag=0} flag{print}' "$f" || true)
    if (echo "$meta" | grep -q 'features:' && echo "$meta" | grep -q 'Intl') || grep -q '\<Intl\>' "$f"; then
      intl_tests+=("$f")
    elif echo "$meta" | grep -q 'includes:' || echo "$meta" | grep -Eq 'flags:\s*\[.*module.*\]' || echo "$meta" | grep -q 'negative:' || echo "$meta" | grep -q 'features:'; then
      other+=("$f")
    else
      basic+=("$f")
    fi

    if [[ $(( ${#basic[@]} + ${#other[@]} + ${#intl_tests[@]} )) -ge $CAP ]]; then
      break 2
    fi
  done < <(find "$dir" -name '*.js' -print0 | sort -z)
done

echo "Collected: basic=${#basic[@]} other=${#other[@]} intl=${#intl_tests[@]} (total=$((${#basic[@]}+${#other[@]}+${#intl_tests[@]})))"

ordered=("${basic[@]}" "${other[@]}" "${intl_tests[@]}")

# run tests from ordered list
for f in "${ordered[@]}"; do
  # extract metadata inside /*--- ... ---*/
  meta=$(awk '/\/\*---/{flag=1; next} /---\*\//{flag=0} flag{print}' "$f" || true)

  # Per-test state for deferred composition: list of files to prepend and flags
  PREPEND_FILES=()
  NEED_PREPEND=false
  NEED_STRICT=false
  tmp=""

  # Feature detection: skip tests requiring JS features our engine doesn't support
  # Cache results in associative array FEATURE_SUPPORTED so we probe each feature once.
  detect_feature() {
    feat="$1"
    if [[ -n "${FEATURE_SUPPORTED[$feat]:-}" ]]; then
      return 0
    fi
    case "$feat" in
      resizable-arraybuffer)
        probe=$(mktemp /tmp/feat_probe.XXXXXX.js)
        cat > "$probe" <<'JS'
try {
  // Try the modern resizable ArrayBuffer constructor form
  new ArrayBuffer(1, { maxByteLength: 1 });
  console.log('OK');
} catch (e) {
  try {
    // Fallback to testing for older forms or other behaviors
    console.log('NO');
  } catch (e2) {
    console.log('NO');
  }
}
JS
        if timeout 2s $RUN_CMD "$probe" > /tmp/feat_probe_out 2>&1; then
          if grep -q "OK" /tmp/feat_probe_out; then
            FEATURE_SUPPORTED[$feat]=true
          else
            FEATURE_SUPPORTED[$feat]=false
          fi
        else
          FEATURE_SUPPORTED[$feat]=false
        fi
        rm -f "$probe" /tmp/feat_probe_out || true
        ;;
      *)
        # Unknown feature: conservatively assume unsupported
        FEATURE_SUPPORTED[$feat]=false
        ;;
    esac
  }

  # skip tests that reference Intl (fast path) when SKIP_FEATURES contains Intl
  if echo "$meta" | grep -q 'features:' && echo "$meta" | grep -q 'Intl'; then
    skip=$((skip+1))
    echo "SKIP (feature: Intl) $f" >> "$RESULTS_FILE"
    continue
  fi

  # Skip tests that require features our engine doesn't support. Probe each feature once.
  features_list=$(echo "$meta" | sed -n "s/^features:[[:space:]]*\[\(.*\)\].*/\1/p" || true)
  if [[ -n "$features_list" ]]; then
    IFS=',' read -ra FEATS <<< "$(echo "$features_list" | tr -d '[:space:]')"
    for feat in "${FEATS[@]}"; do
      feat=${feat//\"/}
      feat=${feat//\'/}
      detect_feature "$feat"
      if [[ "${FEATURE_SUPPORTED[$feat]}" != "true" ]]; then
        skip=$((skip+1))
        echo "SKIP (feature unsupported: $feat) $f" >> "$RESULTS_FILE"
        continue 2
      fi
    done
  fi

  # also skip if the test source mentions the Intl symbol and SKIP_FEATURES includes Intl
  if echo "$SKIP_FEATURES" | tr ',' '\n' | grep -qx "Intl" && grep -q '\<Intl\>' "$f"; then
    skip=$((skip+1))
    echo "SKIP (contains Intl) $f" >> "$RESULTS_FILE"
    continue
  fi

  # handle includes: try to resolve harness files and prepend them to a temporary test file
  tmp=""
  includes_list=$(echo "$meta" | sed -n "s/^includes:[[:space:]]*\[\(.*\)\].*/\1/p" || true)
  if [[ -n "$includes_list" ]]; then
    resolved_includes=()
    IFS=',' read -ra INCS <<< "$(echo "$includes_list" | tr -d '[:space:]')"
    missing=false
    for inc in "${INCS[@]}"; do
      inc=${inc//\"/}
      inc=${inc//\'/}
      # try harness first using index
      inc_path="${HARNESS_INDEX[$inc]:-}"
      if [[ -z "$inc_path" ]]; then
        inc_path=$(find "$REPO_DIR" -type f -name "$inc" -print -quit 2>/dev/null || true)
      fi
      if [[ -z "$inc_path" ]]; then
        echo "MISSING INCLUDE $inc for $f" >> "$RESULTS_FILE"
        missing=true
        break
      fi

      # Special case: compareArray.js is deprecated/empty, but tests including it EXPECT `assert.compareArray`
      # So if we see compareArray.js, we MUST ensure assert.js is included BEFORE it (or instead of it).
      if [[ "$inc" == "compareArray.js" ]]; then
         # Prepend assert.js if it's not already in the list? 
         # Or just add it to resolved_includes now, and let duplication logic handle it (if we had any).
         # Simpler: just ensure we manually add assert.js to resolved_includes if we see compareArray.js
         # Check if assert.js is already in resolved_includes?
         # Actually, the logic below checks for `grep -q assert` in the test file.
         # But the test file might not use `assert()` directly, only `assert.compareArray()`.
         # And compareArray.js usually depends on assert.js.
         
         # Let's enforce assert.js inclusion if compareArray.js is requested.
         assert_path="${HARNESS_INDEX['assert.js']:-}"
         if [[ -n "$assert_path" ]]; then
             # We should validly check if we already added it, but for now strict appending is safer than missing it.
             # Ideally we want assert.js BEFORE compareArray.js, although compareArray.js is empty so it doesn't matter.
             # BUT `assert.compareArray` is defined in `assert.js`.
             # So we just need `assert.js`.
             resolved_includes+=("$assert_path")
         fi
      fi

      resolved_includes+=("$inc_path")
    done

    # if the test references `assert` but none of the includes supply it, prepend harness/assert.js if available
    if grep -qE '\<assert\>|\<verifyProperty\>' "$f"; then
      have_assert=false
      for p in "${resolved_includes[@]}"; do
        if [[ -z "$p" ]]; then
          continue
        fi
        # Treat the include as an assert provider only if it actually defines `assert`
        # (function or var) or declares it in the Test262 metadata `defines:` block.
        if grep -qE 'function[[:space:]]+assert|var[[:space:]]+assert' "$p"; then
          have_assert=true; break
        fi
        # also check the Test262 metadata for `defines: [assert]`
        if awk '/\/\*---/{flag=1; next} /---\*\//{flag=0} flag{print}' "$p" | grep -q 'defines:' && \
           awk '/\/\*---/{flag=1; next} /---\*\//{flag=0} flag{print}' "$p" | grep -q '\bassert\b'; then
          have_assert=true; break
        fi
      done
      if ! $have_assert; then
        inc_path="${HARNESS_INDEX['assert.js']:-}"
        if [[ -n "$inc_path" ]]; then
          # also prepend Test262Error/sta.js if present (assert uses Test262Error)
          sta_path="${HARNESS_INDEX['sta.js']:-}"
          if [[ -n "$sta_path" ]]; then
            resolved_includes=("$sta_path" "$inc_path" "${resolved_includes[@]}")
          else
            resolved_includes=("$inc_path" "${resolved_includes[@]}")
          fi
        fi
      fi
    fi

    if $missing; then
      skip=$((skip+1))
      echo "SKIP (missing-include) $f" >> "$RESULTS_FILE"
      continue
    fi


    # If resolved_includes contains assert.js but not sta.js, ensure we insert sta.js before assert.js
    if [[ ${#resolved_includes[@]} -gt 0 ]]; then
      have_assert_include=false
      have_sta_include=false
      for p in "${resolved_includes[@]}"; do
        base=$(basename "$p")
        if [[ "$base" == "assert.js" ]]; then
          have_assert_include=true
        fi
        if [[ "$base" == "sta.js" ]]; then
          have_sta_include=true
        fi
      done

      if $have_assert_include && ! $have_sta_include; then
        sta_path="${HARNESS_INDEX['sta.js']:-}"
        if [[ -n "$sta_path" ]]; then
          # Insert sta.js before the first assert.js occurrence
          new_resolved=()
          inserted=false
          for p in "${resolved_includes[@]}"; do
            base=$(basename "$p")
            if [[ "$base" == "assert.js" && "$inserted" == "false" ]]; then
              new_resolved+=("$sta_path")
              inserted=true
            fi
            new_resolved+=("$p")
          done
          resolved_includes=("${new_resolved[@]}")
        fi
      fi
    fi

    # Defer creation of a temporary test file; record includes to prepend later
    PREPEND_FILES=("${resolved_includes[@]}")
    NEED_PREPEND=true
  else
    cleanup_tmp=false
  fi

  # If the test uses `assert` but had no includes, automatically prepend harness/assert.js if available
  if [[ "$cleanup_tmp" != "true" ]]; then
    if grep -q '\<assert\>' "$f"; then
      inc_path="${HARNESS_INDEX['assert.js']:-}"
      if [[ -n "$inc_path" ]]; then
        sta_path="${HARNESS_INDEX['sta.js']:-}"
        # Defer injecting assert and optional sta.js until final composition
        # Preserve existing PREPEND_FILES and append new entries
        PREPEND_FILES=("${PREPEND_FILES[@]}")
        if [[ -n "$sta_path" ]]; then
          PREPEND_FILES+=("$sta_path")
        fi
        PREPEND_FILES+=("$inc_path")
        NEED_PREPEND=true
      fi
    fi
  fi

  if echo "$meta" | grep -Eq 'flags:\s*\[.*module.*\]'; then
    skip=$((skip+1))
    echo "SKIP (module) $f" >> "$RESULTS_FILE"
    continue
  fi

  # skip tests that require non-strict mode (noStrict)
  if echo "$meta" | grep -Eq 'flags:\s*\[.*noStrict.*\]'; then
    skip=$((skip+1))
    echo "SKIP (noStrict) $f" >> "$RESULTS_FILE"
    continue
  fi

  if echo "$meta" | grep -q 'negative:'; then
    skip=$((skip+1))
    echo "SKIP (negative) $f" >> "$RESULTS_FILE"
    continue
  fi

  if [[ $n -ge $LIMIT ]]; then
    break
  fi
  n=$((n+1))

  test_to_run="$f"
  if [[ "$cleanup_tmp" == "true" && -n "$tmp" ]]; then
    test_to_run="$tmp"
  fi

  # Ensure all non-module tests are executed under strict semantics by prepending 'use strict'
  # Create a temporary file that begins with a strict directive and then run that
  # Defer strict wrapper to final composition; mark we need to insert 'use strict'
  NEED_STRICT=true

  # Final safety: if the test references Test262Error, ensure `sta.js` is
  # prepended into whatever file we are about to run (original or tmp). This
  # guarantees Test262Error is defined regardless of previous tmp handling.
  if grep -q '\<Test262Error\>' "$f"; then
    sta_path="${HARNESS_INDEX['sta.js']:-}"
    if [[ -n "$sta_path" ]]; then
      # Add sta.js to PREPEND_FILES unless it's already present or some prepend file defines Test262Error
      already=false
      if [[ ${#PREPEND_FILES[@]} -gt 0 ]]; then
        for p in "${PREPEND_FILES[@]}"; do
          if [[ "$(basename "$p")" == "sta.js" ]]; then
            already=true; break
          fi
          if grep -qE 'function[[:space:]]+Test262Error|class[[:space:]]+Test262Error|var[[:space:]]+Test262Error|Test262Error[[:space:]]*=' "$p"; then
            already=true; break
          fi
        done
      fi
      if ! $already; then
        # Also ensure the test itself doesn't already define Test262Error
        if ! grep -qE 'function[[:space:]]+Test262Error|class[[:space:]]+Test262Error|var[[:space:]]+Test262Error|Test262Error[[:space:]]*=' "$f"; then
          if [[ ${#PREPEND_FILES[@]} -gt 0 ]]; then
            PREPEND_FILES=("$sta_path" "${PREPEND_FILES[@]}")
          else
            PREPEND_FILES=("$sta_path")
          fi
          NEED_PREPEND=true
        fi
      fi
    fi
  fi

  # If the test uses the async flag, ensure we prepend Test262's async harness files
  # Use harness/doneprintHandle.js (defines $DONE) and harness/asyncHelpers.js (defines asyncTest/assert.throwsAsync)
  if echo "$meta" | grep -Eq 'flags:\s*\[.*async.*\]'; then
    done_path="${HARNESS_INDEX['doneprintHandle.js']:-}"
    async_helpers_path="${HARNESS_INDEX['asyncHelpers.js']:-}"

    # Prepend doneprintHandle.js (defines $DONE) if available and not already present
    if [[ -n "$done_path" ]]; then
      already=false
      for p in "${PREPEND_FILES[@]:-}"; do
        if [[ "$(basename "$p")" == "$(basename "$done_path")" ]]; then
          already=true; break
        fi
      done
      if ! $already; then
        PREPEND_FILES=("$done_path" "${PREPEND_FILES[@]:-}")
        NEED_PREPEND=true
      fi
    fi

    # Prepend asyncHelpers.js (defines asyncTest/assert.throwsAsync) if available and not already present
    if [[ -n "$async_helpers_path" ]]; then
      already=false
      for p in "${PREPEND_FILES[@]:-}"; do
        if [[ "$(basename "$p")" == "$(basename "$async_helpers_path")" ]]; then
          already=true; break
        fi
      done
      if ! $already; then
        PREPEND_FILES=("$async_helpers_path" "${PREPEND_FILES[@]:-}")
        NEED_PREPEND=true
      fi
    fi
  fi

  # If we deferred any prepends or strict wrapping, compose a single temporary test file now
  if [[ "${NEED_PREPEND:-false}" == "true" || "${NEED_STRICT:-false}" == "true" ]]; then
    TMP_TEST_FILE=$(mktemp /tmp/test262.XXXXXX.js)
    if [[ "${NEED_STRICT:-false}" == "true" ]]; then
      echo '"use strict";' > "$TMP_TEST_FILE"
      echo -e "\n" >> "$TMP_TEST_FILE"
    else
      : > "$TMP_TEST_FILE"
    fi
    declare -A _seen_prepend=()
    if [[ ${#PREPEND_FILES[@]} -gt 0 ]]; then
      for p in "${PREPEND_FILES[@]}"; do
        b=$(basename "$p")
        if [[ -n "${_seen_prepend[$b]:-}" ]]; then
          continue
        fi
        _seen_prepend[$b]=1
        cat "$p" >> "$TMP_TEST_FILE"
        echo -e "\n" >> "$TMP_TEST_FILE"
      done
    fi
    cat "$f" >> "$TMP_TEST_FILE"
    test_to_run="$TMP_TEST_FILE"
    tmp="$TMP_TEST_FILE"
    cleanup_tmp=true
  fi

  echo "RUN $f"
  # run with timeout to avoid hangs (per-test timeout configurable via TEST_TIMEOUT or --timeout)
  if timeout ${TIMEOUT_SECS}s $RUN_CMD "$test_to_run" > /tmp/test262_run_out 2>&1; then
    echo "PASS $f" | tee -a "$RESULTS_FILE"
    pass=$((pass+1))
  else
    echo "FAIL $f" | tee -a "$RESULTS_FILE"
    echo "---- OUTPUT ----" >> "$RESULTS_FILE"
    cat /tmp/test262_run_out >> "$RESULTS_FILE"
    echo "----------------" >> "$RESULTS_FILE"
    fail=$((fail+1))
  fi

  # cleanup temporary test file if created
  if [[ "$cleanup_tmp" == "true" && -n "${tmp:-}" ]]; then
    rm -f "$tmp" || true
  fi
  # Reset per-test state
  PREPEND_FILES=()
  NEED_PREPEND=false
  NEED_STRICT=false
  tmp=""

done

# summary
echo "Ran $n tests: pass=$pass fail=$fail skip=$skip"
echo "Details in $RESULTS_FILE"

if [[ "$FAIL_ON_FAILURE" == "true" && $fail -gt 0 ]]; then
  echo "One or more tests failed. Exiting with failure as requested."
  exit 1
fi
