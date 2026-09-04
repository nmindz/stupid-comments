#!/usr/bin/env bash
# Fails fast: a partial deploy is worse than none.
set -euo pipefail

readonly MARKER="literal # inside a string"

render() {
  cat <<'TEMPLATE'
# This heredoc body is data, not commentary. Reading it as comments
# would trip the ratio rule on a script that carries almost none.
# A third line, to make the misreading unmistakable.
# A fourth.
# A fifth, which would also trip the length rule.
TEMPLATE
}

main() {
  render | grep '#' || true
  echo "$MARKER"
}

main "$@"
