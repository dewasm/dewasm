# Standalone shim smoke: exercises the sqlite3-gem-shaped API surface the
# Rails adapter depends on, without ActiveRecord. Run: ruby test_shim.rb
require_relative "lib/sqlite3"
require "tmpdir"

def assert(cond, label)
  raise "FAIL: #{label}" unless cond
  puts "ok: #{label}"
end

Dir.mktmpdir do |dir|
  path = File.join(dir, "shim.sqlite3")

  db = SQLite3::Database.new(path, results_as_hash: true, default_transaction_mode: :immediate)
  assert !db.closed?, "open"
  assert db.encoding == Encoding::UTF_8, "encoding"

  # Pragmas (the Rails DEFAULT_PRAGMAS setters)
  db.foreign_keys = true
  db.synchronous = :normal
  db.mmap_size = 134_217_728
  db.journal_size_limit = 67_108_864
  db.cache_size = 2000
  jm = db.journal_mode = :wal
  puts "journal_mode=wal -> #{jm.inspect}"
  assert SQLite3::Pragmas.method_defined?("journal_mode="), "Pragmas introspection"

  db.busy_handler_timeout = 100

  db.execute_batch2(<<~SQL)
    CREATE TABLE users (id INTEGER PRIMARY KEY, name varchar(255), score FLOAT, bio TEXT, avatar BLOB, born_at datetime(6));
    CREATE UNIQUE INDEX idx_users_name ON users(name);
  SQL

  stmt = db.prepare("INSERT INTO users (name, score, bio, avatar, born_at) VALUES (?, ?, ?, ?, ?)")
  stmt.bind_params(["alice", 12.5, "こんにちは", "\x00\xFFbin".b, "2026-07-28 12:34:56.789"])
  assert stmt.step.nil?, "insert step -> nil"
  assert db.changes == 1, "changes"
  first_id = db.last_insert_row_id
  assert first_id == 1, "last_insert_row_id"

  stmt.reset!
  stmt.bind_params(["bob", -3, nil, nil, nil])
  stmt.step
  stmt.close
  assert stmt.closed?, "stmt closed"

  # Rails-style read: prepare/step arrays + columns/types
  s = db.prepare("SELECT id, name, score, bio, avatar, born_at FROM users ORDER BY id")
  assert s.columns == %w[id name score bio avatar born_at], "columns"
  assert s.types == ["integer", "varchar(255)", "float", "text", "blob", "datetime(6)"], "types"
  rows = s.to_a
  assert rows.length == 2, "row count"
  a, b = rows
  assert a[1] == "alice" && a[1].encoding == Encoding::UTF_8 && a[1].frozen?, "text col"
  assert a[2] == 12.5, "float col"
  assert a[3] == "こんにちは", "utf8 col"
  assert a[4] == "\x00\xFFbin".b && a[4].encoding == Encoding::ASCII_8BIT, "blob col"
  assert b[2] == -3, "negative i64 col"
  assert b[3].nil?, "null col"
  assert s.step.nil? && s.done?, "step after done"
  s.close

  # results_as_hash affects only the execute family
  h = db.execute("SELECT id, name FROM users WHERE name = ?", ["alice"]).first
  assert h == { "id" => 1, "name" => "alice" }, "execute hash row"
  assert db.get_first_value("SELECT count(*) FROM users") == 2, "get_first_value"

  # 64-bit boundaries (INTEGER affinity column)
  db.execute("CREATE TABLE t64 (v INTEGER)")
  db.execute("INSERT INTO t64 (v) VALUES (?)", [9_223_372_036_854_775_807])
  assert db.get_first_value("SELECT v FROM t64") == 9_223_372_036_854_775_807, "i64 max"
  db.execute("UPDATE t64 SET v = ?", [-9_223_372_036_854_775_808])
  assert db.get_first_value("SELECT v FROM t64") == -9_223_372_036_854_775_808, "i64 min"

  # Error mapping: exact message + exception class
  begin
    db.execute("INSERT INTO users (name) VALUES (?)", ["alice"])
    raise "FAIL: expected ConstraintException"
  rescue SQLite3::ConstraintException => e
    assert e.message.include?("UNIQUE constraint failed: users.name"), "unique message"
  end
  begin
    db.execute("SELECT * FROM missing_table")
    raise "FAIL: expected SQLException"
  rescue SQLite3::SQLException => e
    assert e.message.include?("no such table"), "no such table"
  end

  # Transactions
  db.transaction { db.execute("INSERT INTO users (name) VALUES ('tx')") }
  assert db.get_first_value("SELECT count(*) FROM users WHERE name = 'tx'") == 1, "transaction commit"
  begin
    db.transaction do
      db.execute("INSERT INTO users (name) VALUES ('boom')")
      raise "rollback me"
    end
  rescue RuntimeError
  end
  assert db.get_first_value("SELECT count(*) FROM users WHERE name = 'boom'") == 0, "transaction rollback"
  assert !db.transaction_active?, "autocommit restored"

  assert db.total_changes >= 5, "total_changes"

  # closed-database discipline (Rails matches this exact message)
  db.close
  assert db.closed?, "closed"
  begin
    db.prepare("SELECT 1")
    raise "FAIL: expected ArgumentError"
  rescue ArgumentError => e
    assert e.message == "prepare called on a closed database", "closed db message"
  end

  # Reopen the same file: data persisted through the wasm WASI filesystem
  db2 = SQLite3::Database.new(path)
  assert db2.get_first_value("SELECT name FROM users WHERE id = ?", 1) == "alice", "reopen + read (array mode)"
  db2.close

  # :memory: and readonly
  mem = SQLite3::Database.new(":memory:")
  mem.execute("CREATE TABLE t(a)")
  mem.close
  ro = SQLite3::Database.new(path, readonly: true)
  begin
    ro.execute("INSERT INTO users (name) VALUES ('nope')")
    raise "FAIL: expected ReadOnlyException"
  rescue SQLite3::ReadOnlyException
    assert true, "readonly enforced"
  end
  ro.close
end

puts "SHIM-OK"
