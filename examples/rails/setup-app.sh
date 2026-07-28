#!/usr/bin/env bash
# Generate the demo Rails app (gitignored) and graft in the few files that
# make it ours: the Gemfile points sqlite3 at the dewasm shim, plus a Post
# model with a JSON controller. Requires the rails gem (>= 8.1).
set -euo pipefail
cd "$(dirname "$0")"

if [ -d app ]; then
  echo "setup-app: app/ already exists — remove it first to regenerate" >&2
  exit 1
fi

rails new app --minimal --skip-bundle --skip-git --skip-docker --skip-ci \
  --skip-kamal --skip-solid --skip-test --skip-system-test --database=sqlite3

ruby -pi -e 'sub(/^gem "sqlite3".*$/, %q{gem "sqlite3", ">= 2.1", path: "../sqlite3"})' app/Gemfile
grep -q 'path: "../sqlite3"' app/Gemfile || {
  echo "setup-app: failed to point the Gemfile at the shim" >&2
  exit 1
}

cp app-template/posts_controller.rb app/app/controllers/posts_controller.rb
cp app-template/routes.rb app/config/routes.rb
cp app-template/post.rb app/app/models/post.rb
mkdir -p app/db/migrate
cp app-template/create_posts_migration.rb app/db/migrate/20260728000001_create_posts.rb

echo "setup-app: done — next: ./run.sh"
