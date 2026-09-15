#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and compile it together with the terminal frontend into one native binary.
# Codon has no import path for a sibling source file, so the two are linked by concatenation into doom_app.codon, the same way the backend's own test and benchmark runners do; both generated files are gitignored, so this step has to run before ./run.sh from a clean checkout.
# The default is a debug build, the cheaper of the two, and even that has not been run to completion on this source (README.md is explicit about what was and was not measured). CODON_BUILD=release selects the optimized compile, which costs considerably more again.
set -euo pipefail
cd "$(dirname "$0")"

codon=${DEWASM_CODON:-codon}
build_flags=
if [[ ${CODON_BUILD:-debug} == release ]]; then
  build_flags=-release
fi

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm -- \
    examples/apps/cache/doom.wasm \
    --target codon --mode library --module-name Doom \
    -o examples/doom/codon/doom_gen.codon
)

# The build flags go into the source as a comment, so that switching build mode shows up as a source change and one comparison decides whether the compile can be skipped.
{
  printf '# codon build flags: %s\n' "${build_flags:-none (debug)}"
  cat doom_gen.codon main.codon
} >doom_app.codon.tmp

if [[ -x doom && -f doom_app.codon ]] && cmp -s doom_app.codon.tmp doom_app.codon; then
  rm -f doom_app.codon.tmp
  echo "up to date: $(pwd)/doom (run with ./run.sh)"
  exit 0
fi
mv doom_app.codon.tmp doom_app.codon

# Removed first so that a failed compile leaves no binary behind for the check above to accept.
rm -f doom
cat >&2 <<'WARNING'
=========================================================================
 compiling the DOOM library with codon: expect to wait HOURS. A debug
 compile of this source ran for over an hour on an Apple Silicon laptop
 without finishing, and `-release` costs more again (see README.md).
 The result is cached: a later build whose concatenated source is
 unchanged skips the compile entirely.
=========================================================================
WARNING
echo "compiling doom_app.codon ($(wc -l <doom_app.codon | tr -d ' ') lines)"
"$codon" build $build_flags -o doom doom_app.codon

echo "built $(pwd)/doom (run with ./run.sh)"
