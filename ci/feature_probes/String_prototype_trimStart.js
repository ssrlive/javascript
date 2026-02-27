// feature probe for 'String.prototype.trimStart'
try {
  if (typeof ''.trimStart !== 'function') throw new Error('trimStart missing');
  if ('  x  '.trimStart() !== 'x  ') throw new Error('trimStart wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
