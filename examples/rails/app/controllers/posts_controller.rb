class PostsController < ApplicationController
  allow_unauthenticated_access only: %i[ index show ]
  before_action :set_post, only: %i[ show edit update destroy ]

  PER_PAGE = 10

  def index
    @query = params[:q]
    posts = Post.search(@query).order(created_at: :desc)
    @page = Pagination.new(params[:page], per: PER_PAGE, total: posts.count)
    @posts = @page.slice(posts)
  end

  # The view's `code` state shows the body as the HTML it is served as, highlighted.
  # The browser asks for that branch as a `slots` request, which renders here without the layout.
  def show
  end

  def new
    @post = Post.new
  end

  def edit
  end

  def create
    @post = Post.new(post_params)

    if @post.save
      redirect_to @post, notice: "Post was successfully created."
    else
      render :new, status: :unprocessable_content
    end
  end

  def update
    if @post.update(post_params)
      redirect_to @post, notice: "Post was successfully updated.", status: :see_other
    else
      render :edit, status: :unprocessable_content
    end
  end

  def destroy
    @post.destroy!
    redirect_to posts_path, notice: "Post was successfully destroyed.", status: :see_other
  end

  private
    def set_post
      @post = Post.find(params.expect(:id))
    end

    def post_params
      params.expect(post: [ :title, :body ])
    end
end
