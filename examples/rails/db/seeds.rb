user = User.find_or_create_by!(email_address: "author@example.com") do |u|
  u.password = "secret"
end
user.update!(password: "secret") if BCrypt::Password.new(user.password_digest).cost != BCrypt::Engine.cost

(Rails.env.test? ? 24 : 0).times do |i|
  Post.find_or_create_by!(title: "Post #{i + 1}: on converted extensions") do |post|
    post.body = (1..4).map do |n|
      "<p>Paragraph #{n} of post #{i + 1}. <b>Bold</b>, <i>italic</i>, <code>code</code>, " \
        "<a href=\"https://example.com/#{i}/#{n}\">a link</a>, and an <img src=\"x.png\" onerror=\"alert(1)\"> " \
        "that sanitize strips. #{'Lorem ipsum dolor sit amet, consectetur adipiscing elit. ' * 3}</p>"
    end.join("\n")
  end
end

Post.find_or_create_by!(title: "Hello from pure Ruby") do |post|
  post.body = <<~HTML
    <p>This post is served by Rails on a Ruby process without a native extension of its own.</p>
    <p>The database is <b>SQLite</b>, the templates come through <b>Herb</b>,
    this body is cleaned by <b>Nokogiri</b>, and the sign-in uses <b>bcrypt</b>.</p>
    <script>alert("this tag does not survive sanitize")</script>
  HTML
end

Post.find_or_create_by!(title: "Unicode round trip: 日本語の投稿") do |post|
  post.body = "<p>絵文字 🎉 と <code>&lt;code&gt;</code> も通ります。</p>"
end

puts "seeds: #{User.count} user, #{Post.count} posts (sign in as #{user.email_address} / secret)" unless Rails.env.test?
