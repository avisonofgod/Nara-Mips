#!/bin/sh
# ppp-zombie-watchdog.sh — Elimina interfaces PPP zombies
# Zpot-RS — se ejecuta via cron cada 2 minutos
#
# Zombie REAL = interfaz PPP sin trafico (TX=107 = solo LCP keepalive),
# SIN pppd vivo asociado Y con mas de MIN_AGE segundos de vida.
#
# FIX 2026-07-31 (bug critico): el watchdog mataba clientes recien conectados.
# Un cliente que acaba de autenticar tiene TX=107 temporalmente (~80s mientras
# su router establece la sesion y antes del primer paquete de datos), pero su
# pppd ESTA VIVO. Proteccion doble:
#   1. pppd VIVO asociado al peer -> sesion real, conservar
#   2. Edad de la interfaz < MIN_AGE (5 min) -> recien creada, conservar
# Un zombie genuino (kernel circular dep, ip-down no ejecutado) tiene pppd
# muerto Y la interfaz persiste horas.
#
# FIX 2026-08-01 v2 (bug critico): identificar la sesion REAL por RX ACTIVO,
# no por TX acumulado. Cuando un router se reconecta, la sesion vieja conserva
# el TX historico (GBs) y la nueva (real) tiene TX=107 -> el watchdog eliminaba
# la sesion REAL y conservaba la fantasma, dejando al cliente sin internet en
# loop infinito (03:48 elimino ppp36 real de 2420@MoisesKaren@Huayal, se
# reconecto como ppp37, y el ciclo se repetiria cada 5 min).
# Criterio correcto: la sesion REAL es la que RECIBE datos del cliente (RX
# creciente en muestreo de 2s). La fantasma tiene RX congelado (el peer ya no
# envia por ahi). Si todas estan inactivas, conservar la mas joven (la
# reconexion reciente es la real).

PPP_BASE=/sys/class/net
LOG_TAG=ppp-watchdog
MIN_AGE=300   # 5 minutos (segundos)

# Devuelve 0 (true) si existe un pppd VIVO cuyo cmdline mencione la MAC
# del peer (remotenumber). La MAC es estable y unica por CPE.
# FIX 2026-08-01 v3 (bug critico): ANTES se buscaba la IP FINAL del peer en
# el cmdline, pero el cmdline de pppd contiene la IP PROVISIONAL del pool
# (-R .100-.200), NO la final -> la coincidencia fallaba SIEMPRE -> el
# watchdog mataba clientes VIVOS (tx=107) en loop infinito cada ~8 min
# (FidencioRivera 18:22, MelitoH 18:28). La MAC viene de /var/run/ppp-mac-$ppp
# escrita por ip-up ($6 = calling number).
pppd_alive_for() {
  ppp="$1"
  [ -z "$ppp" ] && return 1
  mac=$(cat "/var/run/ppp-mac-$ppp" 2>/dev/null | tr -d ' \n')
  # Sin archivo MAC no podemos verificar -> conservar (NO matar).
  # Un cliente vivo es peor de matar que un zombie temporal; el zombie
  # real se limpia cuando su interfaz reconecte y genere ppp-mac.
  [ -z "$mac" ] && return 0
  for pid in $(pgrep -x pppd 2>/dev/null); do
    # OJO: /proc/PID/cmdline separa args con \0 -> convertir a espacios
    # antes de grep (FIX 2026-08-02: sin tr, la coincidencia fallaba
    # SIEMPRE y el watchdog conservaba zombies).
    if grep -q "remotenumber $mac" <(tr '\0' ' ' < /proc/$pid/cmdline) 2>/dev/null; then
      return 0
    fi
  done
  return 1
}

# Devuelve 0 (true) si la interfaz tiene menos de MIN_AGE segundos
# (recien creada = cliente recien conectado, no matar)
iface_young() {
  ppp="$1"
  # mtime del directorio sysfs ~ momento de creacion del netdev
  created=$(stat -c %Y "$PPP_BASE/$ppp" 2>/dev/null || echo 0)
  [ "$created" = "0" ] && return 0  # no se puede determinar -> conservador, no matar
  now=$(date +%s)
  age=$(( now - created ))
  [ "$age" -lt "$MIN_AGE" ]
}

