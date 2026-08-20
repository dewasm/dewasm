# A drop-in replacement for the sqlite3 gem's Rails-facing API, backed not by the native libsqlite3 but by SQLite compiled to wasm32-wasi and converted to pure Ruby by dewasm (see ../../README.md).
# `build.sh` generates the sqlite3_wasm.rb this loads.
require_relative "sqlite3/version"
require_relative "sqlite3/constants"
require_relative "sqlite3/errors"
require_relative "sqlite3/wasm_driver"
require_relative "sqlite3/fork_safety"
require_relative "sqlite3/pragmas"
require_relative "sqlite3/statement"
require_relative "sqlite3/database"

module SQLite3
  # Match the real gem's Blob marker class; binding semantics key off
  # ASCII-8BIT encoding either way.
  class Blob < String; end

  def self.libversion
    Constants::SQLITE_VERSION_NUMBER
  end

  module Constants
    SQLITE_VERSION_NUMBER = 3_053_003
  end
  SQLITE_VERSION = "3.53.3-wasm"
  SQLITE_LOADED_VERSION = SQLITE_VERSION
  SQLITE_PACKAGED_LIBRARIES = false
  SQLITE_PRECOMPILED_LIBRARIES = false

  def self.threadsafe = 0
  def self.threadsafe? = false
end
