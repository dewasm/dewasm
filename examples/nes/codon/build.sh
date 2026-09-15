#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and compile it together with the terminal frontend into one native binary.
# Codon has no import path for a sibling source file, so the two are linked by concatenation into nes_app.codon, the same way the backend's own test and benchmark runners do; both generated files are gitignored, so this step has to run before ./run.sh from a clean checkout.
# The default is a -release build: it costs about 1.8x the compile time of a debug build and runs the emulator about 8.5x faster (README has the numbers). CODON_BUILD=debug picks the other one.
set -euo pipefail
cd "$(dirname "$0")"

codon=${DEWASM_CODON:-codon}
build_flags=-release
if [[ ${CODON_BUILD:-release} == debug ]]; then
  build_flags=
fi

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm -- \
    examples/apps/cache/nes.wasm \
    --target codon --mode library --module-name Nes \
    -o examples/nes/codon/nes_gen.codon
)

# The build flags go into the source as a comment, so that switching build mode shows up as a source change and one comparison decides whether the compile can be skipped.
{
  printf '# codon build flags: %s\n' "${build_flags:-none (debug)}"
  cat nes_gen.codon main.codon
} >nes_app.codon.tmp

if [[ -x nes && -f nes_app.codon ]] && cmp -s nes_app.codon.tmp nes_app.codon; then
  rm -f nes_app.codon.tmp
  echo "up to date: $(pwd)/nes (run with ./run.sh)"
  exit 0
fi
mv nes_app.codon.tmp nes_app.codon

# Removed first so that a failed compile leaves no binary behind for the check above to accept.
rm -f nes
echo "compiling nes_app.codon ($(wc -l <nes_app.codon | tr -d ' ') lines)"
"$codon" build $build_flags -o nes nes_app.codon

echo "built $(pwd)/nes (run with ./run.sh)"
