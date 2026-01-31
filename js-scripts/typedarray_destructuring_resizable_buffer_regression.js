// Regression test for destructuring of TypedArrays backed by resizable ArrayBuffer
// Ensures length-tracking views with offset throw TypeError when the view goes out-of-bounds
(function () {
  function CreateResizableArrayBuffer(initial, max) {
    try {
      return new ArrayBuffer(initial, { maxByteLength: max });
    } catch (e) {
      throw new Error('Resizable ArrayBuffer not supported: ' + e);
    }
  }

  const ctors = [
    Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
    Int32Array, Uint32Array, Float32Array, Float64Array
  ];

  for (let ctor of ctors) {
    const rab = CreateResizableArrayBuffer(4 * ctor.BYTES_PER_ELEMENT, 8 * ctor.BYTES_PER_ELEMENT);
    const fixedLength = new ctor(rab, 0, 4);
    const fixedLengthWithOffset = new ctor(rab, 2 * ctor.BYTES_PER_ELEMENT, 2);
    const lengthTracking = new ctor(rab, 0);
    const lengthTrackingWithOffset = new ctor(rab, 2 * ctor.BYTES_PER_ELEMENT);

    // Write some data
    let ta_write = new ctor(rab);
    for (let i = 0; i < 4; ++i) {
      ta_write[i] = i;
    }

    // Shrink so fixed-length views go out of bounds
    rab.resize(3 * ctor.BYTES_PER_ELEMENT);
    // fixedLength must throw
    let threw = false;
    try { let [a,b,c] = fixedLength; } catch (e) { if (!(e instanceof TypeError)) throw e; threw = true; }
    if (!threw) throw new Error('fixedLength did not throw after shrink to 3 for ' + ctor.name);

    threw = false;
    try { let [a,b,c] = fixedLengthWithOffset; } catch (e) { if (!(e instanceof TypeError)) throw e; threw = true; }
    if (!threw) throw new Error('fixedLengthWithOffset did not throw after shrink to 3 for ' + ctor.name);

    // Shrink so views with offset go out of bounds
    rab.resize(1 * ctor.BYTES_PER_ELEMENT);
    threw = false;
    try { let [a,b,c] = lengthTrackingWithOffset; } catch (e) { if (!(e instanceof TypeError)) throw e; threw = true; }
    if (!threw) throw new Error('lengthTrackingWithOffset did not throw after shrink to 1 for ' + ctor.name);

    // lengthTracking (no offset) should still be readable for available elements
    {
      let [a,b] = lengthTracking;
      // a should be 0 (if available) or undefined if not; don't need to assert strongly here
    }
  }

  return "OK";
})();