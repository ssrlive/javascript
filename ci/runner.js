#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const {spawnSync, spawn} = require('child_process');
const os = require('os');
const {composeTest, extractMeta, parseList, hasFlag} = require('./compose_test');

const REPO_DIR = 'test262';
const RESULTS_FILE = 'test262-results.log';
fs.writeFileSync(RESULTS_FILE, '');

// Defaults
let LIMIT = 100;
let FAIL_ON_FAILURE = false;
let CAP_MULTIPLIER = 5;
// FOCUS_LIST: array of focus tokens. Accepts multiple `--focus` flags and comma-separated tokens.
let FOCUS_LIST = [];
let TIMEOUT_SECS = process.env.TEST_TIMEOUT || 20;
const envKeep = process.env.TEST262_KEEP_TMP;
// Default: keep ephemeral /tmp files (use --no-keep-tmp or set TEST262_KEEP_TMP=0 to disable)
let KEEP_TMP = (envKeep === undefined) ? true : (envKeep === '1' || envKeep === 'true');

// Simple arg parsing
const argv = process.argv.slice(2);
for (let i=0;i<argv.length;i++){
  const a = argv[i];
  if (a === '--limit') { LIMIT = Number(argv[++i]); }
  else if (a === '--fail-on-failure') { FAIL_ON_FAILURE = true; }
  else if (a === '--cap-multiplier') { CAP_MULTIPLIER = Number(argv[++i]); }
  else if (a === '--focus') { const val = argv[++i]; FOCUS_LIST.push(...String(val).split(',').map(s=>s.trim()).filter(Boolean)); }
  else if (a === '--timeout') { TIMEOUT_SECS = Number(argv[++i]); }
  else if (a === '--keep-tmp') { KEEP_TMP = true; }
  else if (a === '--help') { console.log('Usage: node ci/runner.js [--keep-tmp] --limit N --focus name[,name2] (multiple --focus allowed)'); process.exit(0);} 
}

function log(line){ fs.appendFileSync(RESULTS_FILE, line + '\n'); }
console.log(`Running Test262 tests (node runner)`);
console.log(`Ephemeral /tmp files are kept by default (KEEP_TMP=${KEEP_TMP}). Use --keep-tmp to explicitly ensure, or set TEST262_KEEP_TMP=0 to disable.`);

if (!fs.existsSync(REPO_DIR)){
  console.log('Cloning test262...');
  spawnSync('git', ['clone', '--depth', '1', 'https://github.com/tc39/test262.git', REPO_DIR], {stdio:'inherit'});
}

// Build engine
console.log('Building engine example...');
const buildRes = spawnSync('cargo', ['build', '--example', 'js', '--all-features'], {stdio:'inherit'});
if (!buildRes || buildRes.status !== 0) {
  console.error('ERROR: Engine build failed. Aborting tests.');
  process.exit(buildRes && buildRes.status ? buildRes.status : 1);
}

// locate binary
let BIN = '';
if (fs.existsSync('target/debug/examples/js')) BIN = 'target/debug/examples/js';
else if (fs.existsSync('target/debug/js')) BIN = 'target/debug/js';
else BIN = '';
console.log(`JS engine binary: ${BIN}`);

// Build harness index
const HARNESS_INDEX = {};
function walkDir(dir){
  const out = [];
  // Read directory entries and sort by name for deterministic traversal
  let items = fs.readdirSync(dir, {withFileTypes:true});
  items = items.sort((a,b)=>a.name.localeCompare(b.name));
  for (const it of items){
    const p = path.join(dir, it.name);
    if (it.isDirectory()) out.push(...walkDir(p));
    else out.push(p);
  }
  // Return sorted list of paths to ensure caller sees deterministic order
  return out.sort();
}
for (const p of walkDir(path.join(REPO_DIR,'harness'))){
  const b = path.basename(p);
  HARNESS_INDEX[b] = p;
}

