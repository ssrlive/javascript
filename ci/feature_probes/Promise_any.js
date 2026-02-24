// feature probe for 'Promise.any'
try {
  if (typeof Promise.any !== 'function') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
