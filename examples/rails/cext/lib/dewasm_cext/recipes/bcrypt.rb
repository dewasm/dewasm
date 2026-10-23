# frozen_string_literal: true

# `bcrypt`: `ext/mri`, the `$objs` of its `extconf.rb` minus the x86 assembly.
DewasmCExt::Recipe.register "bcrypt" do
  checksum "3.1.22", "1f0072e88c2d705d94aff7f2c5cb02eb3f1ec4b8368671e19112527489f29032"

  def configure
    self.sources = %w[ext/mri/bcrypt_ext.c ext/mri/crypt_blowfish.c ext/mri/crypt_gensalt.c ext/mri/wrapper.c]
    self.include_dirs = %w[ext/mri]
    self.defines = { "__SKIP_GNU" => nil }
    self.extension = "bcrypt_ext"
  end
end
