try {
  // Ensure U+180E in a comment parses without error (Mongolian Vowel Separator)
  // Using escape to keep file encoding safe: the engine must accept the character in comments
  eval("/*\u180e*/; void 0;");
  console.log('OK');
} catch (e) {
  console.log('NO');
}
