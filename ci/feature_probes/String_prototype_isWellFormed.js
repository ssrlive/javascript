// feature probe for 'String.prototype.isWellFormed'
try {
  if (typeof ''.isWellFormed !== 'function') throw new Error('isWellFormed missing');
  if (!'abc'.isWellFormed()) throw new Error('isWellFormed wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
