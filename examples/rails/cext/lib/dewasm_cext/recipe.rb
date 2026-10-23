# frozen_string_literal: true

module DewasmCExt
  # How one gem's C extension is compiled against the dewasm `ruby.h`.
  #
  # A recipe names the sources, the flags, the `Init_*` symbol, and where the gem requires its
  # native extension from; the builder does the rest.
  # A gem without a recipe of its own gets the generic one: every `.c` under `ext/<name>/`.
  class Recipe
    class << self
      def registry = (@registry ||= {})

      # `register "name" do ... end`: define a recipe class for a gem, set up by the block.
      def register(gem_name, &block)
        registry[gem_name] = Class.new(self, &block).tap do |klass|
          klass.gem_name = gem_name
          klass.source_path = caller_locations(1, 1).first.path
        end
      end

      def for(gem_name)
        registry.fetch(gem_name) { Generic }
      end

      attr_accessor :gem_name
      # The file the recipe is defined in, part of the build stamp.
      attr_writer :source_path
      def source_path = @source_path || __FILE__

      # Known `.gem` checksums by version; a version outside this table is recorded on first use.
      def checksums = (@checksums ||= {})
      def checksum(version, sha256) = checksums[version] = sha256

      def setting(name, default = nil)
        define_method(name) { instance_variable_defined?("@#{name}") ? instance_variable_get("@#{name}") : default }
        define_method("#{name}=") { |value| instance_variable_set("@#{name}", value) }
      end
    end

    # Globs of C sources, relative to the unpacked gem.
    setting :sources, []
    # Include directories, relative to the unpacked gem, or absolute.
    setting :include_dirs, []
    # C definitions: `{ "NAME" => "1" }`, or `nil` for `-DNAME` with no value.
    setting :defines, {}
    setting :cflags, []
    # The symbol the gem's `extconf.rb` builds: `Init_<extension name>`.
    setting :init
    # Where the gem requires its extension from, relative to `lib/`: `"sqlite3/sqlite3_native"`.
    setting :extension
    # Ruby source appended to `DewasmCExt::Runtime.new(...)` in the loader, such as `preopens:`.
    setting :runtime_options, ""

    attr_reader :builder

    def initialize(builder)
      @builder = builder
      configure
    end

    # Per-gem settings, with the gem's sources at `builder.gem_dir`.
    def configure; end

    # Work before the compile, such as building a library the extension links against.
    # It may add to `cflags` and `include_dirs`.
    def before_compile; end

    def gem_name = self.class.gem_name
    def name = gem_name
    def module_name = "#{gem_name.split(/[^a-z0-9]/i).map(&:capitalize).join}Ext"
    def init_symbol = init || "Init_#{File.basename(extension)}"

    # A checksum of the recipe's own code, part of the build stamp.
    def source_digest = Digest::SHA256.file(self.class.source_path).hexdigest[0, 16]

    # The generic recipe: `ext/<name>/*.c` with `ext/<name>` on the include path.
    class Generic < Recipe
      def self.gem_name = nil

      def gem_name = builder.name

      def configure
        self.sources = ["ext/#{builder.name}/*.c"]
        self.include_dirs = ["ext/#{builder.name}"]
        self.extension = loader_target
      end

      # The generic recipe guesses the require path from the gem's `extconf.rb` location.
      def loader_target
        ext_dir = File.join(builder.gem_dir, "ext", builder.name)
        extconf = File.read(File.join(ext_dir, "extconf.rb"))
        target = extconf[/create_makefile\(\s*["']([^"']+)["']/, 1] || builder.name
        target.sub(%r{\A(lib/)?}, "")
      end
    end
  end
end
