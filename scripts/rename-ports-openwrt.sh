#!/bin/sh /etc/rc.common
# rename-ports — renombra los puertos DSA del hEX a ethX neutro.
#
# El DTS de OpenWrt nombra los puertos como wan/lan2..lan5 y el puerto CPU
# como eth0. Para que NARA presente eth0..eth4 (y el cpu port quede como
# sw0), renombramos en el boot ANTES de que netifd configure la red.
#
# Instalacion (en el router):
#   scp scripts/rename-ports-openwrt.sh root@ROUTER:/etc/init.d/rename-ports
#   ssh root@ROUTER "chmod +x /etc/init.d/rename-ports; /etc/init.d/rename-ports enable"
#
# Regla dura: ejecutar ANTES de /etc/init.d/network (START=08) y tener una
# consola de respaldo (cable en ether5 + IP 192.168.10.1/24) por si netifd
# derriba la interfaz durante el primer rename.

START=08
STOP=90

boot() {
    # Esperar a que el kernel cree los netdevs DSA (udev/hotplug async)
    local tries=0
    while [ $tries -lt 20 ]; do
        [ -d /sys/class/net/wan ] && [ -d /sys/class/net/lan5 ] && break
        tries=$((tries + 1))
        sleep 0.5
    done
    rename_ports
    # Iniciar el resto del arranque (netifd configurara con device ethX)
    start
}

start() {
    rename_ports
}

rename_ports() {
    # Orden critico: liberar eth0 primero (cpu port -> sw0), luego los demas.
    ip link set eth0 name sw0 2>/dev/null
    ip link set wan name eth0 2>/dev/null
    ip link set lan2 name eth1 2>/dev/null
    ip link set lan3 name eth2 2>/dev/null
    ip link set lan4 name eth3 2>/dev/null
    ip link set lan5 name eth4 2>/dev/null
    return 0
}
