document.addEventListener("htmx:afterRequest", (e) => {
  const target = e.target.dataset?.clear;

  if (target && e.detail.successful) {
    document.querySelectorAll(target).forEach((input) => (input.value = ""));
  }
});

document.addEventListener("click", (e) => {
  const button = e.target.closest("[data-copy]");

  if (!button) return;

  navigator.clipboard.writeText(button.previousElementSibling.textContent);
  const label = button.textContent;
  button.textContent = "Kopiert!";
  setTimeout(() => (button.textContent = label), 1000);
});

const uploadButtonId = "open-button-upload-modal";
const imageButtonId = "open-button-image-modal";
const closeModalButtonId = "close-button-upload-modal";
const uploadDialogId = "upload-image-modal";
const imageDialogId = "image-modal";

const uploadImageButton = document.getElementById(uploadButtonId);
const closeModalButton = Array.from(document.getElementsByClassName(closeModalButtonId));
const uploadImageDialog = document.getElementById(uploadDialogId)
const imageDialog = document.getElementById(imageDialogId)
const imageButton = document.getElementById(imageButtonId)

if (uploadImageButton === null) {
  console.error("Cant find button with id: " + uploadButtonId);
} else if (uploadImageDialog === null) {
  console.error("Cant find dialog with id: " + uploadDialogId);
} else if (imageDialog === null) {
  console.error("Cant find dialog with id: " + imageDialogId);
} else if (closeModalButton.length === 0) {
  console.error("Cant find button with id: " + closeModalButtonId);
} else if (imageButton === null) {
  console.error("Cant find button with id: " + imageButtonId);
} else {
  closeModalButton.forEach(btn => {
    btn.addEventListener("click", _ => {
      uploadImageDialog.close();
      imageDialog.close();
    });
  });

  imageButton.addEventListener("click", _ => {
    imageDialog.showModal();
  });

  uploadImageButton.addEventListener("click", _ => {
    uploadImageDialog.showModal();
  });

  imageDialog.addEventListener("click", e => {
    if (e.target === imageDialog) imageDialog.close();
  });

  uploadImageDialog.addEventListener("click", e => {
    if (e.target === uploadImageDialog) uploadImageDialog.close();
  });
}

const titleInput = document.getElementById("title");
const slugInput = document.getElementById("slug");

if (titleInput && slugInput) {
  const slugify = (s) =>
    s
      .toLowerCase()
      .replace(/æ/g, "ae")
      .replace(/ø/g, "o")
      .replace(/å/g, "a")
      .normalize("NFD")
      .replace(/[\u0300-\u036f]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");
  let auto = slugInput.value === "";

  titleInput.addEventListener("input", () => {
    if (auto) slugInput.value = slugify(titleInput.value);
  });
  slugInput.addEventListener("input", () => {
    auto = slugInput.value === "";
  });
}

const previewModal = document.getElementById("full-preview-modal");

document.getElementById("open-full-preview")?.addEventListener("click", () => {
  document.getElementById("project-form").requestSubmit(
    document.getElementById("full-preview-submit"));
  previewModal.showModal();
});
previewModal?.addEventListener("click", (e) => {
  if (e.target === previewModal || e.target.closest("[data-close]")) previewModal.close();
});