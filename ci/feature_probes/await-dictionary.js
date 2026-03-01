// feature probe for 'await-dictionary' (Promise.allKeyed)
try {
  if (typeof Promise.allKeyed !== 'function') throw new Error('Promise.allKeyed missing');
  console.log('OK');
} catch (e) {
  console.log('NO:', e.message);
}
