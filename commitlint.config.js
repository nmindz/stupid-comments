/** Conventional Commits, the vocabulary semantic-release derives versions from. */
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Release commits carry generated notes that legitimately run long.
    'body-max-line-length': [0, 'always'],
    'footer-max-line-length': [0, 'always'],
  },
}
