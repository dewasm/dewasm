#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# ExifTool on `zeroperl`: the build of `6over3/exiftool` flattens the CLI driver of ExifTool 13.x.
# The result is a single ~100 KB script, made smaller by `perltidy`, with the two handlers stripped.
# The stripped handlers are $SIG{INT}/$SIG{CONT}.
# It is checked into `6over3/exiftool` as `src/exiftool` (no extension).
# It is the driver only: its `use Image::ExifTool` resolves in-guest.
# The module tree it resolves from is embedded in the SFS blob of `zeroperl.wasm`.
# That tree is Phil Harvey's pure-Perl Image::ExifTool from CPAN.
# So no module tree is fetched or preopened here.
# We fetch that flattened script from a fixed commit and install it as a non-wasm cache tree.
# It lands at `cache/exiftool-lib/`.
# That is the `-lib` convention CPython and CRuby use for their standard library trees.
# There is no new wasm artifact.
# The `exiftool` e2e case preopens this script next to the input image.
# It drives the script through the same embedding C API on the already-cached cache/zeroperl.wasm.
# So the convert suite gains no row.
#
# Licensing: ExifTool is dual-licensed Artistic/GPL ("same terms as Perl itself").
# The `6over3/exiftool` wrapper repository that hosts the flattened script is Apache-2.0.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# The fixed commit of the `src/exiftool` path, and the SHA-256 of the flattened script at it.
# The commit was resolved via the GitHub commits API, audited 2026-08-01.
EXIFTOOL_COMMIT="dbc8c7ba9dacc44429f3d083618ade58725297fe"
EXIFTOOL_URL="https://raw.githubusercontent.com/6over3/exiftool/$EXIFTOOL_COMMIT/src/exiftool"
EXIFTOOL_SHA256="d0a14066bd30b4c4076f4c8a76bb5049c0c7b4c331bd0a8852576e34f4007df2"

exiftool_stamp="cache/exiftool.src-sha256"
exiftool_out="cache/exiftool-lib/exiftool"

if is_cached "$exiftool_stamp" "$EXIFTOOL_SHA256" "$exiftool_out"; then
  echo "exiftool: cached"
  exit 0
fi

echo "exiftool: fetching $EXIFTOOL_URL"
mkdir -p cache/exiftool-lib
fetch_verified "$EXIFTOOL_URL" "$EXIFTOOL_SHA256" "$exiftool_out"
write_stamp "$exiftool_stamp" "$EXIFTOOL_SHA256"
echo "exiftool: -> $exiftool_out"
