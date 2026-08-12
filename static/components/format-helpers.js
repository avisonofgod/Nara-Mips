// ─── Format Helpers — funciones compartidas entre páginas ───
// Extraídas de páginas HTML. Cargado desde base.html como script global.
// NO DEPENDEN de datos ni DOM de páginas específicas.
// Se asignan a window para compatibilidad con eval() inline de páginas en SPA.

function pad(n){ return n < 10 ? '0' + n : '' + n }
window.pad = pad;

function delayStr(delay) {
  return delay + 'ms';
}
window.delayStr = delayStr;

function offsetColor(offset) {
  if (Math.abs(offset) <= 1) return 'var(--accent-green)';
  if (Math.abs(offset) <= 5) return 'var(--accent-amber)';
  return 'var(--accent-red)';
}
window.offsetColor = offsetColor;

function offsetStr(offset) {
  var prefix = offset > 0 ? '+' : '';
  return prefix + offset + 'ms';
}
window.offsetStr = offsetStr;

function lastLoginColor(login) {
  if (login === 'never') return 'var(--clr-text-light)';
  if (login.indexOf('min') !== -1) return 'var(--accent-green)';
  if (login.indexOf('h') !== -1) return 'var(--accent-amber)';
  return 'var(--accent-red)';
}
window.lastLoginColor = lastLoginColor;

function lastModifiedColor(dateStr) {
  if (dateStr.indexOf('hace') !== -1) return 'var(--accent-amber)';
  if (dateStr.indexOf('min') !== -1) return 'var(--accent-green)';
  return 'var(--clr-text-light)';
}
window.lastModifiedColor = lastModifiedColor;

function modeColor(m){
  return {'balance-rr':'var(--accent-amber)','active-backup':'var(--accent-green)','802.3ad':'var(--accent-blue)','balance-xor':'var(--accent-purple)','broadcast':'var(--accent-red)'}[m]||'var(--clr-text-light)';
}
window.modeColor = modeColor;

function modeLabel(m){
  return {'balance-rr':'balance-rr','active-backup':'active-backup','802.3ad':'802.3ad','balance-xor':'balance-xor','broadcast':'broadcast'}[m]||m;
}
window.modeLabel = modeLabel;

function getProtoStyle(proto) {
  var c = proto === 'arp' ? 'var(--accent-amber)' : (proto === 'ipv4' ? 'var(--accent-green)' : (proto === 'ipv6' ? 'var(--accent-purple)' : 'var(--clr-text-light)'));
  return 'style="color:'+c+';font-family:monospace;font-weight:600"';
}
window.getProtoStyle = getProtoStyle;

function truncateRegex(str, max){
  if(!str) return '—';
  if(str.length <= max) return str;
  return str.substring(0, max) + '…';
}
window.truncateRegex = truncateRegex;

function fmtRadiusBytes(val){
  if(val === 0 || val === null || val === undefined) return '<span style="color:#475569">—</span>';
  if(val >= 1024) return (val/1024).toFixed(1) + ' TB';
  if(val >= 1) return val.toFixed(1) + ' GB';
  return (val * 1024).toFixed(1) + ' MB';
}
window.fmtRadiusBytes = fmtRadiusBytes;

function fmtTraffic(n) {
  if (!n || n === 0) return '0 B';
  var u = ['B','KB','MB','GB','TB'];
  var i = 0;
  var v = n;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return v.toFixed(i >= 1 ? 1 : 0) + ' ' + u[i];
}
window.fmtTraffic = fmtTraffic;

function maxTraffic(tunnels) {
  var max = 0;
  for (var i = 0; i < tunnels.length; i++) {
    var t = tunnels[i];
    var total = t.rx + t.tx;
    if (total > max) max = total;
  }
  return max || 1;
}
window.maxTraffic = maxTraffic;

function formatFileSize(bytes) {
  return fmtBytes(bytes);
}
window.formatFileSize = formatFileSize;

function preferBadgeHtml(prefer) {
  if (prefer) return '<span class="badge badge-up">★ Sí</span>';
  return '<span class="badge" style="background:#f1f5f9;color:#475569">— No</span>';
}
window.preferBadgeHtml = preferBadgeHtml;

function disabledBadgeHtml(disabled) {
  if (disabled) return '<span class="badge badge-down">Sí</span>';
  return '<span class="badge badge-up">No</span>';
}
window.disabledBadgeHtml = disabledBadgeHtml;

function groupBadgeHtml(group) {
  var cls = group === 'full' ? 'badge badge-perm' : group === 'write' ? 'badge badge-info' : 'badge';
  return '<span class="'+cls+'">'+group+'</span>';
}
window.groupBadgeHtml = groupBadgeHtml;

function permissionsBadgeHtml(perms) {
  var parts = perms.split(',');
  var h = '';
  for (var i = 0; i < parts.length; i++) {
    var p = parts[i].trim();
    var cls = p === 'execute' ? 'badge badge-warn' : p === 'write' ? 'badge badge-info' : 'badge';
    h += '<span class="'+cls+'" style="margin-right:0.25rem">'+p+'</span> ';
  }
  return h;
}
window.permissionsBadgeHtml = permissionsBadgeHtml;

function ownerBadgeHtml(owner) {
  var cls = owner === 'admin' ? 'badge badge-perm' : owner === 'operador' ? 'badge badge-info' : owner === 'system' ? 'badge badge-warn' : 'badge';
  return '<span class="'+cls+'">'+owner+'</span>';
}
window.ownerBadgeHtml = ownerBadgeHtml;

function policyBadgeHtml(policy) {
  var cls = policy === 'read' ? 'badge badge-up' : policy === 'write' ? 'badge badge-warn' : policy === 'test' ? 'badge badge-perm' : policy === 'api' ? 'badge badge-info' : 'badge';
  return '<span class="'+cls+'">'+policy+'</span>';
}
window.policyBadgeHtml = policyBadgeHtml;

function jobTypeBadgeHtml(jobType) {
  var jt = jobType.toLowerCase();
  if (jt === 'script') return '<span class="badge badge-up">📜 Script</span>';
  if (jt === 'log')    return '<span class="badge badge-warn">📋 Log</span>';
  return '<span class="badge">'+jobType+'</span>';
}
window.jobTypeBadgeHtml = jobTypeBadgeHtml;

function schedulerStatusBadgeHtml(status) {
  if (status === 'running') return '<span class="badge badge-up">● running</span>';
  if (status === 'idle')    return '<span class="badge" style="background:#f1f5f9;color:#475569">💤 idle</span>';
  return '<span class="badge" style="background:#e2e8f0;color:#64748b">'+status+'</span>';
}
window.schedulerStatusBadgeHtml = schedulerStatusBadgeHtml;

function statusBadgeHtml(status) {
  if (status === 'reachable')   return '<span class="badge badge-up">✅ reachable</span>';
  if (status === 'unreachable') return '<span class="badge badge-down">❌ unreachable</span>';
  return '<span class="badge" style="background:#f1f5f9;color:#475569">⚪ '+status+'</span>';
}
window.statusBadgeHtml = statusBadgeHtml;

function typeBadgeHtml(type) {
  var cls = type === '.rsc' ? 'badge badge-info' : type === '.txt' ? 'badge' : type === '.log' ? 'badge badge-warn' : type === '.backup' ? 'badge badge-up' : type === '.conf' ? 'badge badge-perm' : type === '.script' ? 'badge badge-down' : 'badge';
  return '<span class="'+cls+'">' + type + '</span>';
}
window.typeBadgeHtml = typeBadgeHtml;

// ─── Fin de format helpers ───
