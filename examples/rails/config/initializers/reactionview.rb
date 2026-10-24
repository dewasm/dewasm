# frozen_string_literal: true

# ReActionView compiles the `.html.erb` templates through Herb's engine, as Rails 8.2 does, and
# adds Herb's error page in development.
ReActionView.configure do |config|
  config.intercept_erb = true

  # Reactive templates: a template declares a state the browser owns (`herb:state`), an attribute
  # changes it, and the browser asks the server for the branches that read it.
  # The post page's `View HTML` switch is one; the request it makes runs Pygments.
  config.slots = true

  # The developer tools (`debug_mode`) are on in development by default: element outlines,
  # per-tag diagnostics, and the ERB behind an element.

  # The parse limit the converted Herb needs is in `.herb.yml`, which every `Herb::Engine` reads.

  # Herb's development server watches the view files through `cruise`, a native extension.
  # Off, nothing outside Ruby is loaded.
  config.dev_server = false
end
