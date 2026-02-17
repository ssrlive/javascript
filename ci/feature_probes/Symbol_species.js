// feature probe for 'Symbol.species'
try {
  if (typeof Symbol !== 'function') throw new Error('Symbol missing');
  if (typeof Symbol.species === 'undefined') throw new Error('Symbol.species missing');
  const desc = Object.getOwnPropertyDescriptor(Symbol, 'species');
  if (!desc || typeof desc.get !== 'function') throw new Error('Symbol.species descriptor missing');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
