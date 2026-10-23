class Comment < ApplicationRecord
  belongs_to :post

  validates :author, presence: true
  validates :body, presence: true
end
