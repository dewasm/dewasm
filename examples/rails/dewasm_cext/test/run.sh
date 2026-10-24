#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
out=cache/demo
../bin/compile-ext demo DemoExt Init_demo "$out" demo_ext.c
ruby test.rb
