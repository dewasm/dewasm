class CommentsController < ApplicationController
  allow_unauthenticated_access

  def create
    post = Post.find(params.expect(:post_id))
    comment = post.comments.build(params.expect(comment: [ :author, :body ]))

    if comment.save
      redirect_to post, notice: "Comment added."
    else
      redirect_to post, alert: comment.errors.full_messages.to_sentence
    end
  end
end
