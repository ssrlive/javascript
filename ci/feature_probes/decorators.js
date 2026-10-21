"use strict";

try {
  let called = false;
  function d(value, context) {
    called = context.kind === 'class' &&
      context.name === 'C' &&
      typeof context.addInitializer === 'function';
    context.addInitializer(function () { this.initialized = true; });
    return value;
  }
  @d
  class C {}
  if (!called || typeof C !== 'function' || C.initialized !== true) {
    throw new Error('class decorator was not applied with its context');
  }
  function methodDecorator(value, context) {
    if (context.kind !== 'method' || context.name !== 'method' ||
        context.static || !context.access.has({ method: value }) ||
        context.access.get({ method: value }) !== value) {
      throw new Error('method decorator context is invalid');
    }
    context.addInitializer(function () { this.ready = true; });
    return function () { return value.call(this) + 1; };
  }
  function fieldDecorator(value, context) {
    if (context.kind !== 'field' || context.name !== 'field') {
      throw new Error('field decorator context is invalid');
    }
    return function (initialValue) { return initialValue + 1; };
  }
  class M {
    @fieldDecorator
    field = 1;
    @methodDecorator
    method() { return 1; }
  }
  const instance = new M();
  if (instance.method() !== 2 || instance.field !== 2 || instance.ready !== true) {
    throw new Error('element decorator was not applied');
  }
  console.log('OK');
} catch (e) {
  console.log('NO');
}
