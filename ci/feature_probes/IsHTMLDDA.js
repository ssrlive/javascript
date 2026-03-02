// Feature probe for IsHTMLDDA
// __isHTMLDDA__ has [[IsHTMLDDA]] so typeof returns "undefined",
// but calling it returns null.
try {
  var result = __isHTMLDDA__();
  if (result === null) console.log("OK");
} catch (e) {
  // not supported
}
