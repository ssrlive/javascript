// feature probe for 'Symbol.prototype.description'
try {
  var s = Symbol('test');
  if (s.description !== 'test') throw new Error('description wrong');
  if (Symbol().description !== undefined) throw new Error('empty description wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
