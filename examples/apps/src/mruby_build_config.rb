# mruby's own build system: `rake` (host Ruby), driven by an MRuby::Build config file.
# It has two targets:
#
# - 'host': the minimal native build `MRuby::CrossBuild` needs for `mrbc`.
#   mrbc is the bytecode compiler that turns mrblib/gem Ruby sources into C arrays.
#   Those arrays are baked into the wasm build (this mirrors upstream's build_config/mrbc.rb).
# - 'wasm32-wasi': the actual target.
#   Its cc/linker/archiver are wrapper scripts around wasi-sdk's clang/llvm-ar.
#   scripts/mruby.sh generates them and passes them in through DEWASM_MRUBY_CC/LD/AR.
#   The LLVM SJLJ flags and the -lsetjmp link live there, not here: see that script's comments.
#   DEWASM_MRUBY_GEMS is the space-separated core-gem list scripts/mruby.sh selected.
#   It holds the gems that compile clean on wasi.
#
# Kernel#puts is defined only by mruby-io (mrblib/kernel.rb: `$stdout.puts`).
# mruby-io cannot build for wasi.
# Its src/io.c unconditionally `#include <sys/wait.h>` for IO.popen/fork, absent from wasi-libc.
# mruby-wasi-puts below is a first-party gem restoring #puts on top of core Kernel#print.
# Kernel#print is src/print.c and needs no gem.
#
# `rake` writes a lockfile pinning each gem's git checkout.
# The lockfile goes next to whatever file MRUBY_CONFIG points at.
# `Lockfile.enable` runs unconditionally when the class loads.
# Every gem here is `core:`, part of the sha256-pinned source tree already.
# So the lockfile would only ever record mruby's own version/release_no.
# It would land as an untracked file beside this checked-in one.
# Disabled: nothing here needs it.
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
