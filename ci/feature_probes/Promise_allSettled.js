// feature probe for 'Promise.allSettled'
try {
  if (typeof Promise.allSettled !== 'function') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
