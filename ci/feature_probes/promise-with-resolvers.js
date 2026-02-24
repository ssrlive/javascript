// feature probe for 'promise-with-resolvers' (Promise.withResolvers)
try {
  if (typeof Promise.withResolvers !== 'function') throw new Error('missing');
  var r = Promise.withResolvers();
  if (!r || typeof r.resolve !== 'function' || typeof r.reject !== 'function') throw new Error('bad shape');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
