class PostsController < ApplicationController
  skip_before_action :verify_authenticity_token

  def index
    render json: Post.order(:id).as_json(only: %i[id title body views rating])
  end

  def create
    post = Post.create!(params.require(:post).permit(:title, :body, :views, :rating))
    render json: post.as_json(only: %i[id title body views rating]), status: :created
  end

  def stats
    render json: {
      count: Post.count,
      max_views: Post.maximum(:views),
      avg_rating: Post.average(:rating)&.to_f,
      sqlite_version: ActiveRecord::Base.connection.select_value("SELECT sqlite_version()")
    }
  end
end
