package authz

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"strings"
)

// ADR 0011 slice 5: routes whose answer would be silently wrong for a vesper container are refused for one. dockerd
// keeps its own copy of the container's root filesystem, which never sees the guest's writes, so `docker cp`,
// `export`, `diff` and `commit` would read or change a stale tree; `network connect` and `disconnect` would change
// a namespace the running microVM cannot follow. The container is resolved as dockerd resolves it; any other
// container is left to dockerd.

// vesperContainer names why ref is refused for operation op, or "" when it is not a vesper container.
func vesperContainer(op, ref string) string {
	if ref == "" {
		return ""
	}
	all, err := readStore()
	if errors.Is(err, errNoStore) {
		return "" // no store yet, so no vesper container either
	}
	if err != nil {
		return fmt.Sprintf("%s is refused: dockerd's container store could not be read (%v)", op, err)
	}
	c, found := resolveContainer(ref, all)
	if !found || c.runtime != VesperRuntime {
		return ""
	}
	return fmt.Sprintf("%s is refused for a vesper container: dockerd's copy of its files does not see the microVM's writes", op)
}

// containerRef is the container segment of /containers/<ref>/... on a path with or without the API version.
func containerRef(u *url.URL) string {
	p := versionPrefix.ReplaceAllString(u.Path, "/")
	rest, ok := strings.CutPrefix(p, "/containers/")
	if !ok {
		return ""
	}
	ref, _, _ := strings.Cut(rest, "/")
	return ref
}

func checkContainerFiles(op string) check {
	return func(u *url.URL, _ []byte) Decision {
		if why := vesperContainer(op, containerRef(u)); why != "" {
			return denyVesper("%s", why)
		}
		return allow
	}
}

func checkCommit(u *url.URL, _ []byte) Decision {
	if why := vesperContainer("docker commit", u.Query().Get("container")); why != "" {
		return denyVesper("%s", why)
	}
	return allow
}

func checkNetworkMember(u *url.URL, body []byte) Decision {
	op := "docker network connect"
	if strings.HasSuffix(u.Path, "/disconnect") {
		op = "docker network disconnect"
	}
	if len(strings.TrimSpace(string(body))) == 0 {
		return allow // no container named: dockerd refuses it itself
	}
	var b map[string]json.RawMessage
	if err := json.Unmarshal(body, &b); err != nil {
		return denyVesper("%s: the request body is not readable", op)
	}
	var ref string
	for k, v := range b {
		if strings.EqualFold(k, "Container") {
			_ = json.Unmarshal(v, &ref)
		}
	}
	if why := vesperContainer(op, ref); why != "" {
		return denyVesper("%s; a microVM keeps the networks it started with", strings.Replace(why, "dockerd's copy of its files does not see the microVM's writes", "its network namespace is fixed while it runs", 1))
	}
	return allow
}
