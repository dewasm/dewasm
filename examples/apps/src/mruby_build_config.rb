# mruby's own build system: `rake` (host Ruby), driven by an MRuby::Build
# config file. Two targets:
#
# - 'host': the minimal native build `MRuby::CrossBuild` needs for `mrbc`,
#   the bytecode compiler that turns mrblib/gem Ruby sources into the C
#   arrays baked into the wasm build (mirrors upstream's build_config/mrbc.rb).
# - 'wasm32-wasi': the actual target, whose cc/linker/archiver are wrapper
#   scripts around `zig cc`/`zig ar` that scripts/mruby.sh generates and
#   passes in through DEWASM_MRUBY_CC/LD/AR (the LLVM SJLJ flags and the
#   zig-0.16 rt.o workaround live there, not here: see that script's
#   comments). DEWASM_MRUBY_GEMS is the space-separated core-gem list
#   scripts/mruby.sh selected as compiling clean on wasi.
#
# Kernel#puts is defined only by mruby-io (mrblib/kernel.rb: `$stdout.puts`),
# which cannot build for wasi (its src/io.c unconditionally `#include
# <sys/wait.h>` for IO.popen/fork, absent from wasi-libc); mruby-wasi-puts
# below is a first-party gem restoring #puts on top of core Kernel#print
# (src/print.c, needs no gem).
#
# `rake` writes a lockfile pinning each gem's git checkout next to whatever
# file MRUBY_CONFIG points at (`Lockfile.enable` runs unconditionally when
# the class loads); every gem here is `core:` (part of the sha256-pinned
# source tree already), so it would only ever record mruby's own
# version/release_no, and it would land as an untracked file beside this
# checked-in one. Disabled: nothing here needs it.
MRuby::Lockfile.disable

MRuby::Build.new do |conf|
  conf.toolchain
  conf.build_mrbc_exec
  conf.disable_libmruby
  conf.disable_presym
end

MRuby::CrossBuild.new('wasm32-wasi') do |conf|
  conf.cc.command = ENV.fetch('DEWASM_MRUBY_CC')
  conf.linker.command = ENV.fetch('DEWASM_MRUBY_LD')
  conf.archiver.command = ENV.fetch('DEWASM_MRUBY_AR')
  conf.archiver.archive_options = 'rcs "%{outfile}" %{objs}'

  conf.exts.executable = '.wasm'

  conf.gem core: 'mruby-bin-mruby'
  ENV.fetch('DEWASM_MRUBY_GEMS').split.each { |name| conf.gem core: name }
  conf.gem File.expand_path('mruby-wasi-puts', __dir__)
end
