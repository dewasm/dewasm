#!/usr/bin/env bash
# Build the test extension and run the host runtime's checks against it.
set -euo pipefail
cd "$(dirname "$0")"
out=cache/demo
../bin/compile-ext demo DemoExt Init_demo "$out" demo_ext.c
ruby test.rb
