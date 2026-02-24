// feature probe for 'await-dictionary' (Promise.allKeyed)
try {
  if (typeof Promise.allKeyed !== 'function') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
