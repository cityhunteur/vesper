#!/usr/bin/env python3
"""Raw Docker API requests for the confinement suite (GH #194, ADR 0010).

The Docker CLI cannot send these: a container create padded past the size dockerd shows its authorisation plugin,
a chunked create, keys in another case, paths that are not canonical, and routes the plugin does not list. Each
must be refused; one plain create must succeed, so the refusals are not a dead socket.

usage: confine-requests.py DOCKER_SOCKET IMAGE     exit 0 when every case behaves, 1 otherwise
"""
import json
import socket
import sys


def request(sock_path, raw):
    """Send raw bytes; return (status code, first 160 bytes of the body). 0 means no HTTP answer."""
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(20)
    s.connect(sock_path)
    try:
        s.sendall(raw)
    except OSError:
        pass  # the server may answer and close before it has read a large body
    data = b""
    try:
        while True:
            if b"\r\n\r\n" in data:
                head, body = data.split(b"\r\n\r\n", 1)
                # An answer without a body (a redirect, 204, Content-Length: 0) is complete with its headers.
                if body or b"content-length: 0" in head.lower() or head[9:12] in (b"204", b"301", b"304"):
                    break
            chunk = s.recv(65536)
            if not chunk:
                break
            data += chunk
    except OSError:
        pass
    s.close()
    text = data.decode(errors="replace")
    try:
        status = int(text.split(" ", 2)[1])
    except (IndexError, ValueError):
        status = 0
    return status, text.split("\r\n\r\n", 1)[-1][:160].replace("\r\n", " ")


def send(sock_path, method, path, body=b"", framing=None, ctype="application/json"):
    head = f"{method} {path} HTTP/1.1\r\nHost: docker\r\nContent-Type: {ctype}\r\n"
    head += framing if framing is not None else f"Content-Length: {len(body)}\r\n"
    return request(sock_path, (head + "\r\n").encode() + body)


def main():
    sock_path, image = sys.argv[1], sys.argv[2]
    host = {"Image": image, "Cmd": ["true"], "HostConfig": {"UsernsMode": "host"}}
    small = json.dumps(host).encode()
    padded = json.dumps(dict(host, Labels={"pad": "x" * 1_200_000})).encode()
    lower = json.dumps({"image": image, "hostconfig": {"usernsmode": "host"}}).encode()
    merged = b'{"Image":"%s","HostConfig":{"UsernsMode":"host"},"hostconfig":{"Runtime":"runc"}}' % image.encode()
    chunked = b"%x\r\n%s\r\n0\r\n\r\n" % (len(small), small)
    missing = json.dumps({"Image": image, "HostConfig": {"Binds": ["/var/lib/vesper/confine-missing:/m"]}}).encode()
    create = "/v1.51/containers/create"
    # Refused by the plugin: 403 with its own message.
    plugin = [
        ("host user namespace", send(sock_path, "POST", create, small)),
        ("padded past 1 MiB", send(sock_path, "POST", create, padded)),
        ("chunked", send(sock_path, "POST", create, chunked, "Transfer-Encoding: chunked\r\n")),
        ("lower-case keys", send(sock_path, "POST", create, lower)),
        ("two HostConfig objects", send(sock_path, "POST", create, merged)),
        ("no API version", send(sock_path, "POST", "/containers/create", small)),
        ("not declared as JSON", send(sock_path, "POST", create, small, ctype="text/plain")),
        ("missing bind source", send(sock_path, "POST", create, missing)),
        ("managed plugins", send(sock_path, "GET", "/v1.51/plugins")),
        ("plugin pull", send(sock_path, "POST", "/v1.51/plugins/pull?remote=x")),
        ("swarm", send(sock_path, "POST", "/v1.51/swarm/init", b"{}")),
        ("services", send(sock_path, "GET", "/v1.51/services")),
        ("BuildKit session", send(sock_path, "POST", "/v1.51/session")),
        ("BuildKit grpc", send(sock_path, "POST", "/v1.51/grpc")),
        ("build on the host network", send(sock_path, "POST", "/v1.51/build?networkmode=host", ctype="application/x-tar")),
    ]
    # Never reach a handler: dockerd's router redirects a path that is not canonical and has no such route.
    router = [
        ("doubled slash", send(sock_path, "POST", "//v1.51/containers/create", small), 301),
        ("dot segment", send(sock_path, "POST", "/v1.51/images/../containers/create", small), 301),
        ("unknown route", send(sock_path, "GET", "/v1.51/nothing"), 404),
    ]
    ok = True
    notes = []
    for name, (status, body) in plugin:
        good = status == 403 and "vesper:" in body
        ok &= good
        notes.append(f"{name}={status}" if good else f"{name}=UNEXPECTED {status} {body}")
    for name, (status, body), want in router:
        good = status == want
        ok &= good
        notes.append(f"{name}={status}" if good else f"{name}=UNEXPECTED {status} {body}")
    status, body = send(sock_path, "POST", create, json.dumps({"Image": image, "Cmd": ["true"]}).encode())
    made = ""
    if status == 201:
        made = json.loads(body).get("Id", "")
        send(sock_path, "DELETE", f"/v1.51/containers/{made}?force=1")
    else:
        ok = False
    notes.append(f"plain create={status}")
    print("; ".join(notes))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
