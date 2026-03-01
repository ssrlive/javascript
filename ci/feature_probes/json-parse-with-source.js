// feature probe for 'json-parse-with-source'
try {
  var ok = false;
  JSON.parse('1', function(k, v, ctx) {
    if (typeof ctx === 'object' && ctx.source === '1') ok = true;
    return v;
  });
  if (!ok) throw new Error('fail');
  if (typeof JSON.rawJSON !== 'function') throw new Error('no rawJSON');
  if (typeof JSON.isRawJSON !== 'function') throw new Error('no isRawJSON');
  console.log('OK');
} catch (_) {
  console.log('NO');
}
