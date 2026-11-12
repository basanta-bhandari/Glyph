#!/usr/bin/env bash
set -euo pipefail
glyph_project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec "$glyph_project_dir/dist/glyph-bootstrap" --kitty "$@"
