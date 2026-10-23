# frozen_string_literal: true

# Profile request handling in this process with Vernier, through the integration test session.
# No server is involved: the full stack from the router to the converted SQLite runs here.
# Usage: `bin/rails runner script/profile.rb <out.json> [requests]`.
# Open the output in the Firefox Profiler (`profiler.firefox.com`), or draw it with
# `script/flamegraph.rb`.
require "vernier"
require "action_dispatch/testing/integration"

out = ARGV.fetch(0)
rounds = (ARGV[1] || "20").to_i

ActionDispatch::IntegrationTest.app = Rails.application
session = ActionDispatch::Integration::Session.new(Rails.application)
session.host! "127.0.0.1"

def token_of(session)
  session.response.body[/name="csrf-token" content="([^"]+)"/, 1]
end

# Warm up: sign in, create a post, and render each page once so that templates are compiled.
session.get "/session/new"
session.post "/session", params: { authenticity_token: token_of(session),
                                   email_address: "author@example.com", password: "secret" }
session.get "/posts/new"
session.post "/posts", params: { authenticity_token: token_of(session),
                                 post: { title: "Profiled", body: "<p>body <b>bold</b></p>" } }
post_path = session.response.location.sub(%r{\Ahttps?://[^/]+}, "")
session.get post_path
session.post "#{post_path}/comments", params: { authenticity_token: token_of(session),
                                                comment: { author: "profiler", body: "warm" } }
session.get "/posts"

# A round covers the index, the post page, a comment, and a sign-in check against `bcrypt`.
Vernier.profile(out: out, interval: 200, allocation_interval: 0) do
  rounds.times do |i|
    session.get "/posts"
    session.get post_path
    session.post "#{post_path}/comments", params: { authenticity_token: token_of(session),
                                                    comment: { author: "r#{i}", body: "comment #{i}" } }
    session.get "/session/new"
    session.post "/session", params: { authenticity_token: token_of(session),
                                       email_address: "author@example.com", password: "secret" }
  end
end

puts "profile: #{rounds} rounds written to #{out}"
