try {
  // Use eval to ensure syntax errors for unsupported syntax are caught at runtime
  eval("class C { #m() {} }");
  console.log('OK');
} catch (e) {
  console.log('NO');
}
