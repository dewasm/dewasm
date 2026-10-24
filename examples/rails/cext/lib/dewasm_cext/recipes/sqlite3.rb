# frozen_string_literal: true

DewasmCExt::Recipe.register "sqlite3" do
  checksum "2.10.1", "7ae52c91f5b766882cf44d2dbd26d2dbbb841606f0ec78df3be019bd07b13ff4"

  SQLITE = {
    url: "https://sqlite.org/2026/sqlite-autoconf-3540100.tar.gz",
    sha256: "926ef8d3eab93fbf6e06011901273eec076b25594a4e1cdadd50c987f4f6d64e",
    dir: "sqlite-autoconf-3540100"
  }.freeze
  HAVE = %w[RB_ENC_INTERNED_STR_CSTR RB_ENC_INTERNED_STR RB_PROC_ARITY RB_INTEGER_PACK
            SQLITE3_INITIALIZE SQLITE3_BACKUP_INIT SQLITE3_COLUMN_DATABASE_NAME SQLITE3_OPEN_V2
            SQLITE3_PREPARE_V2 SQLITE3_DB_NAME SQLITE3_ERROR_OFFSET SQLITE3_SERIALIZE
            SQLITE3_DESERIALIZE TYPE_SQLITE3_INT64 TYPE_SQLITE3_UINT64].freeze

  def configure
    self.sources = %w[ext/sqlite3/*.c]
    self.include_dirs = %w[ext/sqlite3]
    self.defines = HAVE.to_h { |h| ["HAVE_#{h}", "1"] }.merge(
      "_WASI_EMULATED_PROCESS_CLOCKS" => nil, "_WASI_EMULATED_SIGNAL" => nil, "SQLITE_NOHAVE_SYSTEM" => nil,
      "SQLITE_OMIT_LOAD_EXTENSION" => nil, "SQLITE_THREADSAFE" => "0", "SQLITE_DEFAULT_WAL_SYNCHRONOUS" => "1",
      "SQLITE_USE_URI" => "1", "SQLITE_ENABLE_DBPAGE_VTAB" => "1", "SQLITE_ENABLE_DBSTAT_VTAB" => "1",
      "SQLITE_ENABLE_COLUMN_METADATA" => "1", "SQLITE_ENABLE_MATH_FUNCTIONS" => "1"
    )
    self.cflags = %w[-fvisibility=hidden -lwasi-emulated-process-clocks -lwasi-emulated-signal]
    self.extension = "sqlite3/sqlite3_native"
    self.runtime_options = ', preopens: { "/" => "/" }, cwd: Dir.pwd'
  end

  def before_compile
    src = File.join(builder.cache_dir, "src", SQLITE[:dir])
    unless File.exist?(File.join(src, "sqlite3.c"))
      tarball = File.join(builder.cache_dir, "downloads", "#{SQLITE[:dir]}.tar.gz")
      builder.download(URI(SQLITE[:url]), tarball) unless File.exist?(tarball)
      actual = Digest::SHA256.file(tarball).hexdigest
      raise DewasmCExt::Compiler::Error, "#{tarball}: checksum #{actual} is not #{SQLITE[:sha256]}" if actual != SQLITE[:sha256]
      FileUtils.mkdir_p(File.dirname(src))
      DewasmCExt::Compiler.run(["tar", "-xzf", tarball, "-C", File.dirname(src),
                                "#{SQLITE[:dir]}/sqlite3.c", "#{SQLITE[:dir]}/sqlite3.h", "#{SQLITE[:dir]}/sqlite3ext.h"],
                               log: builder.build_log)
    end
    self.sources = sources + [File.join(src, "sqlite3.c")]
    self.include_dirs = include_dirs + [src]
  end
end
