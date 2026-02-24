// feature probe for 'promise-try' (Promise.try)
try {
  if (typeof Promise.try !== 'function') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
