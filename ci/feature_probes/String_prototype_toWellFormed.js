// feature probe for 'String.prototype.toWellFormed'
try {
  if (typeof ''.toWellFormed !== 'function') throw new Error('toWellFormed missing');
  console.log('OK');
} catch (e) { console.log('NO'); }
