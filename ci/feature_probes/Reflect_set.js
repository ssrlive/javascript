// feature probe for 'Reflect.set'
try {
  if (typeof Reflect !== 'object' || typeof Reflect.set !== 'function') throw new Error('missing');
  var o = {};
  Reflect.set(o, 'x', 42);
  if (o.x !== 42) throw new Error('wrong value');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
