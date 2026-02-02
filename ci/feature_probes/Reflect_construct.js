// Probe for Reflect.construct support
try {
  if (typeof Reflect === 'object' && Reflect && typeof Reflect.construct === 'function') {
    console.log('OK');
  } else {
    console.log('NO');
  }
} catch (e) {
  console.log('NO');
}
