# frozen_string_literal: true

DewasmCExt::Recipe.register "nokogiri" do
  checksum "1.19.4", "50c951611c92bca05c51411aef45f1cbc50f2821c4802758c5c6d34696533ab5"

  LIBXML2 = "2.13.9"
  LIBXSLT = "1.1.43"

  def configure
    self.sources = %w[ext/nokogiri/*.c gumbo-parser/src/*.c]
    self.include_dirs = %w[ext/nokogiri gumbo-parser/src]
    self.defines = { "NOKOGIRI_PACKAGED_LIBRARIES" => nil, "NOKOGIRI_PRECOMPILED_LIBRARIES" => nil,
                     "HAVE_XMLCTXTSETOPTIONS" => "1", "HAVE_XMLSWITCHENCODINGNAME" => "1",
                     "HAVE_RB_CATEGORY_WARNING" => "1" }
    self.cflags = %w[-std=c99 -Wno-declaration-after-statement]
    self.extension = "nokogiri/nokogiri"
  end

  def prefix = File.join(builder.cache_dir, "nokogiri-libs-#{LIBXML2}-#{LIBXSLT}")

  def before_compile
    build_libraries unless %w[libxml2 libxslt libexslt].all? { |l| File.exist?(File.join(prefix, "lib", "#{l}.a")) }
    header = File.join(builder.gem_dir, "lib/nokogiri/nokogiri_build.h")
    File.write(header, <<~H)
      #define NOKOGIRI_LIBXML2_PATCHES "#{patch_names('libxml2')}"
      #define NOKOGIRI_LIBXSLT_PATCHES "#{patch_names('libxslt')}"
      #define NOKOGIRI_OTHER_LIBRARY_VERSIONS ""
    H
    self.include_dirs = include_dirs + [File.join(prefix, "include/libxml2"), File.join(prefix, "include")]
    self.cflags = cflags + ["-include", header, "-L", File.join(prefix, "lib"), "-lxslt", "-lexslt", "-lxml2"]
  end

  def patch_names(library) = Dir[File.join(builder.gem_dir, "patches", library, "*.patch")].sort.map { |p| File.basename(p) }.join(" ")

  def cross_env
    sdk = DewasmCExt::Compiler.wasi_sdk
    { "CC" => "#{sdk}/bin/clang --target=wasm32-wasip1", "AR" => "#{sdk}/bin/llvm-ar",
      "RANLIB" => "#{sdk}/bin/llvm-ranlib", "NM" => "#{sdk}/bin/llvm-nm",
      "CFLAGS" => "-O2", "CPPFLAGS" => "-DNOKOGIRI_PRECOMPILED_LIBRARIES" }
  end

  def unpack_patched(name, version)
    root = File.join(builder.cache_dir, "src")
    dir = File.join(root, "#{name}-#{version}")
    FileUtils.rm_rf(dir)
    FileUtils.mkdir_p(root)
    run("tar", "-xJf", File.join(builder.gem_dir, "ports/archives/#{name}-#{version}.tar.xz"), "-C", root)
    Dir[File.join(builder.gem_dir, "patches", name, "*.patch")].sort.each do |patch|
      run("patch", "-s", "-p1", "-d", dir, "-i", patch)
    end
    dir
  end

  def run(*command, chdir: nil) = DewasmCExt::Compiler.run(command, log: builder.build_log, chdir: chdir)

  def build_libraries
    FileUtils.rm_rf(prefix)
    jobs = Etc.respond_to?(:nprocessors) ? Etc.nprocessors.to_s : "4"
    builder.say "building libxml2 #{LIBXML2} for wasm32-wasip1"
    dir = unpack_patched("libxml2", LIBXML2)
    run("./configure", "--host=wasm32-wasi", "--prefix=#{prefix}", "--disable-shared", "--enable-static",
        "--disable-dependency-tracking", "--without-python", "--without-readline", "--with-c14n", "--with-debug",
        "--without-threads", "--without-modules", "--without-zlib", "--without-lzma", "--without-iconv",
        "--without-http", *cross_env.map { |k, v| "#{k}=#{v}" }, chdir: dir)
    run("make", "-j#{jobs}", "libxml2.la", chdir: dir)
    run("make", "install-libLTLIBRARIES", "install-data", "install-binSCRIPTS", chdir: dir)
    builder.say "building libxslt #{LIBXSLT} for wasm32-wasip1"
    dir = unpack_patched("libxslt", LIBXSLT)
    run("./configure", "--host=wasm32-wasi", "--prefix=#{prefix}", "--disable-shared", "--enable-static",
        "--disable-dependency-tracking", "--without-python", "--without-crypto", "--with-debug",
        "--without-plugins", "--with-libxml-prefix=#{prefix}", *cross_env.map { |k, v| "#{k}=#{v}" }, chdir: dir)
    run("make", "-j#{jobs}", "-C", "libxslt", "libxslt.la", chdir: dir)
    run("make", "-j#{jobs}", "-C", "libexslt", "libexslt.la", chdir: dir)
    run("make", "-C", "libxslt", "install-libLTLIBRARIES", "install-data", chdir: dir)
    run("make", "-C", "libexslt", "install-libLTLIBRARIES", "install-data", chdir: dir)
  end
end
