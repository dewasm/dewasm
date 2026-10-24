import "reactionview"

// Herb's debug mode connects to a development server whether or not one runs.
// ReActionView 0.6.0 has no setting against it, and this application runs no such server.
document.addEventListener("herb:dev-tools-start", (event) => {
  const client = event.detail?.client
  if (!client) return
  client.showUnavailableAlert = () => {}
  client.disconnect()
})
