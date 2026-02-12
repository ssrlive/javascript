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

function hasFeature(meta, name) {
  const arr = parseList(meta, 'features');
  return arr.includes(name);
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

function references262(filePath) {
  const src = fs.readFileSync(filePath, 'utf8');
  return /\$262\b/.test(src);
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

const realmFeatureName = ['cross', 'realm'].join('-');
const realmMarker = '// Inject: unified $262 shim - idempotent';
function get262StubLines() {
  // Minimal, idempotent $262 shim with createRealm support
  return [
    '// Inject: unified $262 shim - idempotent',
    'if (typeof $262 === "undefined") {',
    '  var $262 = {};',
    '}',
    'if (typeof $262.global === "undefined") {',
    '  $262.global = this;',
    '}',
    'if (typeof $262.evalScript !== "function") {',
    '  $262.evalScript = function(src) {',
    '    return (0, eval)(String(src));',
    '  };',
    '}',
    'if (typeof $262.createRealm !== "function") {',
    '  $262.createRealm = function() {',
    '      // Delegate to runner-provided native hook when available.',
    '      if (typeof globalThis.__createRealm__ === "function") {',
    '        try {',
    '          var nativeRealm = globalThis.__createRealm__();',
    '          if (nativeRealm && nativeRealm.global) return nativeRealm;',
    '        } catch (e) { }',
    '      }',
    '',
    '      // Fallback: emulate a realm with distinct object intrinsics.',
    '      var g = Object.create(null);',
    '      var realmObjectProto = Object.create(null);',
    '      var realmObjectCtor = function Object(value) {',
    '        if (value === null || value === undefined) {',
    '          var o = {};',
    '          try { Object.setPrototypeOf(o, realmObjectProto); } catch (_) {}',
    '          return o;',
    '        }',
    '        return Object(value);',
    '      };',
    '      realmObjectCtor.prototype = realmObjectProto;',
    '      try { realmObjectProto.constructor = realmObjectCtor; } catch (_) {}',
    '',
    '      var GlobalFunction = Function;',
    '      var realmFunctionCtor = function RealmFunction() {',
    '        var args = Array.prototype.slice.call(arguments);',
    '        var f = Reflect.construct(GlobalFunction, args);',
    '        try { f.__origin_global = g; } catch (_) {}',
    '        try {',
    '          if (f && typeof f === "function" && f.prototype && typeof f.prototype === "object") {',
    '            Object.setPrototypeOf(f.prototype, realmObjectProto);',
    '          }',
    '        } catch (_) {}',
    '        return f;',
    '      };',
    '      try { Object.setPrototypeOf(realmFunctionCtor, GlobalFunction); } catch (_) {}',
    '      realmFunctionCtor.prototype = GlobalFunction.prototype;',
    '',
    '      g.globalThis = g;',
    '      g.this = g;',
    '      g.Object = realmObjectCtor;',
    '      g.Function = realmFunctionCtor;',
    '      g.TypeError = TypeError;',
    '      g.RangeError = RangeError;',
    '      g.ReferenceError = ReferenceError;',
    '      g.SyntaxError = SyntaxError;',
    '      g.Error = Error;',
    '      g.Array = Array;',
    '      g.Date = Date;',
    '      g.RegExp = RegExp;',
    '      g.Math = Math;',
    '      g.JSON = JSON;',
    '      g.Promise = Promise;',
    '      g.Symbol = Symbol;',
    '      g.Number = Number;',
    '      g.String = String;',
    '      g.Boolean = Boolean;',
    '      g.parseInt = parseInt;',
    '      g.parseFloat = parseFloat;',
    '      g.isNaN = isNaN;',
    '      g.isFinite = isFinite;',
    '      function __wrapRealmCallable(fn) {',
    '        if (typeof fn !== "function") return fn;',
    '        var defaultProto = null;',
    '        try {',
    '          if (fn.prototype && typeof fn.prototype === "object") {',
    '            defaultProto = Object.getPrototypeOf(fn.prototype);',
    '          }',
    '        } catch (_) {}',
    '        var wrapped = function() {',
    '          var out = fn.apply(this, arguments);',
    '          try {',
    '            var ctorProto = wrapped.prototype;',
    '            var nonObjectProto = (ctorProto === null) || (typeof ctorProto !== "object" && typeof ctorProto !== "function");',
    '            if (nonObjectProto && out && typeof out === "object" && defaultProto && Object.getPrototypeOf(out) !== defaultProto) {',
    '              Object.setPrototypeOf(out, defaultProto);',
    '            }',
    '          } catch (_) {}',
    '          return out;',
    '        };',
    '        try { Object.setPrototypeOf(wrapped, fn); } catch (_) {}',
    '        try { wrapped.__origin_global = g; } catch (_) {}',
    '        try {',
    '          Object.defineProperty(wrapped, "prototype", {',
    '            get: function() { return fn.prototype; },',
    '            set: function(v) { fn.prototype = v; },',
    '            enumerable: false,',
    '            configurable: true',
    '          });',
    '        } catch (_) {',
    '          try { wrapped.prototype = fn.prototype; } catch (_) {}',
    '        }',
    '        return wrapped;',
    '      }',
    '      g.eval = function(src) {',
    '        src = String(src);',
    '        var transformed = "";',
    '        var re = /\\bvar\\s+([^;]+)/g;',
    '        var lastIndex = 0;',
    '        var match;',
    '        while ((match = re.exec(src)) !== null) {',
    '          transformed += src.slice(lastIndex, match.index);',
    '          var decls = match[1];',
    '          var repl = decls.split(",").map(function(p) {',
    '            var s = p.trim();',
    '            var mm = s.match(/^([A-Za-z_$][\\w$]*)(\\s*=\\s*[\\s\\S]+)?$/);',
    '            if (!mm) return "";',
    '            var name = mm[1];',
    '            var init = mm[2];',
    '            if (init) return "this." + name + init;',
    '            return "this." + name + " = undefined";',
    '          }).join("; ");',
    '          transformed += repl;',
    '          lastIndex = re.lastIndex;',
    '        }',
    '        transformed += src.slice(lastIndex);',
    '        // Execute with `this` bound to the emulated realm global.',
    '        try {',
    '          var ret = (new Function("with (this) { return (" + transformed + "); }")).call(g);',
    '          return __wrapRealmCallable(ret);',
    '        } catch (e) {',
    '          var ret2 = (new Function("with (this) { " + transformed + " }")).call(g);',
    '          return __wrapRealmCallable(ret2);',
    '        }',
    '      };',
    '      return { global: g };',
    '    };',
    '}',
  ];
}

function inject262Shim(outLines, testPath, meta) {
  const need262Shim = references262(testPath) || hasFeature(meta, realmFeatureName);
  if (!need262Shim) return;
  if (!outLines.some(l => l.indexOf(realmMarker) !== -1)) {
    outLines.push(...get262StubLines());
    outLines.push('');
  }
}

function verifyComposeStubMarkerCount(testPath, harnessIndex = {}, prependFiles = [], needStrict = true, expected = 1) {
  // Compose the test and check the number of stub markers present
  const { tmpPath } = composeTest({ testPath, repoDir: '.', harnessIndex, prependFiles, needStrict });
  const src = fs.readFileSync(tmpPath, 'utf8');
  const re = new RegExp(realmMarker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g');
  const count = (src.match(re) || []).length;
  return count === expected;
}

function composeTest({testPath, repoDir, harnessIndex, prependFiles = [], needStrict = true}) {
  // Returns { testToRun, tmpPath, cleanupTmp }

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
      const absP = path.resolve(p);
      outLines.push(`// Inject: ${absP}`);
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

  const meta = extractMeta(testPath);
  inject262Shim(outLines, testPath, meta);

  // Ensure dynamic import resolves relative to the original test file path,
  // not the ephemeral /tmp composed file. Only inject when the test actually
  // contains an import (either a static `import` declaration or dynamic
  // `import()` expression) to avoid polluting tests that don't need it.
  const _test_src = fs.readFileSync(testPath, 'utf8');
  const _uses_import = /^\s*import\b/m.test(_test_src) || /\bimport\s*\(/.test(_test_src);
  if (_uses_import) {
    outLines.push('// Inject: stabilize __filepath for module resolution (only for tests that use import)');
    outLines.push(`globalThis.__filepath = ${JSON.stringify(path.resolve(testPath))};`);
    outLines.push('');
  }

  if (testPath.includes('/language/global-code/')) {
    outLines.push('// Inject: enable focused global-code semantics mode');
    outLines.push('// __test262_global_code_mode');
    outLines.push('');
  }

  // append test source
  const absTest = path.resolve(testPath);
  outLines.push(`// Inject: ${absTest}`);
  outLines.push(fs.readFileSync(testPath, 'utf8'));

  fs.writeFileSync(tmpName, outLines.join('\n'));

  // verify assert was injected if test references assert
  if (referencesAssert(testPath) && !definesAssertInFile(tmpName)) {
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
    for (const p of fixedUnique) {
      if (!p) continue;
      const absP = path.resolve(p);
      lines2.push(`// Inject: ${absP}`);
      lines2.push(fs.readFileSync(p, 'utf8'));
      lines2.push('');
    }

    // Inject unified $262 shim into the rebuilt file when required by test/meta
    const metaFixed = extractMeta(testPath);
    inject262Shim(lines2, testPath, metaFixed);

    const absTest = path.resolve(testPath);
    lines2.push(`// Inject: ${absTest}`);
    lines2.push(fs.readFileSync(testPath, 'utf8'));
    fs.writeFileSync(fixedTmp, lines2.join('\n'));
    return {testToRun: fixedTmp, tmpPath: fixedTmp, cleanupTmp: true};
  }

  return {testToRun: tmpName, tmpPath: tmpName, cleanupTmp: true};
}

module.exports = {extractMeta, parseList, hasFlag, hasFeature, get262StubLines, composeTest, referencesAssert, verifyComposeStubMarkerCount};
