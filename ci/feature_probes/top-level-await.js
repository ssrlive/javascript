// feature probe for 'top-level-await'
try {
  // Use eval so the file itself can be parsed even if TLA is unsupported.
  (0, eval)('await Promise.resolve(1);');
  console.log('OK');
} catch (e) {
  console.log('NO');
}
