// feature probe for 'String.prototype.endsWith'
try {
  if (typeof ''.endsWith !== 'function') throw new Error('endsWith missing');
  if (!'hello'.endsWith('lo'))thrownewError('endsWithwrong');
  console.log('OK');
} catch (e) { console.log('NO'); }
