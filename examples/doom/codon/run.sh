#!/usr/bin/env bash
# Build (if needed) and run the DOOM frontend, forwarding any arguments
# (e.g. `./run.sh --smoke` for the headless self-check).
# A codon-built binary links Codon's runtime dylibs (libcodonrt, libomp) relative to the toolchain, so its lib/codon directory has to be on the loader path (docs/backends/codon.md).
set -euo pipefail
cd "$(dirname "$0")"

codon=${DEWASM_CODON:-codon}

./build.sh

codon_path=$(command -v "$codon")
while [[ -L $codon_path ]]; do
  link=$(readlink "$codon_path")
  case $link in
    /*) codon_path=$link ;;
    *) codon_path=$(dirname "$codon_path")/$link ;;
  esac
done
lib_dir=$(cd "$(dirname "$codon_path")/../lib/codon" && pwd)

if [[ $(uname -s) == Darwin ]]; then
  export DYLD_LIBRARY_PATH=$lib_dir${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}
else
  export LD_LIBRARY_PATH=$lib_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
fi

exec ./doom "$@"
