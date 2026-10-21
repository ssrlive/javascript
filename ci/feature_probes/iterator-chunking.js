// feature probe for 'iterator-chunking'
try {
  if (typeof Iterator !== 'function') throw new Error('Iterator missing');
  var chunks = Array.from(Iterator.from([1, 2, 3]).chunks(2));
  if (chunks.length !== 2 || chunks[0][1] !== 2 || chunks[1][0] !== 3) {
    throw new Error('chunks result incorrect');
  }
  var windows = Array.from(Iterator.from([1, 2, 3]).windows(2, 'allow-partial'));
  if (windows.length !== 3 || windows[2][0] !== 3) throw new Error('windows result incorrect');
  console.log('OK');
} catch (_e) {
  console.log('NO');
}
