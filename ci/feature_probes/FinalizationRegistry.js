// feature probe for 'FinalizationRegistry'
try {
  if (typeof FinalizationRegistry !== 'function') throw new Error('FinalizationRegistry missing');
  const fr = new FinalizationRegistry(function() {});
  if (!fr) throw new Error('FinalizationRegistry failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
