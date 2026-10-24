Rails.application.routes.draw do
  resources :posts
  resource :session
  resources :passwords, param: :token
  resources :profiles, only: %i[index show]
  post "profiles/toggle" => "profiles#toggle", as: :toggle_profiles
  get "up" => "rails/health#show", as: :rails_health_check

  root "posts#index"
end
