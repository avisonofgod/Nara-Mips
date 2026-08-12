// Zpot-RS app.js — SPA router + page loader + mock data
// Los submenús se definen AQUI → editar esta estructura cambia todo el menú
// Cada entrada: {menu:[{label,url,page}]}
// - label: texto del submenú
// - url: ruta SPA (se muestra en la barra)

// === HELPERS GLOBALES (compartidos entre paginas) ===
function escHtml(s){ if(!s) return ''; return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;'); }
function escAttr(s){ if(!s) return ''; return String(s).replace(/&/g,'&amp;').replace(/"/g,'&quot;').replace(/'/g,'&#39;'); }
function formatBytes(n){ if(!n || n === 0) return '0 B'; var u=['B','KB','MB','GB','TB']; var i=0; var v=n; while(v>=1024 && i<u.length-1){ v/=1024; i++; } return v.toFixed(i===0?0:1)+' '+u[i]; }
function formatPkts(n){ if(!n || n === 0) return '0'; if(n>=1000000) return (n/1000000).toFixed(1)+'M'; if(n>=1000) return (n/1000).toFixed(1)+'K'; return n.toString(); }

// Cache global de respuestas API
var apiCache = {};
function apiFetch(url, opts, ttl){
  ttl = ttl || 3000;
  var key = url + '|' + (opts ? JSON.stringify(opts) : 'GET');
  var now = Date.now();
  if(apiCache[key] && (now - apiCache[key].ts) < ttl){
    return Promise.resolve(apiCache[key].data);
  }
  return fetch(url, opts).then(function(r){
    return r.json().then(function(data){
      apiCache[key] = {data: data, ts: Date.now()};
      return data;
    });
  });
}
function apiClear(url){
  if(url){
    for(var k in apiCache){ if(k.startsWith(url)) delete apiCache[k]; }
  } else { apiCache = {}; }
}
function getActionBadge(action){ var c=action==='accept'?'#22c55e':(action==='drop'?'#ef4444':(action==='reject'?'#f59e0b':'#60a5fa')); return '<span style="color:'+c+';font-weight:600">'+action+'</span>'; }
function getChainStyle(chain){ var c=chain==='input'?'#60a5fa':(chain==='forward'?'#a78bfa':'#f472b6'); return 'style="color:'+c+';font-family:monospace;font-weight:600"'; }
function mostrarModal(id){ var m=document.getElementById(id); if(m) m.style.display='flex'; }
function cerrarModal(id){ var m=document.getElementById(id); if(m) m.style.display='none'; }
// - page: nombre del archivo HTML en /static/pages/ (sin extensión)
//   Si page es null, se usa renderFn(pageName) para contenido dinámico

var PAGES = {
  dashboard:   {icon:'📊', label:'Dashboard',  items:[{label:'Dashboard',  url:'/',              page:'dashboard'}]},
  interfaces:  {icon:'🔌', label:'Interfaces', items:[{label:'List',      url:'/interfaces/list',     page:'interfaces'},{label:'MWAN',      url:'/routing/mwan',    page:'routing-mwan'},{label:'VLANs',    url:'/interfaces/vlans',    page:'interfaces-vlans'}]},
  ip:          {icon:'🌐', label:'IP',         items:[{label:'Addresses', url:'/ip/addresses',    page:'ip-addresses'},{label:'Routes',    url:'/ip/routes',        page:'ip-routes'},{label:'ARP',       url:'/ip/arp',           page:'ip-arp'},{label:'DHCP Leases',url:'/ip/dhcp-server',  page:'ip-dhcp-leases'},{label:'Pools',     url:'/ip/pools',        page:'ip-pools'},{label:'DNS',       url:'/ip/dns',          page:'ip-dns'},{label:'Remote',    url:'/ip/remote',       page:'ip-remote'}]},
  wireguard:   {icon:'🔒', label:'WireGuard',  items:[{label:'Interfaces',url:'/wireguard/interfaces',page:'wireguard-interfaces'},{label:'Peers',     url:'/wireguard/peers', page:'wireguard-peers'}]},
  ppp:         {icon:'📡', label:'PPP',        items:[{label:'Server',    url:'/ppp/server',   page:'ppp-server'},{label:'Secrets',   url:'/ppp/secrets',    page:'ppp-secrets'},{label:'Active',    url:'/ppp/active',      page:'ppp-active'},{label:'Logs',      url:'/ppp/logs',        page:'ppp-logs'},{label:'RADIUS Auth', url:'/ppp/radius', page:'ppp-radius'}]},
  hotspot:     {icon:'🔥', label:'Hotspot',    items:[{label:'Server',   url:'/hotspot/server', page:'hotspot-server'},{label:'Cookies', url:'/hotspot/cookies', page:'hotspot-cookies'},{label:'Active',url:'/hotspot/active',page:'hotspot-active'},{label:'Logs',url:'/hotspot/logs',page:'hotspot-logs'},{label:'Walled Garden',url:'/hotspot/walled-garden',page:'hotspot-walled-garden'},{label:'IP Bindings',url:'/hotspot/ip-bindings',page:'hotspot-ip-bindings'}]},
  radius:      {icon:'🔐', label:'RADIUS',     items:[{label:'Servers',   url:'/radius/servers', page:'radius-servers'}]},
  firewall:    {icon:'🛡️',label:'Firewall',   items:[{label:'nftables',  url:'/firewall/nftables',page:'firewall-nftables'},{label:'Conntrack',url:'/firewall/conntrack',page:'firewall-conntrack'},{label:'Limits/Log',url:'/firewall/limits',page:'firewall-limit'}]},
  bridge:      {icon:'🔗', label:'Bridge',     items:[{label:'List',      url:'/bridge/list',    page:'bridge-list'},{label:'Ports',     url:'/bridge/ports',    page:'bridge-ports'},{label:'VLANs',     url:'/bridge/vlans',    page:'bridge-vlans'}]},
  system:      {icon:'⚙️', label:'System',     items:[{label:'General',  url:'/system/general', page:'system-identity'},{label:'Users',     url:'/system/users',    page:'system-users'},{label:'Scripts',   url:'/system/scripts',  page:'system-scripts'},{label:'Scheduler', url:'/system/scheduler',page:'system-scheduler'},{label:'Logs',      url:'/system/logs',     page:'system-logs'},{label:'Files',     url:'/system/files',    page:'system-files'}]}
};

// ══════════════════════════════════════════════
// TOP NAV — generado desde PAGES
// ══════════════════════════════════════════════

// No hay toggleDock/hideDock — la topnav siempre visible en desktop
// En móvil se puede ocultar/mostrar via CSS media queries

// ══════════════════════════════════════════════
// NAVEGACIÓN
// ══════════════════════════════════════════════

function sw(key, skipFirst){
  // key = clave en PAGES (ej: 'interfaces')
  var m = PAGES[key];
  if(!m) return;

  // Marcar topnav activo
  document.querySelectorAll('#topnav .di').forEach(function(d){
    d.classList.toggle('active', d.dataset.menu===key)
  });

  // Ocultar dock al seleccionar
  var sn = document.getElementById('subnav-items');
  var items = m.items;
  var currentKey = key;

  // Subnav siempre visible
  sn.style.display = 'flex';
  document.body.classList.add('subnav-visible');

  if(items && items.length > 1){
    // Mostrar submenús reales
    sn.innerHTML = items.map(function(it, i){
      return '<div class="sn" data-url="'+it.url+'"><span class="fl-c'+i%11+'">'+it.label+'</span></div>'
    }).join('');

    // Click handler en subnav (delegación)
    sn.querySelectorAll('.sn').forEach(function(el){
      el.onclick = function(){ lp(el.dataset.url) }
    });
  } else {
    // Sin submenú: mostrar info-bar (hora, online count, etc.)
    sn.innerHTML = '<div class="sn" id="info-bar" style="cursor:default;color:#94a3b8;width:100%;display:flex;gap:1.5rem;align-items:center;padding:0.3rem 0.5rem;border-top:2px solid transparent;flex-shrink:0;font-size:0.8rem">' +
      '<span id="info-clock" style="display:flex;align-items:center;gap:0.4rem">🕐 <span id="clock-value">--:--:--</span></span>' +
      '<span id="info-hotspot" style="display:flex;align-items:center;gap:0.4rem">🔥 <span id="hs-online">--</span> online</span>' +
      '<span id="info-ppp" style="display:flex;align-items:center;gap:0.4rem">📡 <span id="ppp-online">--</span> PPPoE</span>' +
      '<span id="info-macs" style="display:flex;align-items:center;gap:0.4rem;margin-left:auto">🔌 <span id="macs-total">--</span> dispositivos</span>' +
      '</div>';
  }

  // Cargar primer item (si skipFirst es true, ya lp fue llamado)
  if(items && items.length > 0 && !skipFirst) lp(items[0].url);

  // FIX F2: reloj y live-data DESPUES de lp() — lp() limpia los intervals
  // viejos de las paginas; si se iniciaban ANTES (como estaba), lp mataba
  // el reloj recien creado y quedaba congelado para siempre.
  // Solo en dashboard (1-item dock donde tiene sentido mostrar stats en vivo)
  if(key === 'dashboard'){
    startClock();
    simulateLiveData();
  }
}

// 🕐 RELOJ EN VIVO
var clockInterval = null;

function startClock(){
  // FIX F2: reiniciar SIEMPRE — lp() limpia los intervals al navegar y el
  // guard viejo (if clockInterval return) dejaba el reloj congelado.
  if(clockInterval){ clearInterval(clockInterval); clockInterval = null; }
  clockInterval = setInterval(function(){
    var el = document.getElementById('clock-value');
    if(el){
      var now = new Date();
      el.textContent = now.toLocaleTimeString('es-ES', {hour:'2-digit',minute:'2-digit',second:'2-digit'});
    }
  }, 1000);
  // tick inicial
  var now = new Date();
  var el = document.getElementById('clock-value');
  if(el) el.textContent = now.toLocaleTimeString('es-ES', {hour:'2-digit',minute:'2-digit',second:'2-digit'});
}

// 📊 DATOS EN VIVO — FIX F1: datos REALES de la API (antes Math.random)
var liveInterval = null;

function simulateLiveData(){
  if(liveInterval){ clearInterval(liveInterval); liveInterval = null; }
  var update = function(){
    fetch('/api/hotspot/active').then(function(r){ return r.json(); }).then(function(d){
      var el = document.getElementById('hs-online');
      if(el) el.textContent = Array.isArray(d) ? d.length : 0;
    }).catch(function(){});
    fetch('/api/ppp/active').then(function(r){ return r.json(); }).then(function(d){
      var el = document.getElementById('ppp-online');
      if(el) el.textContent = Array.isArray(d) ? d.length : 0;
    }).catch(function(){});
    fetch('/api/arp').then(function(r){ return r.json(); }).then(function(d){
      var el = document.getElementById('macs-total');
      if(el) el.textContent = Array.isArray(d) ? d.length : 0;
    }).catch(function(){});
  };
  update();
  liveInterval = setInterval(update, 10000);
}

function lp(url){
  // Marcar subnav activo
  document.querySelectorAll('#subnav .sn').forEach(function(el){
    if(el.id !== 'info-bar'){
      el.classList.toggle('active', el.dataset.url === url)
    }
  });

  // Buscar qué página cargar
  var page = null;
  var found = false;
  var currentKey = null;
  for(var k in PAGES){
    var items = PAGES[k].items;
    for(var i=0; items && i<items.length; i++){
      if(items[i].url === url){
        page = items[i].page;
        found = true;
        currentKey = k;
        break
      }
    }
    if(found) break
  }

  // Limpiar cualquier timer o auto-refresh de páginas anteriores
  if(window._autoRefreshTimer){
    clearInterval(window._autoRefreshTimer);
    window._autoRefreshTimer = null;
  }
  // Limpiar todos los intervals existentes (por si páginas dejaron timers sueltos)
  var maxId = window.setTimeout(function(){}, 0);
  for(var i = 1; i <= maxId; i++){ clearInterval(i); }

  if(page){
    // Mostrar contenido anterior mientras carga (no borrar hasta tener nuevo)
    fetch('/static/pages/'+page+'.html?_='+Date.now())
      .then(function(r){
        if(!r.ok) throw new Error('page not found');
        return r.text()
      })
      .then(function(html){
        // Doble buffer: construir nueva página oculta
        var newContent = document.createElement('div');
        newContent.id = 'page-content';
        newContent.style.display = 'none';
        newContent.innerHTML = html;
        // Hacer append al DOM real PRIMERO (aún oculto) para que
        // los scripts inline encuentren los elementos con getElementById()
        var content = document.getElementById('content');
        content.innerHTML = '';
        content.appendChild(newContent);
        // AHORA ejecutar scripts inline — el DOM real ya tiene los elementos
        Array.from(newContent.querySelectorAll('script')).forEach(function(s){
          var code = s.textContent;
          try{window.eval.call(window, code)}catch(e){console.error('[Zpot] Error en script de página:', e, 'codigo:', code.substring(0,300))}
        });
        // Envolver tablas ANTES de mostrar para evitar reflow por scrollbar
        wrapTables(true);
        // Mostrar contenido ya completamente renderizado
        newContent.style.display = 'block';
        // Hook para que páginas sepan cuando son cargadas
        if(typeof window.onPageLoad === 'function'){
          try{window.onPageLoad(url)}catch(e){console.error('[Zpot] Error en onPageLoad:', e)}
        }
        // FIX F3: se elimino el bloque _autoRefresh (ninguna pagina lo definia;
        // ademas llamaba cargarPools() siempre, refrescando la pagina equivocada).
        // Inicializar página específica — solo el init de la página actual
        var pageInits = {
          'dashboard': 'cargarDashboard',
          'ppp-secrets': '_initPppSecrets',
          'ppp-active':   '_initPppActive',
          'ppp-server':  'cargarPppServer',
          'ppp-logs': 'cargarPppLogs',
          'ppp-radius': 'cargarPppRadius',
          'ip-addresses': '_initIPAddrs',
          'ip-pools': 'cargarPools',
          'ip-routes': 'cargarRoutes',
          'ip-arp': 'cargarArp',
          'ip-dhcp-leases': 'cargarDhcpLeases',
          'ip-dns': 'cargarDns',
          'interfaces': 'cargarInterfaces',
          'interfaces-vlans': 'cargarVlans',
          'wireguard-interfaces': 'cargarWireguard',
          'wireguard-peers': 'cargarPeers',
          'system-identity': 'cargarSystem',
          'system-users': 'cargarUsers',
          'system-scripts': 'cargarScripts',
          'system-scheduler': 'cargarScheduler',
          'system-logs': 'cargarLogs',
          'system-files': 'cargarFiles',
          'routing-mwan': 'cargarMwan',
          'firewall-nftables': 'cargarNftDash',
          'firewall-conntrack': 'cargarConntrack',
          'firewall-limit': 'cargarLimit',
          'bridge-list': 'cargarBridges',
          'bridge-ports': 'cargarBridgePorts',
          'bridge-vlans': 'cargarBridgeVlans',
          'hotspot-active': 'cargarHotspotActive',
          'hotspot-logs': 'cargarHotspotLogs',
          'hotspot-walled-garden': 'cargarWalledGarden',
          'hotspot-ip-bindings': 'cargarIpBindings',
          'hotspot-cookies': 'cargarCookies',
          'radius-servers': 'cargarRadiusServers',
        };
        var initFn = pageInits[page];
        if(initFn && typeof window[initFn] === 'function'){
          try{ window[initFn](); }catch(e){ console.error('[Zpot] Error en init de '+page+':', e); }
        }
        // También correr cargarX() inline si el script de la página lo llama directamente
        // (wrapTables extra vía requestAnimationFrame para tablas dinámicas futuras)
        requestAnimationFrame(function(){ wrapTables(); });
      })
      .catch(function(e){
        console.error('[Zpot] Error cargando pagina:', url, e);
        // Fallback: si no hay página, mostrar placeholder
        showPlaceholder(url);
      });
  } else {
    showPlaceholder(url);
  }

  window.history.pushState(null, '', url);
}

function showPlaceholder(url){
  if(typeof url === 'number'){
    var labels = ['dashboard','interfaces','ip','wireguard','ppp','hotspot','radius','firewall','bridge','system'];
    url = labels[url] || 'dashboard';
  }
  var label = url.split('/').filter(Boolean).join(' / ') || 'Dashboard';
  document.getElementById('content').innerHTML =
    '<div class="card fade-in"><h3>'+label+'</h3>'+
    '<p style="color:#64748b;padding:1rem 0">⏳ Contenido en desarrollo<br>'+
    '<span style="font-size:0.75rem">Ruta: '+url+'</span></p></div>';
}

// ══════════════════════════════════════════════
// INICIALIZACIÓN
// ══════════════════════════════════════════════

function init(){
  // Generar topnav desde PAGES
  var nav = document.getElementById('topnav');
  var html = '';
  var firstKey = null;
  var ci = 0;
  for(var k in PAGES){
    if(!firstKey) firstKey = k;
    var m = PAGES[k];
    html += '<div class="di" data-menu="'+k+'" onclick="sw(\''+k+'\')">'+
      '<span class="ic">'+m.icon+'</span>'+
      '<span class="lb fl-c'+ci%11+'">'+m.label+'</span></div>';
    ci++;
  }
  nav.innerHTML = html;
  // Si hay una URL en pathname, cargar desde ahí
  var p = window.location.pathname;
  if(p && p !== '/'){
    for(var k in PAGES){
      var items = PAGES[k].items;
      for(var i=0; items && i<items.length; i++){
        if(items[i].url === p){
          lp(p);
          sw(k, true);
          return
        }
      }
    }
  }
  // Cargar página inicial
  sw(firstKey);
}

// ══════════════════════════════════════════════
// HANDLER popstate (navegación atrás/adelante)
// ══════════════════════════════════════════════

window.addEventListener('popstate', function(){
  var p = window.location.pathname;
  // Buscar qué dock activar basado en la URL
  for(var k in PAGES){
    var items = PAGES[k].items;
    for(var i=0; items && i<items.length; i++){
      if(items[i].url === p){
        sw(k);
        return
      }
    }
  }
  sw('dashboard');
});

// ══════════════════════════════════════════════
// MOCK DATA compartida
// ══════════════════════════════════════════════

function fmtBytes(n){
  if(!n||n===0)return'0 B';
  var u=['B','KB','MB','GB','TB'];
  var i=0;var v=n;
  while(v>=1024&&i<u.length-1){v/=1024;i++}
  return v.toFixed(i>=2?2:0)+' '+u[i];
}

// ══════════════════════════════════════════════
// Interfaces — datos reales de la API (FIX F4: se eliminaron mockIfs/mockAddrs
// con interfaces/IPs inventadas que se mostraban como fallback)
// ══════════════════════════════════════════════

// Live speed: bytes previos para calcular delta
var prevBytes = {};
var prevTime = Date.now();

function fmtSpeed(n){
  if(n < 0)return '—';
  if(!n)return '0 bps';
  var bps = n * 8;
  var u = ['bps','Kbps','Mbps','Gbps'];
  var i = 0; var v = bps;
  while(v >= 1000 && i < u.length-1){ v /= 1000; i++; }
  return (i >= 2 ? v.toFixed(1) : v.toFixed(0)) + ' ' + u[i];
}

function renderIfs(ifs){
    var t=document.getElementById('interfaces-tbody');
    if(!t)return;
    var now = Date.now();
    var dt = now - prevTime; // ms desde ultima medicion
    if(dt < 100) dt = 1000;
    var h='';
    for(var i=0;i<ifs.length;i++){
      var x=ifs[i];
      var st;
      if(x.state==='up') st='<span class="badge badge-up">● up</span>';
      else if(x.state==='unknown') st='<span class="badge badge-unknown">● unknown</span>';
      else st='<span class="badge badge-down">● down</span>';
      var mac = x.mac && x.mac !== '' ? x.mac : '—';
      // Calcular velocidad live
      var txSpeed = 0, rxSpeed = 0;
      if(x.state === 'up' && prevBytes[x.name]){
        txSpeed = (x.tx_bytes - (prevBytes[x.name].tx || 0)) / (dt / 1000);
        rxSpeed = (x.rx_bytes - (prevBytes[x.name].rx || 0)) / (dt / 1000);
        if(txSpeed < 0) txSpeed = 0;
        if(rxSpeed < 0) rxSpeed = 0;
      } else if(x.state === 'up' && !prevBytes[x.name]) {
        // Primera iteracion — 0 bps hasta el proximo poll
        txSpeed = 0;
        rxSpeed = 0;
      }
      h+='<tr>'+
        '<td style="font-weight:600;color:#0f172a">'+x.name+'</td>'+
        '<td style="font-family:monospace;font-size:0.8rem;color:#475569">'+mac+'</td>'+
        '<td>'+st+'</td>'+
        '<td style="text-align:right;color:#2563eb;font-family:monospace;font-size:0.85rem;font-weight:600;font-variant-numeric:tabular-nums">'+fmtSpeed(txSpeed)+'</td>'+
        '<td style="text-align:right;color:#16a34a;font-family:monospace;font-size:0.85rem;font-weight:600;font-variant-numeric:tabular-nums">'+fmtSpeed(rxSpeed)+'</td>'+
      '</tr>';
    }
    t.innerHTML=h;
    // Guardar snapshot para proxima iteracion
    prevBytes = {};
    for(var i=0;i<ifs.length;i++){
      prevBytes[ifs[i].name] = {rx: ifs[i].rx_bytes, tx: ifs[i].tx_bytes};
    }
    prevTime = now;
  }

// FIX F4: eliminado mockAddrs (mostraba IPs/interfaces inventadas como
// fallback). Ahora el error se muestra en la tabla, sin datos falsos.
function renderAddrs(addrs){
  var t=document.getElementById('addresses-tbody');
  if(!t)return;
  var h='';
  for(var i=0;i<addrs.length;i++){
    var x=addrs[i];
    var st=x.state==='up'?'<span class="badge badge-up">● up</span>':'<span class="badge badge-down">● down</span>';
    var dyn=x.dynamic?'<span style="color:#d97706">Sí</span>':'<span style="color:#64748b">No</span>';
    h+='<tr>'+
      '<td style="font-weight:600;color:#0f172a;font-family:monospace;font-size:0.85rem">'+x.address+'</td>'+
      '<td style="font-family:monospace;font-size:0.8rem;color:#475569">'+x.network+'</td>'+
      '<td style="color:#2563eb;font-weight:500">'+x.iface+'</td>'+
      '<td>'+st+'</td>'+
      '<td>'+dyn+'</td>'+
      '<td style="color:#475569;font-size:0.8rem">'+x.description+'</td>'+
      '<td><button class="btn btn-sm btn-danger" onclick="deleteAddress(\''+x.iface+'\',\''+x.address+'\')">🗑</button></td>'+
    '</tr>';
  }
  t.innerHTML=h;
}

function addAddress(){
  alert('Add Address — formulario en desarrollo');
}

function deleteAddress(iface, addr){
  if(!confirm('¿Eliminar dirección '+addr+'?'))return;
  // Ruta real del backend: /api/ip-addresses/:iface/:addr (sin /delete)
  fetch('/api/ip-addresses/'+encodeURIComponent(iface)+'/'+encodeURIComponent(addr), {method:'DELETE'})
    .then(function(r){ if(!r.ok) throw new Error('HTTP '+r.status); return r.json(); })
    .then(function(){ cargarAddresses(); })
    .catch(function(e){ alert('Error: '+e.message); });
}

async function cargarAddresses(){
  try{
    var r=await fetch('/api/ip-addresses');
    if(!r.ok)throw new Error('HTTP '+r.status);
    var d=await r.json();
    var list = Array.isArray(d) ? d : (d.addresses || []);
    if(list.length>0){
      renderAddrs(list);
    }else{throw new Error('Sin datos')}
  }catch(e){
    // FIX F4: no inventar datos — mostrar error en la tabla
    var t = document.getElementById('addresses-tbody');
    if(t) t.innerHTML = '<tr><td colspan="7" class="page-empty">Error al cargar direcciones</td></tr>';
  }
}

async function cargarInterfaces(){
  var t=document.getElementById('interfaces-tbody');
  if(!t)return;
  // Auto-polling cada 2s para velocidad live
  if(window.__ifInterval) clearInterval(window.__ifInterval);
  window.__ifInterval = setInterval(_fetchInterfaces, 2000);
  _fetchInterfaces(); // tick inicial
}

async function _fetchInterfaces(){
  try{
    var r=await fetch('/api/interfaces');
    if(!r.ok)throw new Error('HTTP '+r.status);
    var d=await r.json();
    var list = Array.isArray(d) ? d : (d.interfaces || []);
    if(list.length>0){
      renderIfs(list);
    }
  }catch(e){
    console.error('[Zpot] Error interfaces:', e);
  }
}

// ══════════════════════════════════════════════
// TABLAS RESPONSIVAS — envolver en wrapper scrollable
// ══════════════════════════════════════════════
function wrapTables(immediate){
  if(immediate){
    document.getElementById('content').querySelectorAll('table').forEach(function(t){
      if(!t.parentElement.classList.contains('table-wrap')){
        var w = document.createElement('div');
        w.className = 'table-wrap';
        t.parentNode.insertBefore(w, t);
        w.appendChild(t);
      }
    });
  } else {
    setTimeout(function(){
      document.getElementById('content').querySelectorAll('table').forEach(function(t){
        if(!t.parentElement.classList.contains('table-wrap')){
          var w = document.createElement('div');
          w.className = 'table-wrap';
          t.parentNode.insertBefore(w, t);
          w.appendChild(t);
        }
      });
    }, 50);
  }
}
