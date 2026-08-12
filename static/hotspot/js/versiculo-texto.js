const verseBox = document.getElementById("verseBox");

fetch("/hotspot/portal/static/js/versiculos.json")
  .then(r => r.ok ? r.json() : Promise.reject("No se pudo cargar"))
  .then(versiculos => {
    const elegido = versiculos[Math.floor(Math.random() * versiculos.length)];
    if (elegido && elegido.texto && elegido.referencia) {
      verseBox.innerHTML = `"${elegido.texto}"<br><em>${elegido.referencia}</em>`;
    }
  })
  .catch(() => {
    // Fallback en caso extremo
    verseBox.innerHTML = '"Jehová es mi pastor; nada me faltará."<br><em>Salmos 23:1</em>';
  });
