// feature probe for 'string-trimming'
try {
  if (typeof ''.trimStart !== 'function') throw new Error('trimStart missing');
  if (typeof ''.trimEnd !== 'function') throw new Error('trimEnd missing');
  if ('  x  '.trimStart() !== 'x  ') throw new Error('trimStart wrong');
  if ('  x  '.trimEnd() !== '  x') throw new Error('trimEnd wrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
