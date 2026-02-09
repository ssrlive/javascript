// Probe: Proxy
// Run as a normal script; if Proxy support exists print OK
try {
  const p = new Proxy({}, {});
  // Basic behaviour check
  if (typeof p === 'object') console.log('OK');
  else throw new Error('unexpected proxy type');
} catch (e) {
  // ensure non-OK exit
  throw e;
}
