// feature probe for 'String.prototype.trimEnd'
try {
  if (typeof ''.trimEnd !== 'function') throw new Error('trimEnd missing');
  if ('  x  '.trimEnd() !== '  x') throw new Error('trimEnd wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
