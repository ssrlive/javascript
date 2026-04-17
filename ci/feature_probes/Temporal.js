// feature probe for 'Temporal'
try {
  if (typeof Temporal !== 'object') throw new Error('Temporal missing');
  if (typeof Temporal.Instant !== 'function') throw new Error('Temporal.Instant missing');
  if (typeof Temporal.PlainDate !== 'function') throw new Error('Temporal.PlainDate missing');
  if (typeof Temporal.Duration !== 'function') throw new Error('Temporal.Duration missing');
  if (typeof Temporal.Now !== 'object') throw new Error('Temporal.Now missing');
  if (typeof Temporal.Now.instant !== 'function') throw new Error('Temporal.Now.instant missing');

  var instant = Temporal.Instant.from('1970-01-01T00:00Z');
  if (instant.epochMilliseconds !== 0) throw new Error('Instant epochMilliseconds broken');

  var date = new Temporal.PlainDate(2025, 3, 4);
  if (date.year !== 2025 || date.month !== 3 || date.day !== 4) {
    throw new Error('PlainDate fields broken');
  }

  var duration = new Temporal.Duration(1, 2, 3, 4);
  if (duration.years !== 1 || duration.months !== 2 || duration.weeks !== 3 || duration.days !== 4) {
    throw new Error('Duration fields broken');
  }

  console.log('OK');
} catch (e) {
  console.log('NO:', e && e.message ? e.message : String(e));
}
