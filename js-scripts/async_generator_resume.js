// Regression / smoke test for async generator resume/send-value semantics
function sleep(v) { return Promise.resolve(v); }

async function* ag() {
  const a = yield 1;
  console.log(a);
  const b = yield 2;
  console.log(b);
}

const g = ag();
const p1 = g.next();
console.log('p1 type', typeof p1);
try { console.log('p1.then type', typeof p1.then); } catch (e) { console.log('p1.then error', e); }
try { console.log('p1 keys', Object.keys(p1)); } catch (e) { console.log('p1 keys error', e); }
if (typeof p1.then === 'function') { p1.then(v => console.log("p1->", v)); } else { console.log('p1.then is not callable'); }
const p2 = g.next(42);
p2.then(v => console.log("p2->", v));
const p3 = g.next(99);
p3.then(v => console.log("p3->", v));
