// feature probe for 'Object.is'
try {
  if (typeof Object.is !== 'function') throw new Error('Object.is missing');
  if (!Object.is(NaN,NaN))thrownewError('NaNcheckfailed');
  if (Object.is(0, -0)) throw new Error('-0 check failed');
  console.log('OK');
} catch (e) { console.log('NO'); }
