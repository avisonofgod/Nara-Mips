#!/bin/sh
# verify-after-reboot.sh — Verificacion COMPLETA post-reboot de Zpot-RS (Alpine)
# Uso: ssh root@10.7.0.5 "sh /usr/local/bin/verify-after-reboot.sh"
# Devuelve [OK]/[FAIL] por cada check + resumen. Exit 0 si todo OK.
# 2026-08-04 — cubre: sistema, zpot, WG, red, nft, hotspot, PPP,
# balanceo WAN/MWAN, servicios, logs, watchdog.

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "[OK]   $1"; }
bad()  { FAIL=$((FAIL+1)); echo "[FAIL] $1"; }
check() { # check <desc> <cmd...>  -> exit 0 = OK
  d="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi
}

echo "═══════════ VERIFICACION POST-REBOOT $(date -Is) ═══════════"

echo "── A. SISTEMA ──"
ok "uptime: $(uptime | sed 's/^ *//')"
check "ip_forward=1" sh -c 'test "$(cat /proc/sys/net/ipv4/ip_forward)" = "1"'
check "memoria OK" sh -c 'free -m | awk "NR==2 {exit (\$7 < 100000) ? 0 : 1}"'

echo "── B. ZPOT ──"
ZPID=$(ss -tlnp 2>/dev/null | grep 8081 | grep -o "pid=[0-9]*" | head -1 | cut -d= -f2)
[ -n "$ZPID" ] && ok "zpot proceso vivo (pid=$ZPID)" || bad "zpot proceso NO vivo"
check "admin :8081 = 200" sh -c 'curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 http://localhost:8081/ | grep -q 200'
check "portal :80 = 302" sh -c 'curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 http://localhost:80/ | grep -q 302'
check "cola accept 0/128" sh -c 'ss -lnt | grep -E ":8081 " | grep -q "0      128"'
check "watchdog en cron" sh -c 'crontab -l | grep -q zpot-watchdog'
check "zpot-watchdog.sh instalado" sh -c 'test -x /usr/local/bin/zpot-watchdog.sh'

echo "── C. WIREGUARD ──"
check "WG ping 10.7.0.1" sh -c 'ping -c1 -W2 10.7.0.1 >/dev/null 2>&1'
check "wg0 up" sh -c 'ip link show wg0 | grep -q "UP"'
check "ruta 10.7.0.0/24 dev wg0" sh -c 'ip route show | grep -q "10.7.0.0/24 dev wg0"'
check "nft mwan protege WG (iif wg0 accept)" sh -c 'nft list chain inet mwan prerouting 2>/dev/null | grep -q "iif \"wg0\" ip daddr 10.7.0.0/24 accept"'

echo "── D. RED / INTERFACES ──"
check "eth0 192.168.2.102" sh -c 'ip -o addr show eth0 | grep -q "192.168.2.102"'
check "eth1 192.168.3.105" sh -c 'ip -o addr show eth1 | grep -q "192.168.3.105"'
check "eth3 192.168.10.1" sh -c 'ip -o addr show eth3 | grep -q "192.168.10.1"'
check "eth3.881 up" sh -c 'ip link show eth3.881 | grep -q "UP"'
check "wg0 10.7.0.5" sh -c 'ip -o addr show wg0 | grep -q "10.7.0.5"'

echo "── E. NFTABLES ──"
check "tabla inet hotspot" sh -c 'nft list tables 2>/dev/null | grep -q "inet hotspot"'
check "tabla inet mwan" sh -c 'nft list tables 2>/dev/null | grep -q "inet mwan"'
check "chain input (FIX-9 admin 8081)" sh -c 'nft list chain inet hotspot input 2>/dev/null | grep -q "tcp dport 8081 drop"'
N=$(nft list set inet hotspot hotspot_auth 2>/dev/null | grep -c "expires")
[ "$N" -ge 0 ] 2>/dev/null; ok "set hotspot_auth: $N elementos"

