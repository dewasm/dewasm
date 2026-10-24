# frozen_string_literal: true

require "bundler"
require "pathname"
require_relative "builder"

module DewasmCExt
  module Gemfile
    BUILD_COMMANDS = %w[install update lock].freeze

    def dewasm_gem(name, version, **options)
      return if (@groups & Bundler.settings[:without]).any?

      root = Bundler.root
      builder = Builder.new(name, version, cache_dir: Builder.default_cache(root),
                                           lock_path: File.join(root, "dewasm-gems.lock"))
      if builder.compiled?
        builder.refresh_loaders
      elsif DewasmCExt::Gemfile.build_allowed?
        builder.ensure_built
      else
        raise Bundler::GemfileError,
              "#{name} #{version} is not built for dewasm; run `bundle install` (or set DEWASM_CEXT_BUILD=1)"
      end
      replaced = @dependencies.find { |dependency| dependency.name == name }
      @dependencies.delete(replaced) if replaced
      groups = Array(options.delete(:group)) + Array(options.delete(:groups)) + (replaced&.groups || [:default])
      gem name, version, path: Pathname.new(builder.gem_dir).relative_path_from(root).to_s, group: groups, **options
    end

    def self.build_allowed?
      return true if ENV["DEWASM_CEXT_BUILD"] == "1"
      File.basename($PROGRAM_NAME) == "bundle" && BUILD_COMMANDS.include?(ARGV.first)
    end
  end
end

Bundler::Dsl.include(DewasmCExt::Gemfile)
