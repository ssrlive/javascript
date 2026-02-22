// feature probe for 'joint-iteration'
// Tests Iterator.zip / Iterator.zipKeyed (TC39 joint iteration proposal)
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  // The proposal exposes Iterator.zip and/or Iterator.zipKeyed
  if (typeof Iterator.zip !== 'function' && typeof Iterator.zipKeyed !== 'function') {
    throw new Error('Iterator.zip/zipKeyed missing');
  }
  console.log('OK');
} catch (e) {
  console.log('NO:', e.message);
}
