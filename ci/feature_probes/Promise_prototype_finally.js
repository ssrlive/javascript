// feature probe for 'Promise.prototype.finally'
try {
  if (typeof Promise.prototype.finally !== 'function') throw new Error('missing');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
