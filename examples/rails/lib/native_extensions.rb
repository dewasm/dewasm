# frozen_string_literal: true

require "bundled_gems"
require "set"

# Which native extensions the process has loaded, and which of them Ruby itself did not ship.
# "Shipped with Ruby" is a default gem or a bundled gem, whatever its version, or a file under
# Ruby's own architecture directory (`socket`, `openssl`, and the rest of the standard library).
module NativeExtensions
  SHIPPED_GEMS = (Gem::Specification.default_stubs.map(&:name) + Gem::BUNDLED_GEMS::SINCE.keys).to_set
  ARCHDIR = "#{RbConfig::CONFIG["archdir"]}/"
  # The profiler behind the profile pages: loaded once a request is profiled, and reported apart.
  INSTRUMENTATION_GEMS = %w[vernier].to_set

  def self.loaded
    $LOADED_FEATURES.grep(/\.(bundle|so|dylib)\z/)
  end

  def self.gem_of(path)
    Gem::Specification.find do |spec|
      path.start_with?("#{spec.full_gem_path}/") || path.start_with?("#{spec.extension_dir}/")
    end
  end

  # The loaded extensions outside Ruby's own and the profiler: empty when the application runs
  # on Ruby alone.
  # A feature with no directory, such as `enumerator.so`, is one Ruby links in and lists by name.
  def self.outside_ruby
    loaded.reject do |path|
      next true if path.start_with?(ARCHDIR) || !path.include?("/")
      spec = gem_of(path)
      spec && (SHIPPED_GEMS.include?(spec.name) || INSTRUMENTATION_GEMS.include?(spec.name))
    end
  end

  def self.instrumentation
    loaded.select { |path| (spec = gem_of(path)) && INSTRUMENTATION_GEMS.include?(spec.name) }
  end

  # `dewasm` when the gems' extensions run converted (the loader defines `DewasmCExt::Runtime`),
  # `native` when the gems' own extensions are loaded.
  def self.mode = defined?(DewasmCExt::Runtime) ? "dewasm" : "native"

  def self.report
    {
      mode: mode,
      loaded: loaded.map { |path| [gem_of(path)&.full_name || "ruby", File.basename(path)] },
      instrumentation: instrumentation.map { |path| gem_of(path).full_name },
      outside_ruby: outside_ruby
    }
  end
end
