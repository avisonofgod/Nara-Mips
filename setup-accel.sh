#!/bin/sh
# Reemplazar accel-ppp.conf con config para eth3 PPPoE
cat > /etc/accel-ppp.conf << 'EOF'
[modules]
log_file
pppoe
auth_mschap_v2
auth_mschap_v1
auth_chap_md5
auth_pap
chap-secrets
ippool
pppd_compat

[core]
log-level=5

[log]
log-file=/var/log/accel-ppp.log
log-level=5

[pppoe]
interface=eth3
service-name=Zpot

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

[chap-secrets]
gw-ip-address=192.168.20.1
EOF
logger "accel-ppp.conf updated"
echo "OK"
