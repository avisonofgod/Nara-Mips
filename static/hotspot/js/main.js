function normalizeInput(input) {
  input.value = input.value.toUpperCase().replace(/\s+/g, "");
}

document.getElementById("username").addEventListener("input", function () {
  normalizeInput(this);
});

document.getElementById("password").addEventListener("input", function () {
  normalizeInput(this);
});

document.getElementById("loginForm").addEventListener("submit", function () {
  normalizeInput(document.getElementById("username"));
  normalizeInput(document.getElementById("password"));
});
