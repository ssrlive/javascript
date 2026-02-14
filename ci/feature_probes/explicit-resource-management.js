// module
// Conservative probe for `explicit-resource-management`.
// Only report support when both `using` and `await using` work and
// disposal side effects are observable.

try {
  const events = [];

  {
    using r = {
      [Symbol.dispose]() {
        events.push('dispose');
      },
    };
  }

  {
    await using ar = {
      async [Symbol.asyncDispose]() {
        events.push('asyncDispose');
      },
    };
  }

  if (events[0] === 'dispose' && events[1] === 'asyncDispose') {
    console.log('OK');
  } else {
    console.log('UNSUPPORTED 1');
  }
} catch (_e) {
  console.log('UNSUPPORTED 2');
}
