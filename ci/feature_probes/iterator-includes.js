// feature probe for 'iterator-includes'
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  if (!Iterator.from([1, 2, 3]).includes(2)) throw new Error('match missing');
  if (Iterator.from([1, 2, 3]).includes(4)) throw new Error('unexpected match');
  console.log('OK');
} catch (_e) {
  console.log('NO');
}
