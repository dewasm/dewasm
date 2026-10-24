# frozen_string_literal: true

require "digest"
require "fileutils"
require "pathname"
require "rubygems/package"
require_relative "compiler"
require_relative "recipe"
Dir[File.join(__dir__, "recipes/*.rb")].sort.each { |f| require f }

module DewasmCExt
  class Builder
    def self.default_cache(root = Dir.pwd) = ENV["DEWASM_CEXT_CACHE"] || File.join(root, "vendor/dewasm")

    attr_reader :name, :version, :cache_dir, :lock_path, :gem_dir, :recipe

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
    def loader_path = File.join(gem_dir, recipe.loader_dir, "#{recipe.extension}.rb")
    def converted_name = recipe.converted_name || "#{File.basename(recipe.extension)}_wasm"
    def gemspec_path = File.join(gem_dir, "#{name}.gemspec")

    def compiled? = File.exist?(stamp_path) && File.read(stamp_path).strip == stamp_key

    def built? = compiled? && loader_paths.all? { |path| File.exist?(path) && File.read(path) == recipe.loader(from: File.dirname(path)) }

    def refresh_loaders
      write_loader unless built?
      gem_dir
    end

    def ensure_built
      return refresh_loaders if compiled?

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

    def unpack
      FileUtils.rm_rf(gem_dir)
      package = Gem::Package.new(gem_file)
      package.extract_files(gem_dir)
      spec = package.spec
      spec.extensions = []
      recipe.gemspec(spec)
      File.write(gemspec_path, spec.to_ruby)
    end

    def compile
      sources = recipe.sources.flat_map { |g| Dir[File.expand_path(g, gem_dir)].sort }
      raise Compiler::Error, "#{name}: no C sources matched #{recipe.sources.inspect}" if sources.empty?
      includes = recipe.include_dirs.map { |d| ["-I", File.expand_path(d, gem_dir)] }.flatten
      defines = recipe.defines.map { |k, v| v.nil? ? "-D#{k}" : "-D#{k}=#{v}" }
      out_dir = File.dirname(loader_path)
      wasm = Compiler.compile(
        name: converted_name, out_dir: out_dir, sources: sources, cflags: includes + defines + recipe.cflags,
        exports: (recipe.support ? [recipe.init_symbol] : []) + recipe.exports,
        support: recipe.support, link_flags: recipe.link_flags, log: build_log
      )
      Compiler.convert(recipe.after_compile(wasm), File.join(out_dir, "#{converted_name}.rb"), recipe.module_name,
                       log: build_log)
    end

    # A gem tries `<ruby major.minor>/` first with a plain `require`.
    # A copy there keeps an installed native build of the same gem out.
    def loader_paths
      paths = [loader_path]
      paths << File.join(File.dirname(loader_path), ruby_minor, File.basename(loader_path)) if recipe.support
      paths
    end

    def write_loader
      loader_paths.each do |path|
        FileUtils.mkdir_p(File.dirname(path))
        File.write(path, recipe.loader(from: File.dirname(path)))
      end
    end

    def say(message) = @log&.puts("dewasm-cext: #{message}")
  end
end
