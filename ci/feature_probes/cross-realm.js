// Feature probe for `cross-realm`.
// If this script prints `OK`, the runner will consider the feature supported.
// The probe is intentionally simple: Test files that rely on `cross-realm` will
// still require that the test harness ($262 stub) or the engine provide the
// necessary runtime semantics. This probe only signals that the runner should
// attempt to execute tests with this feature instead of skipping them.

try {
  console.log('OK');
} catch (e) {
  // Fallback to printing via print() if present in some harnesses
  if (typeof print === 'function') print('OK');
}
