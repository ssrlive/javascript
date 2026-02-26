// feature probe for 'RegExp.escape'
try {
  if (typeof RegExp.escape === 'function' && RegExp.escape('.') === '\\.') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