// Collect tests under focus
const SEARCH_DIRS = [];
if (FOCUS_LIST.length){
  const toks = FOCUS_LIST;
  for (const tok of toks){
    if (tok === 'language') SEARCH_DIRS.push(path.join(REPO_DIR,'test','language'));
    else if (tok === 'built-ins' || tok === 'builtins') SEARCH_DIRS.push(path.join(REPO_DIR,'test','built-ins'));
    else if (tok === 'intl') SEARCH_DIRS.push(path.join(REPO_DIR,'test','intl402'));
    else if (tok === 'all') SEARCH_DIRS.push(path.join(REPO_DIR,'test'));
    else if (fs.existsSync(path.join(REPO_DIR,'test',tok))) SEARCH_DIRS.push(path.join(REPO_DIR,'test',tok));
    else if (fs.existsSync(tok)) SEARCH_DIRS.push(tok);
  }
} else {
  SEARCH_DIRS.push(path.join(REPO_DIR,'test'));
}

const CAP = LIMIT * CAP_MULTIPLIER;
console.log(`Collecting up to ${CAP} candidate tests (LIMIT=${LIMIT}, CAP_MULTIPLIER=${CAP_MULTIPLIER}). Search dirs: ${SEARCH_DIRS}`);

function collectTests(){
  const basic = [];
  const other = [];
  const intl = [];
  for (const dir of SEARCH_DIRS){
    if (!fs.existsSync(dir)) continue;
    // Support both directories and single-file focus entries. If `dir` is a file,
    // treat it as the sole candidate; if it's a directory, walk it recursively.
    let files = [];
    const stat = fs.statSync(dir);
    if (stat.isFile()) {
      if (dir.endsWith('.js')) files = [dir];
    } else if (stat.isDirectory()) {
      files = walkDir(dir).filter(p=>p.endsWith('.js')).sort();
    }

    for (const f of files){
      const meta = extractMeta(f);
      if ((/features:\s*\[.*Intl.*\]/.test(meta)) || /\bIntl\b/.test(fs.readFileSync(f,'utf8'))) {
        intl.push(f);
      } else if (/includes:|flags:\s*\[.*module.*\]|negative:|features:/.test(meta)) {
        other.push(f);
      } else {
        basic.push(f);
      }
      if ((basic.length + other.length + intl.length) >= CAP) break;
    }
    if ((basic.length + other.length + intl.length) >= CAP) break;
  }
  console.log(`Collected: basic=${basic.length} other=${other.length} intl=${intl.length} (total=${basic.length+other.length+intl.length})`);
  return basic.concat(other, intl);
}

const ordered = collectTests();

// feature probe cache
const FEATURE_SUPPORTED = {};
function detectFeature(feat){
  if (process.env.FORCE_RUN_UNSUPPORTED_FEATURES && process.env.FORCE_RUN_UNSUPPORTED_FEATURES !== 'false') { FEATURE_SUPPORTED[feat] = true; return true; }
  if (feat in FEATURE_SUPPORTED) return FEATURE_SUPPORTED[feat];
  const probeFile = path.join(__dirname, 'feature_probes', feat.replace(/\./g,'_') + '.js');
  if (fs.existsSync(probeFile) && BIN){
    const res = spawnSync(BIN, [probeFile], {timeout:2000});
    const out = (res.stdout||Buffer.from('')).toString();
    if (out.includes('OK')) FEATURE_SUPPORTED[feat] = true; else FEATURE_SUPPORTED[feat] = false;
  } else {
    FEATURE_SUPPORTED[feat] = false;
  }
  return FEATURE_SUPPORTED[feat];
}

let pass=0, fail=0, skip=0, n=0;

