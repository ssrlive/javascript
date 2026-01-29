try {
  function f() {}
  // accessing 'caller' property should be legal for engines that expose it
  var _ = f.caller;
  console.log('OK');
} catch (e) {
  console.log('NO');
}
