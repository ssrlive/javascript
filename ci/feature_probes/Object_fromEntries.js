// feature probe for 'Object.fromEntries'
try {
  if (typeof Object.fromEntries !== 'function') throw new Error('fromEntries missing');
  var o = Object.fromEntries([['a',1],['b',2]]);
  if (o.a !== 1 || o.b !== 2) throw new Error('fromEntries wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
