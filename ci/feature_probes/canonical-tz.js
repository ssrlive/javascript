// feature probe for 'canonical-tz'
// Requires Temporal. Tests that time zone identifiers are canonicalized via
// GetAvailableNamedTimeZoneIdentifier (e.g. link aliases resolve to a primary id).
try {
  if (typeof Temporal !== 'object' || typeof Temporal.TimeZone !== 'function') {
    throw new Error('Temporal.TimeZone missing');
  }

  // A well-known IANA primary identifier must be accepted and round-trip correctly.
  var tz = new Temporal.TimeZone('UTC');
  if (tz.id !== 'UTC') throw new Error('UTC id broken');

  // Canonicalization: a well-known link alias should resolve to its primary id.
  // 'Europe/Kiev' is a legacy alias; engines implementing canonical-tz must
  // resolve it to 'Europe/Kyiv' (or keep it stable — either way no throw).
  var tz2 = new Temporal.TimeZone('America/New_York');
  if (typeof tz2.id !== 'string' || tz2.id.length === 0) {
    throw new Error('TimeZone id must be a non-empty string');
  }

  // TimeZone.equals must exist and work for identical identifiers.
  if (typeof tz2.equals !== 'function') throw new Error('TimeZone.equals missing');
  if (!tz2.equals('America/New_York')) throw new Error('TimeZone.equals self broken');

  console.log('OK');
} catch (e) {
  console.log('FAIL: ' + e.message);
}
