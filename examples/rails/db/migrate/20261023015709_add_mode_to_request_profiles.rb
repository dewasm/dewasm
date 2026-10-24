class AddModeToRequestProfiles < ActiveRecord::Migration[8.1]
  def change
    add_column :request_profiles, :mode, :string
  end
end
