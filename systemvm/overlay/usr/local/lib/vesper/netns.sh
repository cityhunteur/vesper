# Network namespaces for microVMs, sourced by vesper-vesperd (CE-0010) and vesper-microvm (ADR 0009). Each microVM
# with a tap device gets its own namespace, bound at DIR/ID so the jailer can join it (--netns) and teardown can
# find it. The namespace has lo and the taps and nothing else: no veth, no route out. Each caller passes its own DIR
# and tap owner, because the two backends may use the same id and reap their own directories only.
#   vnetns_mounted DIR ID            0 when DIR/ID is a bound namespace
#   vnetns_down DIR ID               unbind and remove; the taps go with the namespace once Firecracker is gone
#   vnetns_up DIR OWNER ID TAP...    create it with lo up and each TAP owned by user OWNER (the kernel's tun owner
#                                    check; busybox tunctl takes a user name); prints a reason and fails otherwise
vnetns_oneline() { tr '\n\r\t' '   ' | sed 's/[^ -~]/?/g' | cut -c1-200; }
vnetns_mounted() { awk -v f="$1/$2" '$2 == f && $3 == "nsfs" {m=1} END {exit !m}' /proc/mounts; }
vnetns_down() {
  vn_f="$1/$2"
  # -l: an inspection may hold the file open; the namespace itself lives on until its last user goes.
  vnetns_mounted "$1" "$2" && umount -l "$vn_f" 2>/dev/null
  vnetns_mounted "$1" "$2" || rm -f "$vn_f"
}
vnetns_up() {
  vn_dir=$1; vn_owner=$2; vn_id=$3; shift 3; vn_f="$vn_dir/$vn_id"
  mkdir -p "$vn_dir" && chmod 0700 "$vn_dir" || { echo "netns directory"; return 1; }
  vnetns_down "$vn_dir" "$vn_id"
  [ -e "$vn_f" ] && { echo "netns $vn_id still bound"; return 1; }
  : > "$vn_f" && unshare --net="$vn_f" true || { rm -f "$vn_f"; echo "netns create failed"; return 1; }
  # Before any interface but lo exists, so every later one inherits "default": IPv6 off, strict reverse-path
  # filtering, no ICMP redirects (ADR 0008).
  # shellcheck disable=SC2016
  nsenter --net="$vn_f" sh -c 'for k in all default lo; do
      echo 1 > /proc/sys/net/ipv6/conf/$k/disable_ipv6 && echo 1 > /proc/sys/net/ipv4/conf/$k/rp_filter \
        && echo 0 > /proc/sys/net/ipv4/conf/$k/accept_redirects && echo 0 > /proc/sys/net/ipv4/conf/$k/send_redirects || exit 1
    done' || { echo "namespace sysctls"; return 1; }
  vn_out=$(nsenter --net="$vn_f" ip link set dev lo up 2>&1) || { echo "lo: $(printf '%s' "$vn_out" | vnetns_oneline)"; return 1; }
  for vn_t in "$@"; do
    vn_out=$(nsenter --net="$vn_f" tunctl -t "$vn_t" -u "$vn_owner" 2>&1) || { echo "tap $vn_t: $(printf '%s' "$vn_out" | vnetns_oneline)"; return 1; }
    vn_out=$(nsenter --net="$vn_f" ip link set dev "$vn_t" up 2>&1) || { echo "tap $vn_t up: $(printf '%s' "$vn_out" | vnetns_oneline)"; return 1; }
  done
}
