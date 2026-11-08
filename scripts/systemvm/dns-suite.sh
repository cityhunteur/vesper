#!/usr/bin/env bash
# GH #30 checklist from macOS through the ingress: the default-bridge resolver and external DNS, a compose
# stack's embedded DNS (service discovery, aliases, external forwarding from a user network), and the
# determinism of those defaults across two `compose up` runs. Writes run/systemvm/dns-report.json.
# Needs `make systemvm-up && make docker-up`, docker compose and outbound network for the test image.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/dns-report.json"
STACK="${ROOT_DIR}/scripts/systemvm/fixtures/dns-stack.yml"
IMAGE="${DNS_IMAGE:-alpine:3.22}"
EXTERNAL="${DNS_EXTERNAL_HOST:-registry-1.docker.io}"
DOCKER="${DOCKER_BIN:-docker}"
export DOCKER_HOST="unix://${SOCKET}" DNS_IMAGE="${IMAGE}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:150}"; else echo "FAIL $1: ${d:0:200}"; fi; }
compose() { "${DOCKER}" compose -f "${STACK}" -p vesper-dns "$@"; }
cexec() { compose exec -T client "$@"; }
cleanup() { compose down --remove-orphans -t 1 >/dev/null 2>&1; }
cleanup
[ -S "${SOCKET}" ] || { echo "dns-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
"${DOCKER}" compose version >/dev/null 2>&1 || { echo "dns-suite: docker compose is not available" >&2; exit 2; }
"${DOCKER}" image inspect "${IMAGE}" >/dev/null 2>&1 || "${DOCKER}" pull -q "${IMAGE}" >/dev/null 2>&1 || { echo "dns-suite: cannot pull ${IMAGE}" >&2; exit 2; }

# Default bridge network: the container inherits the System VM's resolver and reaches external DNS.
rc_out="$("${DOCKER}" run --rm "${IMAGE}" sh -c 'grep -E "^nameserver" /etc/resolv.conf | tr "\n" " "' 2>&1)"
case "${rc_out}" in nameserver*) record default_bridge_resolver true "/etc/resolv.conf: ${rc_out}" ;; *) record default_bridge_resolver false "${rc_out}" ;; esac
out="$("${DOCKER}" run --rm "${IMAGE}" sh -c "getent hosts ${EXTERNAL} | awk '{print \$1}' | head -1" 2>&1)"
[ -n "${out}" ] && record external_dns_default_bridge true "${EXTERNAL} -> ${out}" || record external_dns_default_bridge false "no answer for ${EXTERNAL}"
out="$("${DOCKER}" run --rm "${IMAGE}" sh -c "nc -zw5 ${EXTERNAL} 443 && echo connected" 2>&1)"
[ "${out}" = "connected" ] && record external_tcp_egress true "${EXTERNAL}:443 reachable through the System VM NAT" || record external_tcp_egress false "${out}"

run_stack() { # suffix -> records web ip, subnet, resolver
  compose up -d >/dev/null 2>&1 || { echo "compose up failed"; return 1; }
  for _ in $(seq 1 40); do cexec true >/dev/null 2>&1 && break; sleep 0.25; done
  resolver="$(cexec sh -c 'grep -E "^nameserver" /etc/resolv.conf | head -1' 2>/dev/null | tr -d '\r')"
  web_ip="$(cexec sh -c 'getent hosts web | awk "{print \$1}" | head -1' 2>/dev/null | tr -d '\r')"
  subnet="$("${DOCKER}" network inspect vesper-dns_stack --format '{{(index .IPAM.Config 0).Subnet}}' 2>/dev/null)"
  echo "${resolver}|${web_ip}|${subnet}"
}
first="$(run_stack)"; IFS='|' read -r resolver web_ip subnet <<<"${first}"
[ "${resolver}" = "nameserver 127.0.0.11" ] && record compose_embedded_resolver true "client resolv.conf: ${resolver} (dockerd embedded DNS); network ${subnet}" || record compose_embedded_resolver false "${first}"
[ -n "${web_ip}" ] && record service_discovery_resolves true "web -> ${web_ip}" || record service_discovery_resolves false "web did not resolve: ${first}"
out="$(cexec sh -c 'wget -qO- -T 5 http://web/ 2>&1 | tr -d "\r"' 2>&1)"
[ "${out}" = "web-ok" ] && record service_discovery_connects true "http://web/ -> ${out}" || record service_discovery_connects false "${out}"
for alias in api backend.internal; do
  ip="$(cexec sh -c "getent hosts ${alias} | awk '{print \$1}' | head -1" 2>/dev/null | tr -d '\r')"
  [ -n "${ip}" ] && [ "${ip}" = "${web_ip}" ] && record "alias_${alias//./_}_resolves" true "${alias} -> ${ip} (same as web)" || record "alias_${alias//./_}_resolves" false "${alias} -> ${ip:-none}, web ${web_ip}"
done
out="$(cexec sh -c "getent hosts ${EXTERNAL} | awk '{print \$1}' | head -1" 2>&1 | tr -d '\r')"
[ -n "${out}" ] && record external_dns_from_compose_network true "${EXTERNAL} via 127.0.0.11 -> ${out}" || record external_dns_from_compose_network false "no answer for ${EXTERNAL} from the compose network"
out="$(cexec sh -c 'getent hosts does-not-exist.invalid; echo "rc=$?"' 2>&1 | tr -d '\r' | tr '\n' ' ')"
case "${out}" in "rc=2 "*|"rc=2") record nxdomain_is_clean true "does-not-exist.invalid -> no address, getent rc=2 (NXDOMAIN)" ;; *) record nxdomain_is_clean false "${out}" ;; esac
cleanup
second="$(run_stack)"; IFS='|' read -r resolver2 web_ip2 subnet2 <<<"${second}"
# Service addresses depend on compose's start order inside the subnet; the resolver and the subnet are the defaults.
if [ "${resolver2}" = "${resolver}" ] && [ "${subnet2}" = "${subnet}" ] && [ -n "${web_ip2}" ]; then record deterministic_across_runs true "second compose up: same resolver ${resolver2} and subnet ${subnet2}; web ${web_ip} then ${web_ip2} (address order within the subnet is not fixed)"; else record deterministic_across_runs false "first ${first} second ${second}"; fi
cleanup
left="$("${DOCKER}" network ls --filter name=vesper-dns --format '{{.Name}}' | wc -l | tr -d ' ')"
[ "${left}" = 0 ] && record compose_down_cleans_network true "no vesper-dns networks left" || record compose_down_cleans_network false "${left} networks left"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","stack":"%s","steps":[%s],"passed":%s}\n' "${SOCKET}" "${STACK}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "dns suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]