# Muestrea RX de una lista de interfaces: devuelve las que tienen RX creciente
# (delta > 0 en ~2s) = el peer envia datos por ahi = sesion REAL.
# Uso: rx_active=$(rx_active_ifaces "ppp21 ppp37")  -> "ppp37"
rx_active_ifaces() {
  ifaces="$1"
  # Primera lectura de todas
  rx1_map=""
  for ppp in $ifaces; do
    rx1_map="$rx1_map $ppp:$(cat "$PPP_BASE/$ppp/statistics/rx_bytes" 2>/dev/null || echo 0)"
  done
  sleep 2
  # Segunda lectura y comparacion
  active=""
  for entry in $rx1_map; do
    ppp=${entry%%:*}
    rx1=${entry##*:}
    rx2=$(cat "$PPP_BASE/$ppp/statistics/rx_bytes" 2>/dev/null || echo 0)
    delta=$(( rx2 - rx1 ))
    if [ "$delta" -gt 0 ]; then
      active="$active $ppp"
      logger -t "$LOG_TAG" "  RX-ACTIVO: $ppp delta_rx=$delta (sesion real — peer envia datos)"
    fi
  done
  echo "$active"
}

# Elimina una interfaz PPP y registra el resultado
delete_ppp() {
  ppp="$1"
  STDERR=$(ip link delete dev "$ppp" 2>&1)
  EXITCODE=$?
  if [ "$EXITCODE" -eq 0 ]; then
    logger -t "$LOG_TAG" "    $ppp eliminada OK"
  else
    logger -t "$LOG_TAG" "    ERROR al eliminar $ppp: $STDERR"
  fi
}

# === PASO 1: interfaces con TX=107 ===
for ppp in $(ls -d "$PPP_BASE"/ppp* 2>/dev/null | sed 's|.*/||'); do
  # Saltar si no es interfaz PPP
  [ "${ppp#ppp}" = "$ppp" ] && continue

  # Obtener estadisticas TX
  tx=$(cat "$PPP_BASE/$ppp/statistics/tx_bytes" 2>/dev/null)
  [ -z "$tx" ] && continue

  # TX exactamente 107 = 8 paquetes LCP = solo keepalive, sin trafico
  [ "$tx" != "107" ] && continue

  # Obtener peer IP y username para logging
  peer=$(ip addr show "$ppp" 2>/dev/null | grep 'peer' | awk '{print $4}' | sed 's/\/32//')
  [ -z "$peer" ] && continue
  user=$(grep "$peer" /etc/ppp/chap-secrets 2>/dev/null | awk '{print $1}')
  [ -z "$user" ] && user="desconocido"

  # PROTECCION 0: RX activo -> el peer envia datos (sesion real, no zombie)
  active=$(rx_active_ifaces "$ppp")
  if [ -n "$active" ]; then
    logger -t "$LOG_TAG" "OK: $ppp peer=$peer user=$user tx=$tx (RX activo — sesion real, conservando)"
    continue
  fi

  # PROTECCION 1: si hay pppd VIVO asociado (por MAC), la sesion es real
  if pppd_alive_for "$ppp"; then
    logger -t "$LOG_TAG" "OK: $ppp peer=$peer user=$user tx=$tx (pppd vivo — sesion activa, conservando)"
    continue
  fi

  # PROTECCION 2: si la interfaz es joven (<5 min), es un cliente recien
  # conectado que aun no genera trafico. NO es zombie.
  if iface_young "$ppp"; then
    logger -t "$LOG_TAG" "OK: $ppp peer=$peer user=$user tx=$tx (interfaz joven — recien conectado, conservando)"
    continue
  fi

  logger -t "$LOG_TAG" "ZOMBIE: $ppp peer=$peer user=$user tx=$tx (sin pppd + >5min) — eliminando"

  # Eliminar interfaz zombie (el pppd ya no existe; no hay nada que matar)
  delete_ppp "$ppp"
done

# === PASO 2: Detectar IPs duplicadas entre interfaces PPP ===
# FIX 2026-08-01 (bug): pppd_alive_for($peer) era GLOBAL — si un pppd vivo
# tenia la IP en su cmdline, TODAS las interfaces con ese peer se conservaban
# (incluidas las fantasma TX=107). Ademas el cmdline de pppd contiene la IP
# PROVISIONAL del pool (-R 192.168.20.2), NO la IP final del peer, por lo que
# la coincidencia era casi aleatoria y las duplicadas nunca se limpiaban.
#
# FIX 2026-08-01 v2 (bug critico): la sesion REAL se identifica por RX ACTIVO
# (el peer envia datos por esa interfaz AHORA), no por TX acumulado. La sesion
# vieja de una reconexion conserva GBs de TX historico y engana al criterio
# "mayor TX". Regla: conservar las interfaces con RX creciente; si todas estan
# inactivas, conservar la mas joven (la reconexion reciente es la real) y
# eliminar las viejas TX<=107 (fantasma) cuya edad supere MIN_AGE.

for peer_ip in $(ip -br addr show type ppp 2>/dev/null | \
  awk '{print $NF}' | sed 's/peer //' | sed 's/\/32//' | \
  sort | uniq -d); do

  logger -t "$LOG_TAG" "IP DUPLICADA: $peer_ip — buscando duplicados..."

  # Listar interfaces con este peer
  dup_ifaces=""
  for ppp in $(ip -br addr show type ppp 2>/dev/null | grep "peer $peer_ip" | awk '{print $1}'); do
    dup_ifaces="$dup_ifaces $ppp"
  done

  # Muestrear RX de todas: las que tengan delta > 0 son sesiones REALES
  rx_active=$(rx_active_ifaces "$dup_ifaces")

  if [ -n "$rx_active" ]; then
    # Hay sesion(es) real(es) con RX activo -> conservar SOLO esas,
    # eliminar las demas (fantasma con RX congelado) si son viejas
    for ppp in $dup_ifaces; do
      # Conservar las activas
      case " $rx_active " in
        *" $ppp "*) continue ;;
      esac
      # Proteccion: interfaz joven -> recien conectada, no matar
      if iface_young "$ppp"; then
        logger -t "$LOG_TAG" "  KEEP: $ppp peer=$peer_ip (interfaz joven)"
        continue
      fi
      user=$(grep "$peer_ip" /etc/ppp/chap-secrets 2>/dev/null | awk '{print $1}')
      logger -t "$LOG_TAG" "  ZOMBIE DUP: $ppp peer=$peer_ip user=$user rx=congelado — eliminando (real=$rx_active)"
      delete_ppp "$ppp"
    done
    continue
  fi

  # Ninguna tiene RX activo (todas inactivas). Conservar la mas joven
  # (la reconexion reciente es la real), eliminar las viejas TX<=107.
  best=""
  best_created=9999999999
  for ppp in $dup_ifaces; do
    created=$(stat -c %Y "$PPP_BASE/$ppp" 2>/dev/null || echo 0)
    if [ "$created" -lt "$best_created" ]; then
      best="$ppp"
      best_created=$created
    fi
  done

  for ppp in $dup_ifaces; do
    [ "$ppp" = "$best" ] && continue
    tx=$(cat "$PPP_BASE/$ppp/statistics/tx_bytes" 2>/dev/null || echo 0)
    # Si tiene trafico real (>107), es otro dispositivo con la misma cuenta
    # -> conservar (decision del operador: permitir multi-dispositivo)
    if [ "$tx" -gt 107 ]; then
      logger -t "$LOG_TAG" "  KEEP: $ppp peer=$peer_ip tx=$tx (trafico real — multi-dispositivo)"
      continue
    fi
    # Proteccion: interfaz joven -> recien conectada, no matar
    if iface_young "$ppp"; then
      logger -t "$LOG_TAG" "  KEEP: $ppp peer=$peer_ip (interfaz joven)"
      continue
    fi
    user=$(grep "$peer_ip" /etc/ppp/chap-secrets 2>/dev/null | awk '{print $1}')
    logger -t "$LOG_TAG" "  ZOMBIE DUP: $ppp peer=$peer_ip user=$user tx=$tx — eliminando (real=$best)"
    delete_ppp "$ppp"
  done
done

exit 0
