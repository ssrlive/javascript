#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const { spawnSync, spawn } = require('child_process');
const { composeTest, extractMeta, parseList, hasFlag, referencesAssert } = require('./compose_test');

const TEST262_ROOT_DIR = path.resolve(__dirname, '..', '..', 'test262');
const RESULTS_FILE = 'test262-results.log';

// Defaults
let LIMIT = 100;
let FAIL_ON_FAILURE = false;
let CAP_MULTIPLIER = 5;
let JOBS = Number(process.env.TEST262_JOBS || 8);
// FOCUS_LIST: array of focus tokens. Accepts multiple `--focus` flags and comma-separated tokens.
let FOCUS_LIST = [];
const DEFAULT_FOCUS_DIRS = ['annexB', 'built-ins', 'intl402', 'language'];
let TIMEOUT_SECS = process.env.TEST_TIMEOUT || 60;
const envKeep = process.env.TEST262_KEEP_TMP;
// Default: delete composed temporary files (set TEST262_KEEP_TMP=1 to keep)
let KEEP_TMP = (envKeep === undefined) ? false : (envKeep === '1' || envKeep === 'true');

// Simple arg parsing
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  if (a === '--limit') {
    LIMIT = Number(argv[++i]);
  } else if (a === '--fail-on-failure') {
    FAIL_ON_FAILURE = true;
  } else if (a === '--cap-multiplier') {
    CAP_MULTIPLIER = Number(argv[++i]);
  } else if (a === '--jobs') {
    JOBS = Number(argv[++i]);
  } else if (a === '--focus') {
    const val = argv[++i];
    FOCUS_LIST.push(...String(val).split(',').map(s => s.trim()).filter(Boolean));
  } else if (a === '--timeout') {
    TIMEOUT_SECS = Number(argv[++i]);
  } else if (a === '--keep-tmp') {
    KEEP_TMP = true;
  } else if (a === '--help') {
    console.log('Usage: node ci/runner.js [--keep-tmp] [--jobs N] [--limit N] [--focus name[,name2]] (multiple --focus allowed)');
    console.log('  Append (filesonly) to a focus token to collect only top-level files, e.g. "a/(filesonly)",b/c');
    console.log(`  If --focus is omitted, defaults to: ${DEFAULT_FOCUS_DIRS.join(', ')}`);
    process.exit(0);
  }
}

JOBS = Math.max(1, Number.isFinite(JOBS) ? Math.floor(JOBS) : 1);

