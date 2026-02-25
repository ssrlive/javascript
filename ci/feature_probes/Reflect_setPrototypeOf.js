// feature probe for 'Reflect.setPrototypeOf'
try {
  if (typeof Reflect !== 'object' || typeof Reflect.setPrototypeOf !== 'function') throw new Error('missing');
  var o = {};
  var p = { hello: 'world' };
  var result = Reflect.setPrototypeOf(o, p);
  if (result !== true) throw new Error('should return true');
  if (o.hello !== 'world') throw new Error('prototype not set');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
