// Smoke test for AsyncGenerator.prototype.throw and return behavior
async function* ag() {
  try {
    const a = yield 1;
    console.log('a received', a);
    const b = yield a + 1;
    console.log('b received', b);
  } catch (e) {
    console.log('caught', e);
    return 123;
  }
}

const g = ag();
const p1 = g.next();
if (typeof p1.then === 'function') p1.then(v => console.log('p1->', v));
// Throw into the suspended generator
const p2 = g.throw(42);
if (typeof p2.then === 'function') p2.then(v => console.log('p2->', v), e => console.log('p2 reject', e));
// Calling return should complete the generator
const p3 = g.return(99);
if (typeof p3.then === 'function') p3.then(v => console.log('p3->', v), e => console.log('p3 reject', e));
