# frozen_string_literal: true

# Run a benchmark module under wardite, the pure-Ruby wasm interpreter, with
# the same command line every other runner in the suite gets:
#
#   ruby benchmarks/drivers/wardite.rb <module.wasm> [guest-args...]
#
# The guest sees argv = [basename(module), *guest-args], matching what wasmtime
# passes, so the `<module> <iterations>` contract holds identically here.
# (wardite's own CLI hardcodes argv[0] to "wardite"; we set argv ourselves
# instead of shelling out to it.)
#
# stdout carries guest output and nothing else -- the harness compares it byte
# for byte against the wasmtime oracle. Diagnostics, including the
# `load_ms=<float>` line the harness records, go to stderr.
#
# Exit status: wardite implements proc_exit with Kernel#exit, so a guest that
# calls it terminates this process with the guest's status directly and the
# code below the run never executes. A guest that returns from _start normally
# exits 0.

require "wardite"

def die(message)
  warn "wardite.rb: #{message}"
  exit 1
end

path = ARGV.shift
die "usage: ruby wardite.rb <module.wasm> [guest-args...]" unless path
die "#{path}: no such file" unless File.file?(path)

guest_argv = [File.basename(path), *ARGV]

started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
instance = File.open(path, "rb") do |io|
  Wardite::BinaryLoader.load_from_buffer(io, enable_wasi: true)
end
die "#{path}: module does not import WASI" unless instance.wasi
instance.wasi.argv = guest_argv
elapsed_ms = (Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000.0
warn format("load_ms=%.3f", elapsed_ms)

instance.runtime._start
$stdout.flush
