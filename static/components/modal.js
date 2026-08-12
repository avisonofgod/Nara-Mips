/* ═══════════════════════════════════════════════
   Zpot-RS — Componente: Modal genérico
   
   Crea un modal overlay + panel dinámicamente.
   Uso:
     zModal.show('add-modal', '➕ Title', formHtml, 'Añadir', onSubmit)
     zModal.show('add-modal', '➕ Title', formHtml, 'Añadir', onSubmit, {size:'lg', cols:2})
   Opciones:
     size: 'sm' (420px default), 'lg' (580px), 'xl' (720px)
     cols: 1 (default), 2, 3  — grid de columnas para el body
     noActions: true — oculta botones Cancelar/Guardar
   ═══════════════════════════════════════════════ */

var zModal = {};

zModal.show = function(id, title, bodyHtml, btnLabel, onSubmit, opts){
  opts = opts || {};
  var existing = document.getElementById(id);
  if(existing){
    existing.style.display = 'flex';
    return;
  }
  var overlay = document.createElement('div');
  overlay.id = id;
  overlay.className = 'modal-overlay';
  overlay.onclick = function(e){
    if(e.target === overlay) zModal.close(id);
  };
  var sizeClass = opts.size ? 'modal-'+opts.size : '';
  var colsClass = opts.cols ? 'grid-'+opts.cols : '';
  if(colsClass) bodyHtml = '<div class="'+colsClass+'">'+bodyHtml+'</div>';
  var panel = document.createElement('div');
  panel.className = 'modal-content ' + sizeClass;
  var actionsHtml = opts.noActions ? '' :
    '<div class="modal-actions">' +
      '<button class="btn-ghost" onclick="zModal.close(\''+id+'\')">Cancelar</button>' +
      '<button class="btn-primary" id="'+id+'-submit">'+(btnLabel||'Guardar')+'</button>' +
    '</div>';
  panel.innerHTML =
    '<div class="modal-header"><h3 class="modal-title">'+title+'</h3></div>' +
    '<div class="modal-body">'+bodyHtml+'</div>' +
    actionsHtml;
  overlay.appendChild(panel);
  document.body.appendChild(overlay);
  overlay.style.display = 'flex';
  if(typeof onSubmit === 'function'){
    var btn = document.getElementById(id+'-submit');
    if(btn) btn.onclick = function(){ onSubmit(); };
  }
};

zModal.close = function(id){
  var el = document.getElementById(id);
  if(el) el.style.display = 'none';
};

zModal.setBody = function(id, html){
  var panel = document.querySelector('#'+id+' .modal-body');
  if(panel) panel.innerHTML = html;
};
