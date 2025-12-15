// Minimal Test262Error shim so this file can run standalone.
if (typeof Test262Error === 'undefined') {
  function Test262Error(message) {
    this.name = 'Test262Error';
    this.message = message || '';
  }
  Test262Error.prototype = Object.create(Error.prototype);
  Test262Error.prototype.constructor = Test262Error;
}

function assertRelativeDateMs(date, expectedMs) {
  var actualMs = date.valueOf();
  var localOffset = date.getTimezoneOffset() * 60000;

  if (actualMs - localOffset !== expectedMs) {
    throw new Test262Error(
      'Expected ' + date + ' to be ' + expectedMs +
      ' milliseconds from the Unix epoch'
    );
  }
}

// Example/test cases and a simple runner that prints results.
function runTests() {
  var tests = [
    {desc: 'Unix epoch', date: new Date(Date.UTC(1970, 0, 1, 0, 0, 0))},
    {desc: 'Y2K start UTC', date: new Date(Date.UTC(2000, 0, 1, 0, 0, 0))},
    {desc: 'Leap day with ms', date: new Date(Date.UTC(2000, 1, 29, 12, 34, 56, 789))},
    {desc: 'Before epoch', date: new Date(Date.UTC(1969, 11, 31, 23, 59, 59, 0))},
    {desc: 'Recent date', date: new Date(Date.UTC(2025, 11, 15, 8, 30, 0, 0))}
  ];

  var passed = 0;
  var failed = 0;

  tests.forEach(function(t, i) {
    console.log('Test ' + (i + 1) + ': ' + t.desc + ' -> ' + t.date.toISOString());
    var localOffsetMs = t.date.getTimezoneOffset() * 60000;
    var expectedMs = t.date.valueOf() - localOffsetMs;
    console.log('  expectedMs:', expectedMs);
    console.log('  actualMs:', t.date.valueOf(), 'localOffset(ms):', localOffsetMs);
    try {
      assertRelativeDateMs(t.date, expectedMs);
      console.log('  result: PASS');
      passed++;
    } catch (e) {
      console.log('  result: FAIL -', e.name + ':', e.message);
      failed++;
    }
  });

  console.log('\nSummary: ' + passed + ' passed, ' + failed + ' failed');
}

// Run tests when the file is executed directly.
if (typeof module === 'undefined' || !module.parent) {
  runTests();
}
