// feature probe for 'AggregateError'
try {
  if (typeof AggregateError !== 'function') throw new Error('AggregateError missing');
  const e = new AggregateError([new Error('x')], 'msg');
  if (!(e instanceof AggregateError)) throw new Error('AggregateError instance failed');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