echo "── F. HOTSPOT ──"
H=$(curl -s --connect-timeout 5 http://localhost:8081/api/hotspot/active 2>/dev/null | grep -o '"username"' | wc -l)
ok "sesiones activas API: $H"
C=$(curl -s --connect-timeout 5 http://localhost:8081/api/hotspot/cookies 2>/dev/null | grep -o '"username"' | wc -l)
ok "cookies API: $C"
check "QoS tc eth3 (clases htb)" sh -c 'tc class show dev eth3 2>/dev/null | grep -q "class htb"'
check "ifb_eth3 qdisc" sh -c 'tc qdisc show dev ifb_eth3 2>/dev/null | grep -q "htb"'

echo "── G. PPP ──"
check "pppoe-server vivo" sh -c 'pgrep -f "[p]ppoe-server -I" >/dev/null'
P=$(pgrep -x pppd 2>/dev/null | wc -l)
ok "pppd vivos: $P"
M=$(ls /var/run/ppp-mac-* 2>/dev/null | wc -l)
ok "ppp-mac files: $M"
S=$(curl -s --connect-timeout 5 http://localhost:8081/api/ppp/secrets 2>/dev/null | grep -o '"username"' | wc -l)
ok "secrets API: $S"
check "chap-secrets (>=33)" sh -c 'test "$(grep -c . /etc/ppp/chap-secrets 2>/dev/null)" -ge 34'
check "/dev/ppp existe" sh -c 'test -c /dev/ppp'

echo "── H. BALANCEO WAN / MWAN ──"
MW=$(curl -s --connect-timeout 5 http://localhost:8081/api/mwan/status 2>/dev/null)
echo "  $MW" | head -c 250; echo
NUP=$(echo "$MW" | grep -o '"status":"up"' | wc -l)
[ "$NUP" -ge 2 ] && ok "mwan: $NUP WANs up" || bad "mwan: solo $NUP WANs up (esperado 2)"
check "ip rule 1401 (wan1)" sh -c 'ip rule show | grep -q "1401:.*fwmark 0x1 lookup wan1"'
check "ip rule 1402 (wan2)" sh -c 'ip rule show | grep -q "1402:.*fwmark 0x2 lookup wan2"'
check "tabla wan1 default eth0" sh -c 'ip route show table wan1 2>/dev/null | grep -q "default via 192.168.2.1 dev eth0"'
check "tabla wan2 default eth1" sh -c 'ip route show table wan2 2>/dev/null | grep -q "default via 192.168.3.1 dev eth1"'
check "ruta main default" sh -c 'ip route show | grep -q "^default"'
check "nft mwan mark jhash" sh -c 'nft list chain inet mwan prerouting 2>/dev/null | grep -q "jhash ip saddr mod 2"'
W1=$(conntrack -L 2>/dev/null | grep -c "src=192.168.20.*mark=1")
W2=$(conntrack -L 2>/dev/null | grep -c "src=192.168.20.*mark=2")
ok "balanceo PPP real: clientes ppp wan1(mark=1)=$W1 wan2(mark=2)=$W2"
if [ "$W1" -gt 0 ] && [ "$W2" -gt 0 ]; then ok "distribucion PPP en AMBAS WANs (balanceo activo)"; else bad "PPP solo en una WAN (revisar MWAN)"; fi

echo "── I. SERVICIOS ──"
# NOTA: pgrep de BusyBox falla para daemons (/usr/sbin/*) — usar ps | grep
check "dnsmasq" sh -c 'ps w | grep -q "[d]nsmasq"'
check "ntpd" sh -c 'ps w | grep -q "[n]tpd -N"'
check "crond" sh -c 'ps w | grep -q "[c]rond -c /etc/crontabs"'
check "mwan-agent" sh -c 'ps w | grep -q "[m]wan-agent"'

echo "── J. LOGS ──"
if [ -f /var/log/zpot-reboot.log ]; then
  tail -3 /var/log/zpot-reboot.log | sed "s/^/  /"
  ok "zpot-reboot.log existe"
else
  bad "zpot-reboot.log no existe (no se reinicio por panel)"
fi
E=$(grep -c "panic\|ERROR\|FATAL" /tmp/zpot.log 2>/dev/null)
[ "$E" -gt 0 ] 2>/dev/null && bad "errores/panics en /tmp/zpot.log: $E" || ok "sin panics/errores en /tmp/zpot.log"

echo "═══════════════════════════════════════════════════════════"
echo "RESULTADO: $PASS OK, $FAIL FAIL"
[ "$FAIL" -eq 0 ] && echo "✅ SISTEMA COMPLETO" || echo "⚠️  REVISAR LOS FAIL"
exit $FAIL
