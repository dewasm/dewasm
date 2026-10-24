# frozen_string_literal: true

require "digest"
require "fileutils"
require "net/http"
require "shellwords"
require "uri"

module DewasmCExt
  module Compiler
    ROOT = File.expand_path("../../../../..", __dir__)
    CEXT = File.expand_path("../..", __dir__)
    INCLUDE = File.join(CEXT, "include")
    SUPPORT = File.join(INCLUDE, "support.c")
    EXPORTS = %w[malloc free chdir].freeze
    WASI_SDK_HINT = "set WASI_SDK_PATH to a wasi-sdk root (`mise install` at the repository root does)"

    module_function

    def wasi_sdk
      path = ENV["WASI_SDK_PATH"].to_s
      raise Error, "wasi-sdk not found: #{WASI_SDK_HINT}" if path.empty? || !File.executable?(File.join(path, "bin/clang"))
      path
    end

    def clang = [File.join(wasi_sdk, "bin/clang"), "--target=wasm32-wasip1", "--no-wasm-opt"]

    def dewasm
      return [ENV["DEWASM"]] if ENV["DEWASM"]
      built = File.join(ROOT, "target/release/dewasm")
      return [built] if File.executable?(built)
      ["cargo", "run", "--release", "-q", "--manifest-path", File.join(ROOT, "Cargo.toml"), "-p", "dewasm", "--"]
    end

    def layer_digest
      files = Dir[File.join(INCLUDE, "**/*.{h,c}")].sort + [__FILE__]
      Digest::SHA256.hexdigest(files.map { |f| File.read(f) }.join)[0, 16]
    end

    def build(name:, module_name:, init:, out_dir:, sources:, cflags: [], log: nil)
      wasm = compile(name: name, out_dir: out_dir, sources: sources, cflags: cflags, exports: [init], log: log)
      convert(wasm, File.join(out_dir, "#{name}.rb"), module_name, log: log)
    end

    def compile(name:, out_dir:, sources:, cflags: [], exports: [], support: true, link_flags: [], log: nil)
      FileUtils.mkdir_p(out_dir)
      wasm = File.join(out_dir, "#{name}.wasm")
      export_flags = (exports + EXPORTS).map { |e| "-Wl,--export=#{e}" }
      run(clang + %w[-O2 -mexec-model=reactor -I] + [INCLUDE] + cflags + export_flags +
          %w[-Wl,--export-table -Wl,--export=__stack_pointer -Wl,--strip-debug -o] +
          [wasm] + (support ? [SUPPORT] : []) + sources + link_flags, log: log)
      wasm
    end

    def convert(wasm, rb, module_name, log: nil)
      run(dewasm + [wasm, "--target", "ruby", "--mode", "library", "--module-name", module_name,
                    "--function-names", "-o", rb], log: log)
      rb
    end

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
