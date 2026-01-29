#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const os = require('os');

function extractMeta(filePath) {
  const src = fs.readFileSync(filePath, 'utf8');
  const m = src.match(/\/\*---([\s\S]*?)---\*\//);
  if (!m) return '';
  return m[1];
}

function parseList(meta, key) {
  // simple parser for lines like: includes: ["a.js", "b.js"]
  const re = new RegExp(`${key}:\\s*\\[(.*?)\\]`, 's');
  const m = meta.match(re);
  if (!m) return [];
  const inner = m[1];
  // split by comma but tolerate spaces
  return inner.split(',').map(s => s.trim().replace(/^['\"]|['\"]$/g, '')).filter(Boolean);
}

function hasFlag(meta, name) {
  const re = /flags:\s*\[([\s\S]*?)\]/;
  const m = meta.match(re);
  if (!m) return false;
  return m[1].includes(name);
}

function referencesAssert(filePath) {
  const src = fs.readFileSync(filePath, 'utf8');
  return /\bassert\b/.test(src);
}

function definesAssertInFile(filePath) {
  if (!fs.existsSync(filePath)) return false;
  const src = fs.readFileSync(filePath, 'utf8');
  return /function\s+assert\b|var\s+assert\b|assert\._isSameValue/.test(src) || /defines:\s*\[([^\]]*\bassert\b[^\]]*)\]/.test(extractMeta(filePath));
}

function ensureArrayDistinct(arr) {
  const seen = new Set();
  const out = [];
  for (const p of arr) {
    if (!p) continue;
    const b = path.basename(p);
    if (!seen.has(b)) {
      seen.add(b);
      out.push(p);
    }
  }
  return out;
}

function composeTest({testPath, repoDir, harnessIndex, prependFiles = [], needStrict = true}) {
  // Returns { testToRun, tmpPath, cleanupTmp, debug }
  const debug = [];
  debug.push(`DEBUG PREPEND_BEFORE ${prependFiles.map(p => path.relative('.', p)).join(',')} , for ${testPath}`);

  // prepends are file paths
  let PREPEND_FILES = prependFiles.slice();

  // If test references assert, ensure assert/sta will be available (but keep existing prepends)
  if (referencesAssert(testPath)) {
    const assertPath = harnessIndex['assert.js'];
    const staPath = harnessIndex['sta.js'];
    if (assertPath) {
      // ensure sta then assert at the front but preserve ordering and avoid duplicates
      const fixed = [];
      if (staPath) fixed.push(staPath);
      fixed.push(assertPath);
      // then append existing prepends if not duplicates
      for (const p of PREPEND_FILES) {
        const b = path.basename(p);
        if (!fixed.some(q => path.basename(q) === b)) fixed.push(p);
      }
      PREPEND_FILES = fixed;
    }
  }
  // New: if any of the PREPEND_FILES (includes) reference 'assert' but do NOT define it,
  // ensure sta.js/assert.js are injected before them. This covers harness files that
  // call assert but do not define it (e.g. propertyHelper.js).
  (function ensureAssertForIncludes(){
    const assertPath = harnessIndex['assert.js'];
    const staPath = harnessIndex['sta.js'];
    if (!assertPath) return; // nothing to do if assert.js not available

    // Check each include: if it references assert and does not define it, we need to ensure assert is present
    let needInject = false;
    for (const p of PREPEND_FILES) {
      if (!p || !fs.existsSync(p)) continue;
      const src = fs.readFileSync(p, 'utf8');
      const references = /\bassert\b/.test(src);
      const defines = /function\s+assert\b|var\s+assert\b|assert\._isSameValue/.test(src) || /defines:\s*\[[^\]]*\bassert\b/.test(extractMeta(p));
      if (references && !defines) { needInject = true; break; }
    }
    if (needInject) {
      const fixed = [];
      if (staPath) fixed.push(staPath);
      fixed.push(assertPath);
      for (const p of PREPEND_FILES) {
        const b = path.basename(p);
        if (!fixed.some(q => path.basename(q) === b)) fixed.push(p);
      }
      PREPEND_FILES = fixed;
    }
  })();

  // If the test references Test262Error directly (e.g., older Sputnik tests that use
  // 'throw new Test262Error(...)'), ensure we inject the Test262 harness 'sta.js'
  // which defines Test262Error so the test can run. Prefer harness files from test262.
  (function ensureTest262Error(){
    const src = fs.readFileSync(testPath, 'utf8');
    if (/\bTest262Error\b/.test(src)) {
      // Check if any of the PREPEND_FILES already defines Test262Error
      let definesTest262Error = false;
      for (const p of PREPEND_FILES) {
        if (!p || !fs.existsSync(p)) continue;
        const s = fs.readFileSync(p, 'utf8');
        if (/function\s+Test262Error\b|Test262Error.prototype/.test(s) || /defines:\s*\[[^\]]*\bTest262Error\b/.test(extractMeta(p))) { definesTest262Error = true; break; }
      }
      if (!definesTest262Error) {
        const sta = harnessIndex['sta.js'];
        if (sta && fs.existsSync(sta)) {
          // Place it at the front so it defines Test262Error for subsequent includes/tests
          PREPEND_FILES.unshift(sta);
        }
      }
    }
  })();
  debug.push(`DEBUG PREPEND_AFTER ${PREPEND_FILES.map(p => path.relative('.', p)).join(',')} , for ${testPath}`);

  // If no module tests: add strict wrapper
  // Create tmp file
  const prefix = path.join(os.tmpdir(), 'test262.');
  const tmpName = fs.mkdtempSync(prefix) + '.js';
  const outLines = [];
  if (needStrict) {
    outLines.push('"use strict";');
    outLines.push('');
  }

  // Write unique prepends
  PREPEND_FILES = ensureArrayDistinct(PREPEND_FILES);
  for (const p of PREPEND_FILES) {
    if (!p) continue;
    if (fs.existsSync(p)) {
      outLines.push(fs.readFileSync(p, 'utf8'));
      outLines.push('');
    }
  }

  // Ensure host-provided `print` exists for test harnesses: if absent, bind to console.log
  outLines.push('// Inject: ensure print is defined for harnesses');
  outLines.push('if (typeof print === "undefined") {');
  outLines.push('  if (typeof console !== "undefined" && typeof console.log === "function") {');
  outLines.push('    var print = function(msg) { console.log(msg); };');
  outLines.push('  } else {');
  outLines.push('    var print = function() {};');
  outLines.push('  }');
  outLines.push('}');
  outLines.push('');

  // append test source
  outLines.push(fs.readFileSync(testPath, 'utf8'));

  fs.writeFileSync(tmpName, outLines.join('\n'));

  // verify assert was injected if test references assert
  if (referencesAssert(testPath) && !definesAssertInFile(tmpName)) {
    debug.push(`WARN (assert missing after compose) ${testPath}`);
    // rebuild ensuring sta/assert at top while preserving other PREPEND_FILES
    const fixedTmp = fs.mkdtempSync(prefix) + '.js';
    const lines2 = [];
    if (needStrict) {
      lines2.push('"use strict";');
      lines2.push('');
    }
    const assertPath = harnessIndex['assert.js'];
    const staPath = harnessIndex['sta.js'];
    const fixedPrepend = [];
    if (staPath) fixedPrepend.push(staPath);
    if (assertPath) fixedPrepend.push(assertPath);
    for (const p of PREPEND_FILES) {
      if (!p) continue;
      const b = path.basename(p);
      if (!fixedPrepend.some(q => path.basename(q) === b)) fixedPrepend.push(p);
    }
    const fixedUnique = ensureArrayDistinct(fixedPrepend);
    debug.push(`DEBUG PREPEND_FIXED ${fixedUnique.map(p => path.relative('.', p)).join(',')} , for ${testPath}`);

    for (const p of fixedUnique) {
      lines2.push(fs.readFileSync(p, 'utf8'));
      lines2.push('');
    }
    lines2.push(fs.readFileSync(testPath, 'utf8'));
    fs.writeFileSync(fixedTmp, lines2.join('\n'));
    return {testToRun: fixedTmp, tmpPath: fixedTmp, cleanupTmp: true, debug};
  }

  return {testToRun: tmpName, tmpPath: tmpName, cleanupTmp: true, debug};
}

module.exports = {extractMeta, parseList, hasFlag, composeTest, referencesAssert};
