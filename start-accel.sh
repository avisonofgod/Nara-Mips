#!/bin/sh
# Escribe config de accel-ppp y arranca en eth3
cat << 'EOF' > /etc/accel-ppp.conf
[modules]
log_file
pppoe
auth_chap_md5
auth_pap
chap-secrets
ippool

[core]
log-level=5

[log]
log-file=/var/log/accel-ppp.log
log-level=5

[pppoe]
interface=eth3

[dns]
dns1=192.168.20.1
dns2=8.8.8.8

[client-ip-range]
192.168.20.2-192.168.20.254

[ip-pool]
gw-ip-address=192.168.20.1

[ppp]
lcp-echo-interval=10
lcp-echo-failure=2
mtu=1492
mru=1492
noauth
# Sin opciones de auth — chap-secrets manejara

[chap-secrets]
gw-ip-address=192.168.20.1
EOF
pkill accel-pppd 2>/dev/null
sleep 1
accel-pppd -c /etc/accel-ppp.conf -d
sleep 2
pgrep accel-pppd && echo "accel OK" || echo "accel FAIL"
