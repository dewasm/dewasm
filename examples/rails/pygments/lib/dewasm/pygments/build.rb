# frozen_string_literal: true

require "digest"
require "fileutils"
require_relative "../../../../cext/lib/dewasm_cext/compiler"

module Dewasm
  module Pygments
    # The build of `generated/`: the Pygments wheel, unpacked; the standard library of the
    # CPython SDK; the reactor of `ext/pyg.c` linked against the SDK, pre-initialized by Wizer,
    # and converted to Ruby by dewasm.
    #
    # The SDK is `examples/apps/cache/cpython-sdk`, which `examples/apps/setup.sh` builds.
    # A stamp records the inputs, so a changed wheel, C file, or SDK rebuilds.
    class Build
      ROOT = File.expand_path("../../..", __dir__)
      GENERATED = File.join(ROOT, "lib/dewasm/pygments/generated")
      WORK = File.join(ROOT, "build")
      SDK = File.expand_path("../../apps/cache/cpython-sdk", ROOT)
      SDK_HINT = "run examples/apps/setup.sh (it builds CPython as a library for wasm32-wasip1)"
      PYTHON = "python3.14"
      WHEEL = {
        name: "pygments-2.21.0-py3-none-any.whl",
        url: "https://files.pythonhosted.org/packages/py3/p/pygments/pygments-2.21.0-py3-none-any.whl",
        sha256: "2363c69b61c4a97c838da3b130dcd6468f4848992b21a82f2a63ec34377137d9"
      }.freeze
      # The CPython link line of `Tools/wasm/wasi`, for a reactor.
      LINK = %w[
        -mexec-model=reactor -O2
        -D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_GETPID -D_WASI_EMULATED_PROCESS_CLOCKS
        -z stack-size=16777216 -Wl,--stack-first -Wl,--initial-memory=41943040
        -Wl,--export=malloc -Wl,--export=free
      ].freeze
      LIBS = %w[-ldl -lwasi-emulated-signal -lwasi-emulated-getpid -lwasi-emulated-process-clocks -lpthread -lm].freeze

      def initialize(log: $stderr)
        @log = log
      end

      def source = File.join(ROOT, "ext/pyg.c")
      def wheel = File.join(WORK, WHEEL[:name])
      def wasm = File.join(WORK, "pyg.wasm")
      def pre_wasm = File.join(WORK, "pyg-pre.wasm")
      def build_log = File.join(WORK, "build.log")
      def converted = File.join(GENERATED, "pygments_wasm.rb")
      def stamp_path = File.join(GENERATED, "stamp")
      def sdk_stamp_path = "#{SDK}.src-sha256"

      def stamp_key
        [WHEEL[:sha256], Digest::SHA256.file(source).hexdigest[0, 16], Digest::SHA256.file(__FILE__).hexdigest[0, 16],
         File.read(sdk_stamp_path).strip.tr("\n", " "), "cc:wasi-sdk"].join(" ")
      end

      def built?
        File.exist?(sdk_stamp_path) && File.exist?(stamp_path) && File.read(stamp_path).strip == stamp_key &&
          File.exist?(converted)
      end

      def run
        return GENERATED if built?

        sdk_present!
        say "building dewasm-pygments"
        FileUtils.mkdir_p(WORK)
        File.delete(build_log) if File.exist?(build_log)
        FileUtils.rm_rf(GENERATED)
        FileUtils.mkdir_p(GENERATED)
        fetch_wheel
        unpack_wheel
        copy_stdlib
        link
        pre_initialize
        convert
        File.write(stamp_path, "#{stamp_key}\n")
        say "built dewasm-pygments -> #{GENERATED}"
        GENERATED
      end

      def sdk_present!
        return if File.exist?(File.join(SDK, "lib/libpython3.14.a")) && File.exist?(sdk_stamp_path)

        raise DewasmCExt::Compiler::Error, "#{SDK} is missing: #{SDK_HINT}"
      end

      def fetch_wheel
        return if File.exist?(wheel) && Digest::SHA256.file(wheel).hexdigest == WHEEL[:sha256]

        say "fetching #{WHEEL[:name]}"
        DewasmCExt::Compiler.download(URI(WHEEL[:url]), wheel)
        actual = Digest::SHA256.file(wheel).hexdigest
        return if actual == WHEEL[:sha256]

        File.delete(wheel)
        raise DewasmCExt::Compiler::Error, "#{WHEEL[:name]}: checksum #{actual} does not match #{WHEEL[:sha256]}"
      end

      # A wheel is a `.zip` archive; `/site` holds the `pygments` package it carries.
      def unpack_wheel
        DewasmCExt::Compiler.run(["unzip", "-q", wheel, "pygments/*", "-d", File.join(GENERATED, "site")], log: build_log)
      end

      def copy_stdlib
        FileUtils.mkdir_p(File.join(GENERATED, "lib"))
        FileUtils.cp_r(File.join(SDK, "lib", PYTHON), File.join(GENERATED, "lib", PYTHON))
      end

      def link
        libraries = Dir[File.join(SDK, "lib/*.a")].sort
        DewasmCExt::Compiler.run(
          DewasmCExt::Compiler.clang + LINK + ["-I", File.join(SDK, "include", PYTHON), source, "-o", wasm] +
            libraries + LIBS,
          log: build_log
        )
      end

      # Wizer calls `pyg_init` and writes the memory it leaves into the module.
      # The directories are the ones the Ruby side preopens, in the same order.
      def pre_initialize
        DewasmCExt::Compiler.run(
          [ENV.fetch("WIZER", "wizer"), "--allow-wasi", "--init-func", "pyg_init", "--wasm-bulk-memory", "true",
           "--mapdir", "/lib::#{File.join(GENERATED, 'lib')}", "--mapdir", "/site::#{File.join(GENERATED, 'site')}",
           "-o", pre_wasm, wasm],
          log: build_log
        )
      end

      def convert
        DewasmCExt::Compiler.run(
          DewasmCExt::Compiler.dewasm + [pre_wasm, "--target", "ruby", "--mode", "library", "--module-name",
                                         "PygmentsExt", "--function-names", "-o", converted],
          log: build_log
        )
      end

      def say(message) = @log&.puts("dewasm-pygments: #{message}")
    end
  end
end
