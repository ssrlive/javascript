"use strict";

try {
  function d(v) { return v; }
  @d
  class C {}
  console.log('OK');
} catch (e) {
  console.log('NO');
}
