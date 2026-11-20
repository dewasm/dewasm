# frozen_string_literal: true

DewasmCExt::Recipe.register "herb" do
  checksum "0.11.2", "926f16b6c552a077755f686d30f795db049a5596814a0dfd8133cbe36be787ba"

  def configure
    self.sources = %w[ext/herb/*.c src/**/*.c vendor/prism/src/*.c]
    self.include_dirs = %w[src/include vendor/prism/include vendor/prism/src]
    self.defines = { "HERB_USE_MALLOC" => nil, "HERB_EXCLUDE_PRETTYPRINT" => nil, "PRISM_EXCLUDE_PRETTYPRINT" => nil,
                     "PRISM_EXCLUDE_JSON" => nil, "PRISM_EXCLUDE_PACK" => nil }
    self.cflags = %w[-fvisibility=hidden]
    self.extension = "herb/herb"
  end

  def before_compile
    self.sources = sources.flat_map { |g| Dir[File.expand_path(g, builder.gem_dir)] }.reject { |f| f.end_with?("/src/main.c") }
  end
end
