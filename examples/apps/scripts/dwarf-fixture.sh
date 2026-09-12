#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# dwarf-fixture: a first-party C fixture built WITH DWARF (`-g`) so the
# --dwarf-line source back-mapping test has a module carrying `.debug_line`.
# The source is committed (src/dwarf_fixture.c, first-party); unlike the other apps there is nothing to download, so the "pin" is the sha256 of that source file plus the toolchain token: editing either rebuilds.
# Built at -O1 (not -O0) so the fixture exercises the folded-expression marker path the core test calibrates.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

DWARF_FIXTURE_SRC="src/dwarf_fixture.c"
DWARF_FIXTURE_KEY="$(shasum -a 256 "$DWARF_FIXTURE_SRC" | cut -d' ' -f1) $(wasi_sdk_stamp)"

dwarf_stamp="cache/dwarf-fixture.src-sha256"
if is_cached "$dwarf_stamp" "$DWARF_FIXTURE_KEY" cache/dwarf-fixture.wasm; then
  echo "dwarf-fixture: cached"
  exit 0
fi

require_wasi_sdk dwarf-fixture

echo "dwarf-fixture: building dwarf-fixture.wasm (wasi-sdk clang -g -O1)"
wasi_sdk_clang -g -O1 -o cache/dwarf-fixture.wasm "$DWARF_FIXTURE_SRC"

write_stamp "$dwarf_stamp" "$DWARF_FIXTURE_KEY"
echo "dwarf-fixture: -> cache/dwarf-fixture.wasm"
