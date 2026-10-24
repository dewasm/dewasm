class RequestProfile < ApplicationRecord
  scope :search, ->(query) {
    next all if query.blank?

    q = "%#{sanitize_sql_like(query.strip)}%"
    where("http_method || ' ' || path LIKE :q OR mode LIKE :q", q: q)
  }
end