/*
  Execution semantics:
  - --limit N controls the number of tests *executed* (pass+fail == N).
  - The CAP_MULTIPLIER is applied to compute how many candidate files to parse (CAP = LIMIT * CAP_MULTIPLIER).
  - Skipped tests (noStrict, negative, missing includes, unsupported features) do NOT count toward the limit.
*/
async function runAll(){
  let execCount = 0; // counts executed tests (pass+fail)
  for (const f of ordered){
    // stop when we've executed LIMIT tests
    if (execCount >= LIMIT) break;
    n++;
    const meta = extractMeta(f);

    // detect noStrict (capture the full flags array reliably)
    const flagsBlock = (meta.match(/flags\s*:\s*\[[\s\S]*?\]/) || [''])[0];
    if (flagsBlock && flagsBlock.includes('noStrict')){ skip++; log(`SKIP (noStrict) ${f}`); continue; }

    // Skip raw tests (they require special raw-source handling)
    if (flagsBlock && flagsBlock.includes('raw')) { skip++; log(`SKIP (raw) ${f}`); continue; }

    // Skip tests marked as pending via esid: pending
    if (/esid\s*:\s*pending\b/.test(meta)) { skip++; log(`SKIP (esid pending) ${f}`); continue; }

    if (/negative:/.test(meta)) { skip++; log(`SKIP (negative) ${f}`); continue; }

    // features
    const feats = (meta.match(/features:\s*\[(.*?)\]/s) || [])[1];
    if (feats){
      const featsList = feats.split(',').map(s=>s.trim().replace(/^['\"]|['\"]$/g,''));
      let unsupported=false;
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

    // fast skip for Intl
    if (/features:/.test(meta) && /Intl/.test(meta)) { skip++; log(`SKIP (feature: Intl) ${f}`); continue; }
    if (/\bIntl\b/.test(fs.readFileSync(f,'utf8'))) { skip++; log(`SKIP (contains Intl) ${f}`); continue; }

    // handle includes
    const includes = parseIncludes(meta);
    let resolved_includes = [];
    let missing=false;
    if (includes.length>0){
      for (const inc of includes){
        const incBasename = inc;
        let incPath = HARNESS_INDEX[incBasename] || '';
        if (!incPath){
          // search repo
          const found = ordered.find(p => path.basename(p) === incBasename) || null;
          if (found) incPath = found; else {
            // search harness
            // fallback: try find under repo harness
            incPath = Object.values(HARNESS_INDEX).find(p => path.basename(p) === incBasename) || '';
          }
        }
        if (!incPath){ log(`MISSING INCLUDE ${inc} for ${f}`); missing=true; break; }
        // special-case compareArray.js -> ensure assert.js present
        if (incBasename === 'compareArray.js'){
          if (HARNESS_INDEX['assert.js']) resolved_includes.push(HARNESS_INDEX['assert.js']);
        }
        resolved_includes.push(incPath);
      }

      // if test references assert but none of the includes supply it, add assert
      if (/\bassert\b/.test(fs.readFileSync(f,'utf8'))){
        let have_assert=false;
        for (const p of resolved_includes){ if (p && (/function\s+assert|var\s+assert/.test(fs.readFileSync(p,'utf8')) || /defines:\s*\[[^\]]*\bassert\b/.test(extractMeta(p)) )) { have_assert=true; break; } }
        if (!have_assert && HARNESS_INDEX['assert.js']){
          const sta = HARNESS_INDEX['sta.js'];
          if (sta) resolved_includes.unshift(sta);
          resolved_includes.unshift(HARNESS_INDEX['assert.js']);
        }
      }

      if (missing){ skip++; log(`SKIP (missing-include) ${f}`); continue; }
    }

    // async flag handling
    if (/flags:\s*\[.*async.*\]/.test(meta)){
      const done = HARNESS_INDEX['doneprintHandle.js'];
      const asyncHelpers = HARNESS_INDEX['asyncHelpers.js'];
      if (done && !resolved_includes.find(p=>path.basename(p)===path.basename(done))) resolved_includes.unshift(done);
      if (asyncHelpers && !resolved_includes.find(p=>path.basename(p)===path.basename(asyncHelpers))) resolved_includes.unshift(asyncHelpers);
    }

    // Compose test
    // Only force a strict wrapper if the test explicitly requests it via the Test262
    // metadata flag 'onlyStrict'. For legacy tests that expect sloppy semantics, do
    // not inject a global "use strict" which can change eval semantics.
    const needStrict = hasFlag(meta, 'onlyStrict');
    const {testToRun, tmpPath, cleanupTmp, debug} = composeTest({testPath: f, repoDir: REPO_DIR, harnessIndex:HARNESS_INDEX, prependFiles: resolved_includes, needStrict});
    // Only emit detailed debug lines when explicitly requested via TEST262_LOG_LEVEL=debug
    if (process.env.TEST262_LOG_LEVEL === 'debug') {
      for (const d of debug) log(d);
    }

    // Run test
    try {
      log(`RUN ${f}`);
      let res;
      if (BIN) {
        // call the built engine binary directly with the composed test file
        res = spawnSync(BIN, [tmpPath], {timeout: TIMEOUT_SECS*1000, encoding:'utf8'});
      } else {
        // fall back to cargo run with appropriate args
        res = spawnSync('cargo', ['run', '--all-features', '--example', 'js', '--', tmpPath], {timeout: TIMEOUT_SECS*1000, encoding:'utf8'});
      }

      if (res && res.status === 0) {
        log(`PASS ${f}`); pass++;
        execCount++;
        // Progress indicator: print a dot to terminal after each successful test
        try { process.stdout.write('.'); } catch (e) { /* ignore */ }
        // On success, remove temporary composed file to avoid clutter
        if (cleanupTmp && tmpPath && fs.existsSync(tmpPath)) {
          try { fs.unlinkSync(tmpPath); } catch (e) { /* ignore */ }
        }
      } else {
        log(`FAIL ${f}`);
        execCount++;
        // Print a concise output summary (prefer stderr, else stdout). Show first non-empty line + up to 4 following lines
        log('---- OUTPUT (summary) ----');
        const outStr = ((res && res.stderr) ? res.stderr : (res && res.stdout ? res.stdout : '') ) || '';
        const outLines = String(outStr).split('\n');
        // find first non-empty line index
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
          summaryText = outLines.slice(0,5).join('\n');
          log(summaryText);
        } else {
          summaryText = '<no output>';
          log(summaryText);
        }
        log('----------------');
        // Also print concise failure summary to terminal (stderr)
        try {
          console.error(`\nFAIL ${f}`);
          console.error(summaryText);
          console.error('----------------');
        } catch (e) { /* ignore terminal print errors */ }
        // If KEEP_TMP is true, keep the ephemeral tmp file in /tmp (do NOT copy into ci/retained)
        if (cleanupTmp && tmpPath && fs.existsSync(tmpPath)){
          try {
            if (KEEP_TMP) {
              log(`TEST FILE KEPT: ${tmpPath}`);
              // in debug mode, include a short head of the file for convenience
              if (process.env.TEST262_LOG_LEVEL === 'debug') {
                const content = fs.readFileSync(tmpPath,'utf8').split('\n').slice(0,60).join('\n');
                log('---- TEST FILE (head 60 lines): ----');
                log(content);
                log('---- END TEST FILE ----');
              }
            }
            // otherwise: do nothing here; tmp will be removed in finally block
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
      log('----------------');
      // Also print exception to terminal (stderr)
      try {
        console.error(`FAIL ${f}`);
        console.error(errText);
        console.error('----------------');
      } catch (e) { /* ignore terminal print errors */ }
      fail++;
    } finally {
      // Remove tmp file unless KEEP_TMP is true; we keep ephemeral /tmp files when KEEP_TMP is true
      try {
        if (cleanupTmp && tmpPath && fs.existsSync(tmpPath)) {
          if (KEEP_TMP) {
            log(`NOTE: keeping ephemeral tmp file ${tmpPath} because TEST262_KEEP_TMP is set`);
          } else {
            fs.unlinkSync(tmpPath);
          }
        }
      } catch (e) {
        // ignore
      }
    }
  }

  log(`Ran ${n} candidates: pass=${pass} fail=${fail} skip=${skip}`);
  log(`Executed ${pass+fail} tests (pass+fail).`);
  console.log(`\nRan ${n} candidates: pass=${pass} fail=${fail} skip=${skip}`);
  console.log(`Executed ${pass+fail} tests (pass+fail).`);
  // Show location of verbose results file
  console.log(`Details in ${RESULTS_FILE}`);
  // Exit non-zero if any tests failed (default behavior)
  if (fail > 0) {
    console.log('One or more tests failed; exiting with status 1');
    process.exit(1);
  }
  process.exit(0);
}

function parseIncludes(meta){
  const re = /includes:\s*\[(.*?)\]/s;
  const m = meta.match(re);
  if (!m) return [];
  return m[1].split(',').map(s=>s.trim().replace(/^['\"]|['\"]$/g,'')).filter(Boolean);
}

runAll();
