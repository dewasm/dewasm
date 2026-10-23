# frozen_string_literal: true

require "digest"
require "fileutils"
require "net/http"
require "shellwords"
require "uri"

module DewasmCExt
  # The toolchain: the `clang` of `wasi-sdk` for the compile, dewasm for the conversion.
  module Compiler
    ROOT = File.expand_path("../../../../..", __dir__)
    CEXT = File.expand_path("../..", __dir__)
    INCLUDE = File.join(CEXT, "include")
    SUPPORT = File.join(INCLUDE, "support.c")
    # What the link exports for the host runtime: the `Init_*` symbol, buffers (`malloc`/`free`),
    # `chdir` for the working directory, the function table for method calls, and the shadow
    # stack pointer, which the host restores when an exception unwinds through guest frames.
    EXPORTS = %w[malloc free chdir].freeze
    WASI_SDK_HINT = "set WASI_SDK_PATH to a wasi-sdk root (`mise install` at the repository root does)"

    module_function

    def wasi_sdk
      path = ENV["WASI_SDK_PATH"].to_s
      raise Error, "wasi-sdk not found: #{WASI_SDK_HINT}" if path.empty? || !File.executable?(File.join(path, "bin/clang"))
      path
    end

    def clang = [File.join(wasi_sdk, "bin/clang"), "--target=wasm32-wasip1", "--no-wasm-opt"]

    # The dewasm command: `$DEWASM`, the release build in this repository, or `cargo run`.
    def dewasm
      return [ENV["DEWASM"]] if ENV["DEWASM"]
      built = File.join(ROOT, "target/release/dewasm")
      return [built] if File.executable?(built)
      ["cargo", "run", "--release", "-q", "--manifest-path", File.join(ROOT, "Cargo.toml"), "-p", "dewasm", "--"]
    end

    # A checksum of dewasm cext itself, so a header or runtime change rebuilds every gem.
    def layer_digest
      files = Dir[File.join(INCLUDE, "**/*.{h,c}")].sort + [__FILE__]
      Digest::SHA256.hexdigest(files.map { |f| File.read(f) }.join)[0, 16]
    end

    # Compile `sources` into `<out_dir>/<name>.wasm`, then convert it into `<out_dir>/<name>.rb`.
    # `--function-names` makes a profile read `sqlite3VdbeExec` rather than `_f1286`.
    def build(name:, module_name:, init:, out_dir:, sources:, cflags: [], log: nil)
      FileUtils.mkdir_p(out_dir)
      wasm = File.join(out_dir, "#{name}.wasm")
      rb = File.join(out_dir, "#{name}.rb")
      exports = ([init] + EXPORTS).map { |e| "-Wl,--export=#{e}" }
      run(clang + %w[-O2 -mexec-model=reactor -I] + [INCLUDE] + cflags + exports +
          %w[-Wl,--export-table -Wl,--export=__stack_pointer -Wl,--strip-debug -o] +
          [wasm, SUPPORT] + sources, log: log)
      run(dewasm + [wasm, "--target", "ruby", "--mode", "library", "--module-name", module_name,
                    "--function-names", "-o", rb], log: log)
      rb
    end

    # Fetch `uri` into `path`, following up to `limit` redirects.
    def download(uri, path, limit = 5)
      raise Error, "too many redirects for #{uri}" if limit.zero?
      Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == "https") do |http|
        http.request(Net::HTTP::Get.new(uri)) do |response|
          case response
          when Net::HTTPSuccess
            File.open(path, "wb") { |f| response.read_body { |chunk| f.write(chunk) } }
          when Net::HTTPRedirection
            return download(URI(response["location"]), path, limit - 1)
          else
            raise Error, "fetching #{uri} failed: #{response.code} #{response.message}"
          end
        end
      end
    end

    # Run a command, keeping its output in `log` (or passing it through), failing loud.
    def run(command, log: nil, chdir: nil, env: {})
      options = {}
      options[:chdir] = chdir if chdir
      if log
        File.open(log, "a") { |f| f.puts("$ #{Shellwords.join(command)}") }
        options[:out] = options[:err] = [log, "a"]
      end
      return if system(env, *command, **options)

      tail = log ? File.readlines(log).last(30).join : ""
      raise Error, "command failed: #{Shellwords.join(command)}\n#{tail}"
    end

    class Error < StandardError; end
  end
end
