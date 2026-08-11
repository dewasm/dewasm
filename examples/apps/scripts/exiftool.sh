#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# ExifTool on zeroperl: 6over3/exiftool's build pipeline flattens ExifTool
# 13.x's CLI driver into a single ~100 KB script (perltidy-minified, the two
# $SIG{INT}/$SIG{CONT} handlers stripped), checked into 6over3/exiftool as src/exiftool (no extension).
# It is the driver only: its `use Image::ExifTool`
# resolves in-guest from the module tree embedded in zeroperl.wasm's SFS blob
# (Phil Harvey's pure-Perl Image::ExifTool from CPAN), so no module tree is fetched or preopened here.
# We fetch that flattened script from a pinned commit and install it as a non-wasm cache tree (cache/exiftool-lib/, the -lib convention cpython/cruby use for their stdlib trees).
# There is no new wasm artifact: the exiftool e2e case preopens this script next to the input image and drives it through the same embedding C API on the already-cached cache/zeroperl.wasm, so the convert suite gains no row.
#
# Licensing: ExifTool is dual-licensed Artistic/GPL ("same terms as Perl itself"); the 6over3/exiftool wrapper repo that hosts the flattened script is
# Apache-2.0.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Pinned commit of the src/exiftool path (resolved via the GitHub commits API, audited 2026-08-01) and the sha256 of the flattened script's bytes at it.
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
