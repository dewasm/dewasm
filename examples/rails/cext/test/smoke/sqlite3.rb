# frozen_string_literal: true

# The unmodified sqlite3 gem on the converted extension.
# It covers what ActiveRecord's adapter uses, plus the callbacks the old shim could not offer.
# Usage: `ruby sqlite3.rb <unpacked gem dir> <scratch dir>`.
require "tmpdir"

gem_dir = ARGV.fetch(0)
scratch = ARGV.fetch(1)
$LOAD_PATH.unshift File.join(gem_dir, "lib")
require "sqlite3"

raise "native extension loaded" if $LOADED_FEATURES.grep(/sqlite3_native\.(so|bundle)$/).any?

def check(desc)
  yield
  puts "ok   #{desc}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "FAIL #{desc}: #{e.class}: #{e.message}"
  puts e.backtrace.first(8).map { |l| "     #{l}" }
  $failed = true
end

db = SQLite3::Database.new(":memory:")
check("version strings") do
  raise SQLite3::SQLITE_VERSION unless SQLite3::SQLITE_VERSION == "3.54.1"
  raise SQLite3::SQLITE_LOADED_VERSION unless SQLite3::SQLITE_LOADED_VERSION == "3.54.1"
end
check("DDL, DML, and typed results") do
  db.execute("CREATE TABLE posts (id INTEGER PRIMARY KEY, title TEXT, body TEXT, views INTEGER, rating REAL, data BLOB)")
  db.execute("INSERT INTO posts (title, body, views, rating, data) VALUES (?, ?, ?, ?, ?)",
             ["Hello", "world", 9_007_199_254_740_993, 4.5, SQLite3::Blob.new("\x00\xffbin".b)])
  row = db.execute("SELECT title, body, views, rating, data FROM posts").first
  raise row.inspect unless row == ["Hello", "world", 9_007_199_254_740_993, 4.5, "\x00\xffbin".b]
  raise row[0].encoding.to_s unless row[0].encoding == Encoding::UTF_8
  raise row[4].encoding.to_s unless row[4].encoding == Encoding::BINARY
  raise "last_insert_row_id" unless db.last_insert_row_id == 1
end
check("prepared statement: binds, columns, types, reset") do
  stmt = db.prepare("SELECT id, title FROM posts WHERE id = :id")
  stmt.bind_params(id: 1)
  raise stmt.columns.inspect unless stmt.columns == %w[id title]
  raise stmt.types.inspect unless stmt.types == %w[integer text]
  raise "step" unless stmt.step == [1, "Hello"]
  raise "done" unless stmt.step.nil?
  stmt.reset!
  stmt.bind_params(id: 2)
  raise "no row" unless stmt.step.nil?
  stmt.close
end
check("UTF-8 round trip and i64 boundaries") do
  db.execute("INSERT INTO posts (title, views) VALUES (?, ?)", ["日本語の題", -(2**63)])
  db.execute("INSERT INTO posts (title, views) VALUES (?, ?)", ["max", 2**63 - 1])
  rows = db.execute("SELECT title, views FROM posts WHERE id > 1 ORDER BY id")
  raise rows.inspect unless rows == [["日本語の題", -(2**63)], ["max", 2**63 - 1]]
  # Past int64 the gem binds a double, as the native extension does.
  db.execute("INSERT INTO posts (title, views) VALUES (?, ?)", ["wide", 2**63])
  raise "2**63" unless db.execute("SELECT views FROM posts WHERE title = 'wide'").first == [2.0**63]
end
check("constraint error maps to an exception with SQLite's message") do
  db.execute("CREATE TABLE u (name TEXT UNIQUE)")
  db.execute("INSERT INTO u VALUES ('a')")
  begin
    db.execute("INSERT INTO u VALUES ('a')")
    raise "no error"
  rescue SQLite3::ConstraintException => e
    raise e.message unless e.message.include?("UNIQUE constraint failed: u.name")
  end
  begin
    db.execute("SELEC 1")
    raise "no error"
  rescue SQLite3::SQLException => e
    raise e.message unless e.message.include?("syntax error")
  end
end
check("transactions") do
  db.transaction { db.execute("INSERT INTO u VALUES ('b')") }
  begin
    db.transaction do
      db.execute("INSERT INTO u VALUES ('c')")
      raise "abort"
    end
  rescue RuntimeError
  end
  raise db.execute("SELECT name FROM u ORDER BY name").inspect unless db.execute("SELECT name FROM u ORDER BY name") == [["a"], ["b"]]
end
check("create_function: SQLite calls back into Ruby") do
  db.create_function("double_it", 1) { |func, value| func.result = value * 2 }
  raise "result" unless db.execute("SELECT double_it(21)").first == [42]
  db.create_function("fails", 0) { |_func| raise ArgumentError, "from ruby" }
  begin
    db.execute("SELECT fails()")
    raise "no error"
  rescue ArgumentError => e
    raise e.message unless e.message == "from ruby"
  end
end
check("create_aggregate: a Ruby aggregator") do
  db.create_aggregate("my_sum", 1) do
    step { |func, value| @sum = (@sum || 0) + value }
    finalize { |func| func.result = @sum || 0 }
  end
  raise "aggregate" unless db.execute("SELECT my_sum(views) FROM posts WHERE id = 1").first == [9_007_199_254_740_993]
end
check("collation: a Ruby comparator") do
  reverse = Object.new
  def reverse.compare(a, b) = b <=> a
  db.collation("reverse", reverse)
  rows = db.execute("SELECT name FROM u ORDER BY name COLLATE reverse")
  raise rows.inspect unless rows == [["b"], ["a"]]
end
check("busy_handler and busy_timeout accept") do
  db.busy_timeout(100)
  db.busy_handler { |count| count < 3 }
end
check("pragma and execute_batch") do
  db.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE t2 (x); INSERT INTO t2 VALUES (1);")
  raise "foreign_keys" unless db.execute("PRAGMA foreign_keys").first == [1]
  raise "batch" unless db.execute("SELECT x FROM t2").first == [1]
end
check("a database file on disk") do
  path = File.join(scratch, "smoke.sqlite3")
  File.delete(path) if File.exist?(path)
  file_db = SQLite3::Database.new(path)
  file_db.execute("CREATE TABLE k (v TEXT)")
  file_db.execute("INSERT INTO k VALUES ('persisted')")
  file_db.close
  raise "no file" unless File.size(path) > 0
  again = SQLite3::Database.new(path, readonly: true)
  raise "readonly?" unless again.readonly?
  raise "reopen" unless again.execute("SELECT v FROM k").first == ["persisted"]
  again.close
end
check("statement garbage: many short-lived statements") do
  2000.times { |i| db.execute("SELECT ?", [i]) }
  GC.start
  raise "count" unless db.execute("SELECT count(*) FROM posts").first == [4]
end
db.close
raise "closed?" unless db.closed?

if $failed
  puts "sqlite3 smoke: FAILED"
  exit 1
end
puts "sqlite3 smoke: ok"
