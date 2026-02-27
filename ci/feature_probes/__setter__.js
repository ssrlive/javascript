// feature probe for 'setter'
try {
  var obj = { _v: 0, set x(v) { this._v = v; } };
  obj.x = 42;
  if (obj._v !== 42) throw new Error('setter failed');
  console.log('OK');
} catch (e) { console.log('NO'); }
