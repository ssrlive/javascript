// feature probe for 'error-cause'
try {
  const err = new Error('x', { cause: 1 });
  if (!err || err.cause !== 1) throw new Error('Error cause unsupported');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
