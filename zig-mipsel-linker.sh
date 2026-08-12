#!/bin/bash
# Linker wrapper NARA: fuerza link ESTATICO musl mipsel
# - quita -pie / -Bdynamic / -lgcc_s (no existe libgcc_s.a en musl-cross)
# - -static al inicio, -lgcc (estatica) al final
GCC=/home/toolchains/mipsel-linux-musl-cross/bin/mipsel-linux-musl-gcc
args=()
for a in "$@"; do
  case "$a" in
    -pie|-Wl,-Bdynamic|-Wl,--eh-frame-hdr|-lgcc_s) continue ;;
  esac
  args+=("$a")
done
exec "$GCC" -static "${args[@]}" -lgcc