function formatElapsed(elapsedMs) {
  const totalSeconds = Math.floor(elapsedMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const ms = elapsedMs % 1000;

  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s ${ms}ms`;
  if (minutes > 0) return `${minutes}m ${seconds}s ${ms}ms`;
  if (seconds > 0) return `${seconds}s ${ms}ms`;
  return `${ms}ms`;
}

function cleanupComposedArtifacts(tmpPath) {
  try {
    if (tmpPath && fs.existsSync(tmpPath)) {
      fs.unlinkSync(tmpPath);
    }
    if (tmpPath) {
      const bootstrapPath = tmpPath.replace('/.test262_composed_', '/.test262_bootstrap_');
      if (bootstrapPath !== tmpPath && fs.existsSync(bootstrapPath)) {
        fs.unlinkSync(bootstrapPath);
      }
    }
  } catch (e) {
    // ignore
  }
}

console.log(`Running Test262 tests (node runner)`);
console.log(`Composed temporary files are deleted by default (KEEP_TMP=${KEEP_TMP}). Use --keep-tmp to explicitly ensure, or set TEST262_KEEP_TMP=1 to enable.`);
console.log(`Execution jobs: ${JOBS}`);

function runCommandAsync(cmd, args, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(cmd, args, options);
    let stdout = '';
    let stderr = '';
    let settled = false;
    const timeoutMs = Number(options.timeout || 0);

    if (child.stdout) {
      child.stdout.on('data', (d) => {
        stdout += d.toString();
      });
    }
    if (child.stderr) {
      child.stderr.on('data', (d) => {
        stderr += d.toString();
      });
    }

    let timeoutId = null;
    if (timeoutMs > 0) {
      timeoutId = setTimeout(() => {
        if (settled) return;
        try { child.kill('SIGKILL'); } catch (e) { /* ignore */ }
      }, timeoutMs);
    }

    child.on('error', (err) => {
      if (settled) return;
      settled = true;
      if (timeoutId) clearTimeout(timeoutId);
      resolve({ status: -1, stdout, stderr: `${stderr}\n${String(err)}` });
    });

    child.on('close', (code) => {
      if (settled) return;
      settled = true;
      if (timeoutId) clearTimeout(timeoutId);
      resolve({ status: code, stdout, stderr });
    });
  });
}

if (!fs.existsSync(TEST262_ROOT_DIR)) {
  console.log('Cloning test262...');
  spawnSync('git', ['clone', '--depth', '1', 'https://github.com/tc39/test262.git', TEST262_ROOT_DIR], { stdio: 'inherit' });
}

// Build engine
console.log('Building engine interpreter...');
const USE_RELEASE = process.env.TEST262_RELEASE !== '0';
const buildArgs = ['build', '-p', 'js', '--all-features'];
if (USE_RELEASE) buildArgs.push('--release');
const buildRes = spawnSync('cargo', buildArgs, { stdio: 'inherit' });
if (!buildRes || buildRes.status !== 0) {
  console.error('ERROR: Engine build failed. Aborting tests.');
  process.exit(buildRes && buildRes.status ? buildRes.status : 1);
}

// Locate binary
let BIN = '';
const binCandidates = USE_RELEASE ? ['target/release/js', 'target/release/js.exe'] : ['target/debug/js', 'target/debug/js.exe'];

for (const candidate of binCandidates) {
  if (fs.existsSync(candidate)) {
    BIN = path.resolve(candidate);
    break;
  }
}

if (!BIN) {
  console.error('ERROR: JS engine binary not found after build. Checked paths:');
  for (const candidate of binCandidates) {
    console.error(`  - ${path.resolve(candidate)}`);
  }
  process.exit(1);
}
console.log(`JS engine binary: ${BIN}`);

const SCRIPT_START_NS = process.hrtime.bigint();
process.on('exit', (code) => {
  const elapsedMs = Number((process.hrtime.bigint() - SCRIPT_START_NS) / 1000000n);
  const elapsedLine = `Total elapsed: ${formatElapsed(elapsedMs)} (${elapsedMs} ms), exit=${code}`;
  console.log(elapsedLine);
  log(elapsedLine);
});

fs.writeFileSync(RESULTS_FILE, '');
function log(line) { fs.appendFileSync(RESULTS_FILE, line + '\n'); }

// Build harness index
const HARNESS_INDEX = {};
function shouldSkipEntry(entryName) {
  return entryName.startsWith('.test262_composed_');
}

function walkDir(dir) {
  const out = [];
  let items = fs.readdirSync(dir, { withFileTypes: true });
  items = items.sort((a, b) => a.name.localeCompare(b.name, 'en', { numeric: true }));
  for (const it of items) {
    if (shouldSkipEntry(it.name)) continue;
    const p = path.join(dir, it.name);
    if (it.isDirectory()) out.push(...walkDir(p));
    else out.push(p);
  }
  return out.sort((a, b) => a.localeCompare(b, 'en', { numeric: true }));
}

for (const p of walkDir(path.join(TEST262_ROOT_DIR, 'harness'))) {
  const b = path.basename(p);
  HARNESS_INDEX[b] = p;
}

// Collect tests under focus
const FILES_ONLY_MARKER = '(filesonly)';
const SEARCH_DIRS = [];

function stripFilesOnlyMarker(raw) {
  let text = String(raw || '').trim();
  let filesOnly = false;
  if (text.endsWith(FILES_ONLY_MARKER)) {
    filesOnly = true;
    text = text.slice(0, -FILES_ONLY_MARKER.length).trim();
    while (text.endsWith('/') || text.endsWith('\\')) {
      text = text.slice(0, -1);
    }
  }
  return { text, filesOnly };
}

if (FOCUS_LIST.length) {
  const toks = FOCUS_LIST;
  for (const tokRaw of toks) {
    const { text: tok, filesOnly } = stripFilesOnlyMarker(tokRaw);
    if (!tok) continue;
    if (fs.existsSync(path.join(TEST262_ROOT_DIR, 'test', tok))) {
      SEARCH_DIRS.push({ path: path.join(TEST262_ROOT_DIR, 'test', tok), filesOnly });
    } else {
      console.error(`WARNING: Focus token "${tokRaw}" (stripped to "${tok}") does not exist as a file or directory under test262/test/. Skipping.`);
    }
  }
} else {
  for (const dir of DEFAULT_FOCUS_DIRS) {
    const p = path.join(TEST262_ROOT_DIR, 'test', dir);
    if (fs.existsSync(p)) {
      SEARCH_DIRS.push({ path: p, filesOnly: false });
    } else {
      console.error(`WARNING: Default focus directory not found: ${p}`);
    }
  }
}

const CAP = LIMIT * CAP_MULTIPLIER;
const searchDirsLabel = SEARCH_DIRS
  .map(entry => `${entry.path}${entry.filesOnly ? FILES_ONLY_MARKER : ''}`)
  .join(',');
console.log(`Collecting up to ${CAP} candidate tests (LIMIT=${LIMIT}, CAP_MULTIPLIER=${CAP_MULTIPLIER}). Search dirs: ${searchDirsLabel}`);

function listFilesOnly(dir) {
  let items = fs.readdirSync(dir, { withFileTypes: true });
  items = items.sort((a, b) => a.name.localeCompare(b.name, 'en', { numeric: true }));
  return items
    .filter(it => !shouldSkipEntry(it.name))
    .filter(it => it.isFile())
    .map(it => path.join(dir, it.name))
    .sort((a, b) => a.localeCompare(b, 'en', { numeric: true }));
}

function collectTests() {
  const basic = [];
  const other = [];
  for (const entry of SEARCH_DIRS) {
    const dir = entry.path;
    if (!fs.existsSync(dir)) continue;

    let files = [];
    const stat = fs.statSync(dir);
    if (stat.isFile()) {
      if (dir.endsWith('.js')) files = [dir];
    } else if (stat.isDirectory()) {
      if (entry.filesOnly) {
        files = listFilesOnly(dir).filter(p => p.endsWith('.js')).sort();
      } else {
        files = walkDir(dir).filter(p => p.endsWith('.js')).sort();
      }
    }

    for (const f of files) {
      if (f.includes('/.test262_composed_')) continue;
      const meta = extractMeta(f);
      if (/includes:|flags:\s*\[.*module.*\]|negative:|features:/.test(meta)) {
        other.push(f);
      } else {
        basic.push(f);
      }
      if ((basic.length + other.length) >= CAP) break;
    }
    if ((basic.length + other.length) >= CAP) break;
  }
  console.log(`Collected: basic=${basic.length} other=${other.length} (total=${basic.length + other.length})`);
  return basic.concat(other);
}

const ordered = collectTests();

// Feature probe cache
const FEATURE_SUPPORTED = {};
// Hard-coded unsupported features
const HARDCODED_UNSUPPORTED = new Set([]);

function findProbeFile(feat) {
  const names = new Set([
    feat,
    feat.replace(/\./g, '_'),
  ]);
  for (const name of names) {
    const probeFile = path.join(__dirname, 'feature_probes', `${name}.js`);
    if (fs.existsSync(probeFile)) return probeFile;
  }
  return null;
}

function detectFeature(feat) {
  // Allow environment override to force running unsupported features
  if (process.env.FORCE_RUN_UNSUPPORTED_FEATURES && process.env.FORCE_RUN_UNSUPPORTED_FEATURES !== 'false') {
    FEATURE_SUPPORTED[feat] = true;
    return true;
  }

  // Short-circuit for known-unsupported features
  if (HARDCODED_UNSUPPORTED.has(feat)) {
    FEATURE_SUPPORTED[feat] = false;
    return false;
  }

  if (feat in FEATURE_SUPPORTED) return FEATURE_SUPPORTED[feat];

  // Intl.* features and related Intl flags are assumed supported
  if (/^Intl\./i.test(feat) || feat === 'Intl-enumeration' || feat === 'intl-normative-optional') {
    FEATURE_SUPPORTED[feat] = true;
    return true;
  }

  const probeFile = findProbeFile(feat);
  if (probeFile && BIN) {
    // Determine whether this probe should be run as an ES module.
    let probeIsModule = false;
    try {
      const src = fs.readFileSync(probeFile, 'utf8');
      if (/^\s*\/\/\s*module\b/m.test(src)) probeIsModule = true;
      if (/^\s*await\b/m.test(src)) probeIsModule = true;
      if (/^\s*import\b/m.test(src)) probeIsModule = true;
      if (/^\s*export\b/m.test(src)) probeIsModule = true;
    } catch (e) {
      probeIsModule = false;
    }

    const runArgs = [];
    if (probeIsModule) runArgs.push('--module');
    runArgs.push(probeFile);
    const probeTimeoutMs = Math.max(5000, Number(TIMEOUT_SECS || 0) * 1000);
    const res = spawnSync(BIN, runArgs, { timeout: probeTimeoutMs, encoding: 'utf8' });
    const out = (res && res.stdout) ? String(res.stdout) : '';
    FEATURE_SUPPORTED[feat] = out.includes('OK');
  } else {
    FEATURE_SUPPORTED[feat] = false;
  }
  return FEATURE_SUPPORTED[feat];
}

let pass = 0, fail = 0, skip = 0, n = 0;

// Skip tests known to be too slow for a tree-walking interpreter
const SLOW_TESTS = [
  // 'S15.1.3.1_A2.5_T1.js',
  // 'S15.1.3.1_A2.4_T1.js',
  // 'S15.1.3.2_A2.5_T1.js',
  // 'S15.1.3.2_A2.4_T1.js',
];

// Skip entire directories whose tests are too slow
const SLOW_DIRS = [
  // 'built-ins/RegExp/property-escapes/generated',
];

/*
  Execution semantics:
  - --limit N controls the number of tests *executed* (pass+fail == N).
  - The CAP_MULTIPLIER is applied to compute how many candidate files to parse (CAP = LIMIT * CAP_MULTIPLIER).
  - Skipped tests do NOT count toward the limit.
*/
async function runAll() {
  let scheduledCount = 0;
  const running = new Set();

  async function runSingleComposedTest(f, tmpPath, cleanupTmp, testCwd, isModule, expectedNegative, needsAgent) {
    let currentSucceeds = false;
    try {
      log(`RUN ${f}`);
      let res;
      if (BIN) {
        const binArgs = [];
        if (isModule) binArgs.push('--module');
        binArgs.push(tmpPath);
        res = await runCommandAsync(BIN, binArgs, { timeout: TIMEOUT_SECS * 1000, cwd: testCwd });
      } else {
        const cargoArgs = ['run', '--all-features', '--package', 'js', '--'];
        if (isModule) cargoArgs.push('--module');
        cargoArgs.push(tmpPath);
        res = await runCommandAsync('cargo', cargoArgs, { timeout: TIMEOUT_SECS * 1000, cwd: testCwd });
      }

      const outStr = ((res && res.stderr) ? res.stderr : (res && res.stdout ? res.stdout : '')) || '';
      let isPass = false;
      let failReason = '';

      if (expectedNegative) {
        if (res && res.status === 0) {
          failReason = 'Expected test to fail, but it succeeded.';
        } else if (!outStr.includes(expectedNegative.type)) {
          failReason = `Expected error type ${expectedNegative.type} not found in output.`;
        } else {
          isPass = true;
        }
      } else {
        if (res && res.status === 0) {
          isPass = true;
        } else {
          failReason = 'Test failed with non-zero exit code.';
        }
      }

      if (isPass) {
        log(`PASS ${f}`);
        pass++;
        try { process.stdout.write('.'); } catch (e) { /* ignore */ }
        if (cleanupTmp) cleanupComposedArtifacts(tmpPath);
        currentSucceeds = true;
      } else {
        if (failReason) {
          log(`FAIL ${f} - ${failReason}`);
        } else {
          log(`FAIL ${f}`);
        }
        log('---- OUTPUT (summary) ----');
        const outLines = String(outStr).split('\n');
        let idx = 0;
        while (idx < outLines.length && outLines[idx].trim() === '') idx++;
        let summaryText = '';
        if (idx < outLines.length) {
          summaryText = outLines.slice(idx, idx + 5).join('\n');
          log(summaryText);
        } else if (outLines.length > 0 && outLines.join('').trim() === '') {
          summaryText = '<no output>';
          log(summaryText);
        } else if (outLines.length > 0) {
          summaryText = outLines.slice(0, 5).join('\n');
          log(summaryText);
        } else {
          summaryText = '<no output>';
          log(summaryText);
        }
        try {
          if (failReason) {
            console.error(`\nFAIL ${f} - ${failReason}`);
          } else {
            console.error(`\nFAIL ${f}`);
          }
          console.error(summaryText);
          if (KEEP_TMP) console.error(`TEST FILE KEPT: ${tmpPath}`);
          console.error('----------------');
        } catch (e) { /* ignore terminal print errors */ }
        if (cleanupTmp && tmpPath && fs.existsSync(tmpPath)) {
          try {
            if (KEEP_TMP) {
              log(`TEST FILE KEPT: ${tmpPath}`);
              if (process.env.TEST262_LOG_LEVEL === 'debug') {
                const content = fs.readFileSync(tmpPath, 'utf8').split('\n').slice(0, 60).join('\n');
                log('---- TEST FILE (head 60 lines): ----');
                log(content);
                log('---- END TEST FILE ----');
              }
            }
          } catch (e) {
            log(`WARN (retain-check failed) ${e}`);
          }
        }
        fail++;
      }
    } catch (err) {
      log(`FAIL ${f}`);
      log('---- OUTPUT ----');
      const errText = String(err);
      log(errText);
      try {
        console.error(`FAIL ${f}`);
        console.error(errText);
        if (KEEP_TMP) console.error(`TEST FILE KEPT: ${tmpPath}`);
        console.error('----------------');
      } catch (e) { /* ignore terminal print errors */ }
      fail++;
    } finally {
      try {
        if (cleanupTmp) {
          if (currentSucceeds || !KEEP_TMP) {
            cleanupComposedArtifacts(tmpPath);
          } else {
            log(`NOTE: keeping composed temp file ${tmpPath} because TEST262_KEEP_TMP is set`);
          }
        }
      } catch (e) {
        // ignore
      }
      if (!currentSucceeds) log('----------------');
    }
  }

  for (const f of ordered) {
    if (scheduledCount >= LIMIT) break;
    n++;

    if (/_FIXTURE\.js$/.test(f)) { skip++; log(`SKIP (fixture) ${f}`); continue; }
    if (SLOW_TESTS.some(s => f.endsWith(s))) { skip++; log(`SKIP (slow) ${f}`); continue; }
    if (SLOW_DIRS.some(d => f.includes(d))) { skip++; log(`SKIP (slow-dir) ${f}`); continue; }

    const meta = extractMeta(f);

    // Detect noStrict
    const flagsBlock = (meta.match(/flags\s*:\s*\[[\s\S]*?\]/) || [''])[0];
    if (flagsBlock && flagsBlock.includes('noStrict')) { skip++; log(`SKIP (noStrict) ${f}`); continue; }

    // Parse negative metadata for negative-test result handling.
    let expectedNegative = null;
    if (/negative:\s*/.test(meta)) {
      const blockMatch = meta.match(/negative:[\s\S]*?(?=(?:\n[^\s]|-{3}|$))/);
      if (blockMatch) {
        const tMatch = blockMatch[0].match(/type:\s*(\w+)/);
        const pMatch = blockMatch[0].match(/phase:\s*(\w+)/);
        if (tMatch) {
          expectedNegative = {
            type: tMatch[1],
            phase: pMatch ? pMatch[1] : 'runtime'
          };
        }
      }
    }

    // Feature detection
    const feats = (meta.match(/features:\s*\[(.*?)\]/s) || [])[1];
    if (feats) {
      const featsList = feats.split(',').map(s => s.trim().replace(/^['\"]|['\"]$/g, ''));
      let unsupported = false;
      for (const ft of featsList) {
        if (!detectFeature(ft)) {
          unsupported = true;
          log(`SKIP (feature unsupported: ${ft}) ${f}`);
          skip++;
          break;
        }
      }
      if (unsupported) continue;
    }

    // Read test source once and reuse for all checks below
    const testSrc = fs.readFileSync(f, 'utf8');

    // Detect tests that require $262.agent (multi-threaded worker support).
    // These require an engine-side host implementation. Do not route them through Node,
    // because that tests the host shim rather than the Rust engine.
    const needsAgent = /\$262\.agent\b/.test(testSrc);

    // Handle includes
    const includes = parseList(meta, 'includes');
    let resolved_includes = [];
    let missing = false;
    if (includes.length > 0) {
      for (const inc of includes) {
        const incBasename = inc;
        let incPath = HARNESS_INDEX[incBasename] || '';

        // If the include has a path prefix, try resolving relative to harness first
        if (!incPath && inc.includes('/')) {
          const rel = path.join(TEST262_ROOT_DIR, 'harness', inc);
          if (fs.existsSync(rel)) incPath = rel;
          if (!incPath) {
            const bn = path.basename(inc);
            incPath = HARNESS_INDEX[bn] || '';
          }
        }

        if (!incPath) {
          const found = ordered.find(p => path.basename(p) === incBasename) || null;
          if (found) {
            incPath = found;
          } else {
            incPath = Object.values(HARNESS_INDEX).find(p => path.basename(p) === incBasename) || '';
          }
        }

        if (!incPath) { log(`MISSING INCLUDE ${inc} for ${f}`); missing = true; break; }

        // Special-case compareArray.js -> ensure assert.js present
        if (incBasename === 'compareArray.js') {
          if (HARNESS_INDEX['assert.js']) resolved_includes.push(HARNESS_INDEX['assert.js']);
        }
        resolved_includes.push(incPath);
      }
      if (missing) { skip++; log(`SKIP (missing-include) ${f}`); continue; }
    }

    // Async flag handling
    if (/flags:\s*\[.*async.*\]/.test(meta)) {
      const done = HARNESS_INDEX['doneprintHandle.js'];
      const asyncHelpers = HARNESS_INDEX['asyncHelpers.js'];
      if (done && !resolved_includes.find(p => path.basename(p) === path.basename(done))) {
        resolved_includes.unshift(done);
      }
      if (asyncHelpers && !resolved_includes.find(p => path.basename(p) === path.basename(asyncHelpers))) {
        resolved_includes.unshift(asyncHelpers);
      }
    }

    // Compose test
    const isModule = hasFlag(meta, 'module');
    const needStrict = !isModule && hasFlag(meta, 'onlyStrict');
    const { testToRun, tmpPath, cleanupTmp } = composeTest({
      testPath: f,
      repoDir: TEST262_ROOT_DIR,
      harnessIndex: HARNESS_INDEX,
      prependFiles: resolved_includes,
      needStrict,
      needsAgent,
      expectedNegative
    });

    const testCwd = path.dirname(tmpPath);

    const p = runSingleComposedTest(f, tmpPath, cleanupTmp, testCwd, isModule, expectedNegative, needsAgent)
      .finally(() => running.delete(p));
    running.add(p);
    scheduledCount++;

    if (running.size >= JOBS) {
      await Promise.race(Array.from(running));
    }
  }

  if (running.size > 0) {
    await Promise.all(Array.from(running));
  }

  log(`Ran ${n} candidates: pass=${pass} fail=${fail} skip=${skip}`);
  log(`Executed ${pass + fail} tests (pass+fail).`);
  console.log(`\nRan ${n} candidates: pass=${pass} fail=${fail} skip=${skip}`);
  console.log(`Executed ${pass + fail} tests (pass+fail).`);
  console.log(`Details in ${RESULTS_FILE}`);
  if (fail > 0) {
    console.log('One or more tests failed; exiting with status 1');
    process.exit(1);
  }
  process.exit(0);
}

runAll();
