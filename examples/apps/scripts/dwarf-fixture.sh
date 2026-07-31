#!/usr/bin/env bash
# dwarf-fixture: a first-party C fixture built WITH DWARF (`-g`) so the
# --dwarf-line source back-mapping test has a module carrying `.debug_line`
# (ADR-38). The source is committed (src/dwarf_fixture.c, first-party — ADR-9);
# unlike the other apps there is nothing to download, so the "pin" is the
# sha256 of that source file: editing it rebuilds. Built at -O1 (not -O0) so the
# fixture exercises the folded-expression marker path the core test calibrates.
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

DWARF_FIXTURE_SRC="src/dwarf_fixture.c"
DWARF_FIXTURE_SHA256="$(shasum -a 256 "$DWARF_FIXTURE_SRC" | cut -d' ' -f1)"

dwarf_stamp="cache/dwarf-fixture.src-sha256"
if is_cached "$dwarf_stamp" "$DWARF_FIXTURE_SHA256" cache/dwarf-fixture.wasm; then
  echo "dwarf-fixture: cached"
  exit 0
fi

require_tool dwarf-fixture zig "install zig (e.g. brew install zig) to build the DWARF fixture"
echo "dwarf-fixture: building dwarf-fixture.wasm (zig cc -g -O1)"
zig cc -target wasm32-wasi -g -O1 -o cache/dwarf-fixture.wasm "$DWARF_FIXTURE_SRC"
write_stamp "$dwarf_stamp" "$DWARF_FIXTURE_SHA256"
echo "dwarf-fixture: -> cache/dwarf-fixture.wasm"
