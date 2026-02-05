function assert(condition, message) {
  if (!condition) {
    throw new Error(message || 'Assertion failed');
  }
}

const actual = [];
const expected = [
  'Promise: 6',
  'Promise: 5',
  'Await: 3',
  'Promise: 4',
  'Promise: 3',
  'Await: 2',
  'Promise: 2',
  'Promise: 1',
  'Await: 1',
  'Promise: 0'
];
const iterations = 3;

async function* naturalNumbers(start) {
  let current = start;
  while (current > 0) {
    yield Promise.resolve(current--);
  }
}

async function trigger() {
  for await (const num of naturalNumbers(iterations)) {
    actual.push('Await: ' + num);
  }
}

function countdown(counter) {
  actual.push('Promise: ' + counter);
  if (counter > 0) {
    return Promise.resolve(counter - 1).then(countdown);
  }
  return Promise.resolve();
}

const triggerPromise = trigger();
countdown(iterations * 2).then(() => {
  triggerPromise.then(() => {
    // console.log('expected', expected);
    // console.log('  actual', actual);
    assert(actual.length === expected.length, `Expected ${expected.length} entries, got ${actual.length}`);
    for (let i = 0; i < expected.length; i++) {
      assert(actual[i] === expected[i], `At index ${i}, expected "${expected[i]}", got "${actual[i]}"`);
    }
    // console.log('PASSED');
  });
});
