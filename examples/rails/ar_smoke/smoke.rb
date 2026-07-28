# ActiveRecord on the dewasm sqlite3 shim, without Rails: migration, CRUD,
# transactions, type round-trips, joins. Run: bundle exec ruby smoke.rb
require "active_record"
require "fileutils"
require "logger"

def assert(cond, label)
  raise "FAIL: #{label}" unless cond
  puts "ok: #{label}"
end

FileUtils.rm_rf(File.join(__dir__, "tmp"))
FileUtils.mkdir_p(File.join(__dir__, "tmp"))
db_path = File.join(__dir__, "tmp", "ar.sqlite3")

ActiveRecord::Base.establish_connection(
  adapter: "sqlite3",
  database: db_path,
  pool: 5,
  timeout: 5000
)

ActiveRecord::Schema.define do
  create_table :authors, force: true do |t|
    t.string :name, null: false
    t.boolean :active, default: true
    t.timestamps
  end
  add_index :authors, :name, unique: true

  create_table :posts, force: true do |t|
    t.references :author, null: false, foreign_key: true
    t.string :title
    t.text :body
    t.float :rating
    t.integer :views, limit: 8
    t.binary :thumbnail
    t.datetime :published_at
    t.timestamps
  end
end
puts "ok: migration"

class Author < ActiveRecord::Base
  has_many :posts
end

class Post < ActiveRecord::Base
  belongs_to :author
end

alice = Author.create!(name: "alice")
bob = Author.create!(name: "bob", active: false)
assert Author.count == 2, "create/count"
assert alice.id == 1 && alice.persisted?, "persisted"

post = alice.posts.create!(
  title: "Hello dewasm",
  body: "こんにちは、純Ruby SQLite!",
  rating: 4.5,
  views: 9_007_199_254_740_993, # > 2^53: must survive as INTEGER
  thumbnail: "\x89PNG\x00\xFF".b,
  published_at: Time.utc(2026, 7, 28, 12, 34, 56)
)
reloaded = Post.find(post.id)
assert reloaded.body == "こんにちは、純Ruby SQLite!", "utf8 round-trip"
assert reloaded.rating == 4.5, "float round-trip"
assert reloaded.views == 9_007_199_254_740_993, "i64 round-trip"
assert reloaded.thumbnail == "\x89PNG\x00\xFF".b, "binary round-trip"
assert reloaded.published_at == Time.utc(2026, 7, 28, 12, 34, 56), "datetime round-trip"
assert reloaded.author == alice, "belongs_to"

# Prepared-statement cache reuse (same SQL executed repeatedly)
10.times { |i| assert Post.where(title: "Hello dewasm").count == 1, "stmt cache #{i}" if i == 9 }

# Update / delete counters
post.update!(rating: 5.0)
assert Post.find(post.id).rating == 5.0, "update"
assert Post.where("rating >= ?", 5.0).count == 1, "where with bind"

# Transaction rollback
begin
  ActiveRecord::Base.transaction do
    Author.create!(name: "carol")
    raise ActiveRecord::Rollback
  end
end
assert Author.count == 2, "transaction rollback"

# Unique-constraint mapping to RecordNotUnique
begin
  Author.create!(name: "alice")
  raise "FAIL: expected RecordNotUnique"
rescue ActiveRecord::RecordNotUnique
  puts "ok: RecordNotUnique"
end

# NOT NULL violation
begin
  Author.create!(name: nil)
  raise "FAIL: expected NotNullViolation"
rescue ActiveRecord::NotNullViolation
  puts "ok: NotNullViolation"
end

# Foreign-key violation (pragma foreign_keys=ON is a DEFAULT_PRAGMA)
begin
  Post.create!(author_id: 999_999, title: "orphan")
  raise "FAIL: expected InvalidForeignKey"
rescue ActiveRecord::InvalidForeignKey
  puts "ok: InvalidForeignKey"
end

# Join + aggregate
assert Author.joins(:posts).where(posts: { rating: 5.0 }).first&.name == "alice", "join"
assert Post.maximum(:rating) == 5.0, "aggregate"

# insert_all (multi-row) & pluck
Author.insert_all([{ name: "dave" }, { name: "erin" }])
assert Author.order(:name).pluck(:name) == %w[alice bob dave erin], "insert_all + pluck"

# Destroy
bob.destroy!
assert Author.where(name: "bob").count == 0, "destroy"

# Schema introspection (Rails reads sqlite_master / table_info heavily)
assert (%w[authors posts] - ActiveRecord::Base.connection.tables).empty?, "tables"
cols = ActiveRecord::Base.connection.columns(:posts).map(&:name)
assert cols.include?("thumbnail"), "columns introspection"

puts "AR-SMOKE-OK"
