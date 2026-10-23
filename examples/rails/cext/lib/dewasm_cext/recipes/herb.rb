# frozen_string_literal: true

# `herb`: `ext/herb`, its parser under `src/` (minus the CLI's `main.c`), and the Prism it vendors.
# The source list and flags follow its `extconf.rb`.
# `HERB_USE_MALLOC` is its own switch for a platform without `mmap`, which WASI is.
DewasmCExt::Recipe.register "herb" do
  checksum "0.11.1", "7a5a74c4931b341a2b7b7f5a530616801f5721a633d2655f002156ff553cd2d7"

  PRISM_MAIN = %w[diagnostic encoding node options pack prettyprint prism regexp serialize static_literals token_type].freeze
  PRISM_UTIL = %w[pm_buffer pm_char pm_constant_pool pm_integer pm_list pm_memchr pm_newline_list pm_string
                  pm_strncasecmp pm_strpbrk].freeze

  def configure
    self.sources = ["ext/herb/*.c", "src/**/*.c"] +
      PRISM_MAIN.map { |f| "vendor/prism/src/#{f}.c" } + PRISM_UTIL.map { |f| "vendor/prism/src/util/#{f}.c" }
    self.include_dirs = %w[src/include vendor/prism/include vendor/prism/src vendor/prism/src/util]
    self.defines = { "HERB_USE_MALLOC" => nil, "HERB_EXCLUDE_PRETTYPRINT" => nil, "PRISM_EXCLUDE_PRETTYPRINT" => nil,
                     "PRISM_EXCLUDE_JSON" => nil, "PRISM_EXCLUDE_PACK" => nil }
    self.cflags = %w[-fvisibility=hidden]
    self.extension = "herb/herb"
  end

  def before_compile
    self.sources = sources.flat_map { |g| Dir[File.expand_path(g, builder.gem_dir)] }.reject { |f| f.end_with?("/src/main.c") }
  end
end
