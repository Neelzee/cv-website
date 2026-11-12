// The editor's small bits of behaviour. Kept out of the templates so the CSP can forbid inline scripts.

// Empties the inputs of `data-clear` buttons after a successful htmx request
document.addEventListener("htmx:afterRequest", (e) => {
  const target = e.target.dataset?.clear;

  if (target && e.detail.successful) {
    document.querySelectorAll(target).forEach((input) => (input.value = ""));
  }
});

// Copies the markdown snippet in front of a `data-copy` button
document.addEventListener("click", (e) => {
  const button = e.target.closest("[data-copy]");

  if (!button) return;

  navigator.clipboard.writeText(button.previousElementSibling.textContent);
  const label = button.textContent;
  button.textContent = "Kopiert!";
  setTimeout(() => (button.textContent = label), 1000);
});
