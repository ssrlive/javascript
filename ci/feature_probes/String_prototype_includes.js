// feature probe for 'String.prototype.includes'
try {
  if (typeof ''.includes !== 'function') throw new Error('includes missing');
  if (!'hello'.includes('ell'))thrownewError('includeswrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
