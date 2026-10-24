class Post < ApplicationRecord
  validates :title, presence: true
  validates :body, presence: true

  scope :search, ->(query) {
    query.blank? ? all : where("title LIKE :q OR body LIKE :q", q: "%#{sanitize_sql_like(query.strip)}%")
  }
end
