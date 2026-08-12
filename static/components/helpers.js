/* ═══════════════════════════════════════════════
   Zpot-RS — Helpers de UI reutilizables

   Funciones para construir HTML de formularios,
   inputs, selects, grids, badges y layouts.
   ═══════════════════════════════════════════════ */

var zUI = {};

// Cache de respuestas fetch
var zCache = {};
var zCacheTTL = 5000; // 5 segundos

function zFetch(url, opts){
  var key = url + '|' + (opts ? JSON.stringify(opts) : 'GET');
  var now = Date.now();
  if(zCache[key] && (now - zCache[key].time) < zCacheTTL){
    return Promise.resolve(zCache[key].data);
  }
  return fetch(url, opts).then(function(r){
    return r.json().then(function(data){
      zCache[key] = {data: data, time: Date.now()};
      return data;
    });
  });
}

// Limpiar cache (forzar recarga)
function zClearCache(url){
  if(url){
    for(var k in zCache){
      if(k.startsWith(url)) delete zCache[k];
    }
  } else {
    zCache = {};
  }
}

// Input de texto/number/password con label
// opts: {type, value, mono, min, max, placeholder, comment}
zUI.input = function(id, label, ph, opts){
  opts = opts || {};
  var type = opts.type || 'text';
  var val = opts.value ? ' value="'+opts.value+'"' : '';
  var min = opts.min ? ' min="'+opts.min+'"' : '';
  var max = opts.max ? ' max="'+opts.max+'"' : '';
  var cls = opts.mono ? 'mono' : '';
  var placeholder = ph ? ' placeholder="'+ph+'"' : '';
  var comment = opts.comment ? '<span class="field-comment">'+opts.comment+'</span>' : '';
  return '<div><label>'+label+'</label><input id="'+id+'" type="'+type+'"'+val+min+max+placeholder+' class="'+cls+'">'+comment+'</div>';
};

// Select con opciones
// options: [{value:'x', text:'X'}, ...] o [['val','Text'], ...]
zUI.select = function(id, label, options){
  var opts = options.map(function(o){
    if(Array.isArray(o)) return '<option value="'+o[0]+'">'+o[1]+'</option>';
    return '<option value="'+o.value+'">'+o.text+'</option>';
  }).join('');
  return '<div><label>'+label+'</label><select id="'+id+'">'+opts+'</select></div>';
};

// Checkbox
zUI.checkbox = function(id, label, checked){
  var chk = checked ? ' checked' : '';
  return '<div class="checkbox-wrap"><label><input id="'+id+'" type="checkbox"'+chk+'> '+label+'</label></div>';
};

// Textarea
zUI.textarea = function(id, label, ph, opts){
  opts = opts || {};
  var placeholder = ph ? ' placeholder="'+ph+'"' : '';
  var cls = opts.mono ? 'mono' : '';
  return '<div class="span-2"><label>'+label+'</label><textarea id="'+id+'" rows="'+ (opts.rows||3) +'" class="'+cls+'"'+placeholder+'></textarea></div>';
};

// Grid contenedor
zUI.grid = function(cols, content){
  return '<div class="grid-'+cols+'">'+content+'</div>';
};

// Modal helper completo: title + formHtml en grid
zUI.openModal = function(id, titulo, fields, opts){
  opts = opts || {};
  var cols = opts.cols||2;
  var size = opts.size||'lg';
  var boton = opts.boton||'Guardar';
  var formHtml = zUI.grid(cols, fields);
  zModal.show(id, titulo, formHtml, boton, opts.callback, {size:size, cols:cols});
};

// Badge helpers
zUI.badge = function(text, color){
  return '<span class="badge"'+(color?' style="color:'+color+';border-color:'+color+'66"':'')+'>'+text+'</span>';
};

zUI.badgeUp = function(text){ return '<span class="badge badge-up">● '+ (text||'up') +'</span>'; };
zUI.badgeDown = function(text){ return '<span class="badge badge-down">● '+ (text||'down') +'</span>'; };
zUI.badgeWarn = function(text){ return '<span class="badge badge-warn">● '+ (text||'warn') +'</span>'; };

// Celda monospace
zUI.mono = function(text, color){
  var c = color ? ' style="color:'+color+'"' : '';
  return '<span style="font-family:var(--font-mono);font-size:0.85rem"'+c+'>'+escHtml(text)+'</span>';
};

// Celda de interfaz (color azul)
zUI.iface = function(name){
  return '<span style="color:var(--accent-blue);font-weight:500">'+escHtml(name)+'</span>';
};

// Descripción (o placeholder)
zUI.desc = function(text){
  return text ? '<span style="color:var(--clr-text-muted);font-size:0.8rem">'+escHtml(text)+'</span>'
              : '<span style="color:var(--clr-text-light)">—</span>';
};

// Page header: título + botón
zUI.pageHeader = function(title, btnHtml){
  return '<div class="page-header"><h3>'+title+'</h3>'+(btnHtml||'')+'</div>';
};

// Input de number abreviado
zUI.num = function(id, label, val, opts){
  opts = opts || {};
  opts.type = 'number';
  opts.value = val||0;
  return zUI.input(id, label, '', opts);
};

// Escapar HTML — definido en app.js (se carga antes, version correcta)
// escHtml y escAttr NO se redefinen aqui para no sobrescribir la version de app.js
