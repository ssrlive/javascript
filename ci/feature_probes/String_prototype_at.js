// feature probe for 'String.prototype.at'
try {
  if (typeof ''.at !== 'function') throw new Error('at missing');
  if ('abc'.at(-1) !== 'c') throw new Error('at wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
