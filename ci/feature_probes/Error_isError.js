// feature probe for 'Error.isError'
try {
  if (typeof Error !== 'function') throw new Error('Error missing');
  if (typeof Error.isError !== 'function') throw new Error('Error.isError missing');

  if (Error.isError(new Error('x')) !== true) throw new Error('Error instance should be true');
  if (Error.isError({ name: 'Error', message: 'x' }) !== false) throw new Error('fake error should be false');
  if (Error.isError(undefined) !== false) throw new Error('undefined should be false');

  console.log('OK');
} catch (_e) {
  console.log('NO');
}
