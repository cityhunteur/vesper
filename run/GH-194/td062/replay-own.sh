#!/usr/bin/env bash
# The binds suite's replay-ownership step alone (TD-062 negative control).
ROOT_DIR=/Volumes/Workspace/github.com/cityhunteur/vesper; SHARE=$ROOT_DIR/run/WOR-39/share; IMAGE=alpine:3.22; DOCKER=docker
export DOCKER_HOST="unix://${ROOT_DIR}/run/vesper/docker.sock"
"${DOCKER}" rm -f binds-own >/dev/null 2>&1
"${DOCKER}" run -d --name binds-own -v "${SHARE}:/data" "${IMAGE}" sh -c 'printf "%s\n" "#!/bin/sh" "[ \"\$3\" = owned.txt ] || exit 0" "echo \"\$(stat -c %u /data/owned.txt) \$(chmod 644 /data/owned.txt 2>&1 && echo chmod-ok)\" >> /tmp/own.log" > /own.sh; chmod +x /own.sh; inotifyd /own.sh /data:c & sleep 120' >/dev/null 2>&1; sleep 1
for i in 1 2 3 4 5; do echo "edit ${i}" >> "${SHARE}/owned.txt"; sleep 0.6; done; sleep 1
own="$("${DOCKER}" exec binds-own cat /tmp/own.log 2>&1 | tr '\n' ';')"
n_ok="$(printf '%s' "${own}" | tr ';' '\n' | grep -c '^0 chmod-ok$')"; n_all="$(printf '%s' "${own}" | tr ';' '\n' | grep -c .)"
[ "${n_all}" -ge 4 ] && [ "${n_ok}" = "${n_all}" ] && echo PASS replayed_file_owned_by_container_root "${n_all} replayed host edits: each event handler saw owner 0 and could chmod at once (${own})" || echo FAIL replayed_file_owned_by_container_root "handler lines: ${own}"
"${DOCKER}" rm -f binds-own >/dev/null 2>&1; rm -f "${SHARE}/owned.txt"
