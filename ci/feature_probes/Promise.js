// feature probe for 'Promise'
try {
  if (typeof Promise !== 'function') throw new Error('Promise missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
