/* ═══════════════════════════════════════════════
   Zpot-RS — Componente: Tablas + celdas HTML
   
   Funciones para construir filas de tabla sin 
   repetir el loop + HTML en cada página.
   
   Uso:
     zTable.build(data, cols, rowFn) → string HTML
     zTable.render(id, data, cols, rowFn) → renderiza en tbody
     zTable.badge(val) → badge estilizado
     zTable.badgeStatus(state) → badge up/down
   ═══════════════════════════════════════════════ */

var zTable = {};

// Construye HTML de filas desde datos
zTable.build = function(data, rowFn){
  if(!data || !data.length) return '<tr><td colspan="10" class="page-empty">Sin datos</td></tr>';
  var h = '';
  for(var i=0;i<data.length;i++){
    h += rowFn(data[i], i);
  }
  return h;
};

// Renderiza directo en un tbody
zTable.render = function(tbodyId, data, rowFn){
  var t = document.getElementById(tbodyId);
  if(!t) return;
  t.innerHTML = zTable.build(data, rowFn);
};

// Badge genérico
zTable.badge = function(text, cls){
  return '<span class="badge '+cls+'">'+text+'</span>';
};

zTable.badgeUp = function(text){
  return zTable.badge(text||'up', 'badge-up');
};

zTable.badgeDown = function(text){
  return zTable.badge(text||'down', 'badge-down');
};

zTable.badgeInfo = function(text){
  return zTable.badge(text, 'badge-info');
};

zTable.badgeWarn = function(text){
  return zTable.badge(text, 'badge-warn');
};

zTable.badgePerm = function(text){
  return zTable.badge(text, 'badge-perm');
};

// Botón eliminar
zTable.deleteBtn = function(onclick, label){
  return '<button class="btn btn-danger btn-sm" onclick="'+onclick+'">'+(label||'🗑')+'</button>';
};
