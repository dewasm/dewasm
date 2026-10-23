# frozen_string_literal: true

require "digest"
require "fileutils"
require "pathname"
require "rubygems/package"
require_relative "compiler"
require_relative "recipe"
Dir[File.join(__dir__, "recipes/*.rb")].sort.each { |f| require f }

module DewasmCExt
  # One gem at one version: fetched from rubygems.org, unpacked, its extension compiled and
  # converted, and a loader `.rb` put where the gem requires its native extension from.
  # The result is a directory a Gemfile can name with `path:`.
  #
  # A stamp beside the directory records what it was built from.
  # A new gem checksum, a changed recipe, a changed dewasm cext, or a changed toolchain rebuilds.
  class Builder
    # The build directory: `vendor/dewasm` under the Gemfile (`dewasm_gem`) or the working
    # directory (`bin/build-gem`), unless `DEWASM_CEXT_CACHE` names one.
    def self.default_cache(root = Dir.pwd) = ENV["DEWASM_CEXT_CACHE"] || File.join(root, "vendor/dewasm")

    attr_reader :name, :version, :cache_dir, :lock_path, :gem_dir, :recipe

    # `lock_path` records the checksum of a `.gem` the recipe does not know, on first fetch.
    def initialize(name, version, cache_dir: Builder.default_cache, lock_path: nil, log: $stderr)
      @name = name
      @version = version
      @cache_dir = File.expand_path(cache_dir)
      @lock_path = lock_path
      @log = log
      @gem_dir = File.join(@cache_dir, "gems", "#{name}-#{version}")
      @recipe = Recipe.for(name).new(self)
    end

    def gem_file = File.join(cache_dir, "downloads", "#{name}-#{version}.gem")
    def stamp_path = "#{gem_dir}.stamp"
    def build_log = "#{gem_dir}.log"
    def loader_path = File.join(gem_dir, "lib", "#{recipe.extension}.rb")
    def converted_name = "#{File.basename(recipe.extension)}_wasm"
    def gemspec_path = File.join(gem_dir, "#{name}.gemspec")

    def built? = File.exist?(stamp_path) && File.read(stamp_path).strip == stamp_key && File.exist?(loader_path)

    # Fetch, unpack, compile, convert, and write the loader, unless the stamp is current.
    def ensure_built
      return gem_dir if built?

      say "building #{name} #{version} (#{recipe.class.gem_name ? 'its recipe' : 'the generic recipe'})"
      fetch
      unpack
      File.delete(build_log) if File.exist?(build_log)
      recipe.before_compile
      compile
      write_loader
      File.write(stamp_path, "#{stamp_key}\n")
      say "built #{name} #{version} -> #{gem_dir}"
      gem_dir
    end

    def stamp_key
      [gem_checksum, recipe.source_digest, Compiler.layer_digest, "cc:wasi-sdk", "ruby-#{ruby_minor}"].join(" ")
    end

    def ruby_minor = RUBY_VERSION[/\A\d+\.\d+/]

    # ---- fetch ---------------------------------------------------------------

    def fetch
      return if File.exist?(gem_file) && Digest::SHA256.file(gem_file).hexdigest == gem_checksum(allow_new: true)

      FileUtils.mkdir_p(File.dirname(gem_file))
      say "fetching #{name}-#{version}.gem"
      download(URI("https://rubygems.org/downloads/#{name}-#{version}.gem"), gem_file)
      actual = Digest::SHA256.file(gem_file).hexdigest
      expected = known_checksum
      if expected.nil?
        record_checksum(actual)
      elsif expected != actual
        File.delete(gem_file)
        raise Compiler::Error, "#{name}-#{version}.gem: checksum #{actual} does not match the recorded #{expected}"
      end
    end

    def download(uri, path) = Compiler.download(uri, path)

    # The checksum the recipe or the lock file knows; `nil` when neither does.
    def known_checksum
      recipe.class.checksums[version] || lock_entries["#{name}-#{version}"]
    end

    def gem_checksum(allow_new: false)
      known_checksum || (allow_new ? nil : Digest::SHA256.file(gem_file).hexdigest)
    end

    def lock_entries
      return {} unless lock_path && File.exist?(lock_path)
      File.readlines(lock_path, chomp: true).filter_map { |l| l.split(" ", 2).then { |k, v| [k, v] if v } }.to_h
    end

    def record_checksum(sha256)
      raise Compiler::Error, "no checksum known for #{name}-#{version}.gem and no lock file to record it in" unless lock_path
      say "recording the checksum of #{name}-#{version}.gem in #{lock_path}"
      File.open(lock_path, "a") { |f| f.puts("#{name}-#{version} #{sha256}") }
    end

    # ---- unpack --------------------------------------------------------------

    # The `.gemspec` written beside the files drops `extensions`, so Bundler does not build the
    # native one.
    def unpack
      FileUtils.rm_rf(gem_dir)
      package = Gem::Package.new(gem_file)
      package.extract_files(gem_dir)
      spec = package.spec
      spec.extensions = []
      File.write(gemspec_path, spec.to_ruby)
    end

    # ---- compile and loader -----------------------------------------------

    def compile
      sources = recipe.sources.flat_map { |g| Dir[File.expand_path(g, gem_dir)].sort }
      raise Compiler::Error, "#{name}: no C sources matched #{recipe.sources.inspect}" if sources.empty?
      includes = recipe.include_dirs.map { |d| ["-I", File.expand_path(d, gem_dir)] }.flatten
      defines = recipe.defines.map { |k, v| v.nil? ? "-D#{k}" : "-D#{k}=#{v}" }
      Compiler.build(
        name: converted_name, module_name: recipe.module_name, init: recipe.init_symbol,
        out_dir: File.dirname(loader_path), sources: sources,
        cflags: includes + defines + recipe.cflags, log: build_log
      )
    end

    # The `.rb` the gem requires in place of its native extension.
    # It loads the host runtime, the converted class, and runs the extension's `Init_*`.
    # A copy sits under `<dir>/<ruby major.minor>/` too, the place a prebuilt gem keeps its
    # extension, since a gem tries that path first with a plain `require`.
    # Found there, the `require` never asks RubyGems for a gem that could provide the file, so an
    # installed native copy of the gem stays out.
    # The paths are relative, so the build directory moves with the application.
    def write_loader
      runtime = File.expand_path("../dewasm_cext.rb", __dir__)
      converted = File.join(File.dirname(loader_path), converted_name)
      [loader_path, File.join(File.dirname(loader_path), ruby_minor, File.basename(loader_path))].each do |path|
        FileUtils.mkdir_p(File.dirname(path))
        from = Pathname.new(File.dirname(path))
        File.write(path, <<~RUBY)
          # Generated by DewasmCExt: the converted extension in place of the native one.
          require_relative #{Pathname.new(runtime).relative_path_from(from).to_s.inspect}
          require_relative #{Pathname.new(converted).relative_path_from(from).to_s.inspect}
          DewasmCExt::Runtime.new(#{recipe.module_name}, #{recipe.init_symbol.inspect}#{recipe.runtime_options})
        RUBY
      end
    end

    def say(message) = @log&.puts("dewasm-cext: #{message}")
  end
end
