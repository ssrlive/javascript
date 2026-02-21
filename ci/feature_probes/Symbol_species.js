// feature probe for 'Symbol.species'
try {
  if (typeof Symbol !== 'function') throw new Error('Symbol missing');
  if (typeof Symbol.species !== 'symbol') throw new Error('Symbol.species missing');
  const desc = Object.getOwnPropertyDescriptor(Symbol, 'species');
  if (!desc || desc.writable || desc.enumerable || desc.configurable) throw new Error('Symbol.species descriptor incorrect');
  console.log('OK');
} catch (e) {
  console.log('NO: ' + e.message);
}
