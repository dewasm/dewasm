# frozen_string_literal: true

gem_dir = ARGV.fetch(0)
$LOAD_PATH.unshift File.join(gem_dir, "lib")
require "bcrypt"

raise "native extension loaded" if $LOADED_FEATURES.grep(/bcrypt_ext\.(so|bundle)$/).any?

secret = "s3kr1t"
pw = BCrypt::Password.create(secret, cost: 4)
raise "self-verify failed" unless pw == secret
raise "wrong secret accepted" if pw == "other"

reference = "$2a$05$CCCCCCCCCCCCCCCCCCCCC.E5YPO9kmyuRGyh0XouQYb4YMJKvyOeW"
raise "reference vector not accepted" unless BCrypt::Password.new(reference) == "U*U"
raise "reference vector accepted a wrong key" if BCrypt::Password.new(reference) == "U*V"

salt = BCrypt::Engine.generate_salt(4)
raise "bad salt #{salt}" unless salt.start_with?("$2a$04$") && salt.size == 29
puts "bcrypt smoke: ok (#{pw})"
