# frozen_string_literal: true

ReActionView.configure do |config|
  config.intercept_erb = true
  config.slots = true

  # The development server's file watcher, `cruise`, is a native extension.
  config.dev_server = false
end
