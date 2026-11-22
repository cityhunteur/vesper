// Package authz is the policy of vesper-authz, dockerd's authorisation plugin in the System VM (ADR 0010).
//
// dockerd runs every container in a remapped user namespace. This policy refuses the requests that would start a
// container outside that mapping, and every request it does not know: a route is allowed only when it is in the
// table below, and a route joins the table only with a test that shows it cannot start an unmapped container.
package authz

import (
	"encoding/json"
	"fmt"
	"net/url"
	"path"
	"regexp"
	"strings"
)

// Request is what the policy needs from dockerd's AuthZReq message.
type Request struct {
	Method string
	URI    string // the raw request URI, as the client sent it
	Body   []byte // empty when dockerd did not pass one: not JSON, no declared length, or 1 MiB and more
}

// Decision is the plugin's answer. Reason is set when Allow is false and is shown to the Docker client.
type Decision struct {
	Allow  bool
	Reason string
}

func deny(format string, a ...any) Decision {
	return Decision{Reason: "vesper: " + fmt.Sprintf(format, a...) + " (ADR 0010)"}
}

var allow = Decision{Allow: true}

// check inspects a request to an allowed route. A nil check allows it as it is.
type check func(u *url.URL, body []byte) Decision

type route struct {
	methods string // space-separated
	path    *regexp.Regexp
	check   check
}

// An object id or name: one path segment. Image and distribution names may hold slashes, so they use `.+`.
const seg = `[^/]+`

func r(methods, pattern string, c check) route {
	return route{methods: " " + methods + " ", path: regexp.MustCompile(`^` + pattern + `$`), check: c}
}

// routes is the allow-list, on paths with the API version removed. Swarm (/swarm, /services, /nodes, /tasks,
// /secrets, /configs), managed plugins (/plugins), checkpoints and BuildKit's /session and /grpc are absent on
// purpose: none has a test yet that shows it stays inside the mapping.
var routes = []route{
	r("GET HEAD", `/_ping`, nil),
	r("GET", `/(version|info|events|system/df)`, nil),
	r("POST", `/auth`, nil),

	r("GET", `/containers/json`, nil),
	r("POST", `/containers/create`, checkCreate),
	r("POST", `/containers/prune`, nil),
	r("GET", `/containers/`+seg+`/(json|top|logs|changes|export|stats|attach/ws|archive)`, nil),
	r("HEAD PUT", `/containers/`+seg+`/archive`, nil),
	r("POST", `/containers/`+seg+`/(start|stop|restart|kill|pause|unpause|wait|resize|attach|rename|update|exec)`, nil),
	r("DELETE", `/containers/`+seg, nil),
	r("POST", `/exec/`+seg+`/(start|resize)`, nil),
	r("GET", `/exec/`+seg+`/json`, nil),
	r("POST", `/commit`, nil),

	r("GET", `/images/(json|search|get)`, nil),
	r("POST", `/images/(create|load|prune)`, nil),
	r("GET", `/images/.+/(json|history|get)`, nil),
	r("POST", `/images/.+/(push|tag)`, nil),
	r("DELETE", `/images/.+`, nil),
	r("GET", `/distribution/.+/json`, nil),
	r("POST", `/build`, checkBuild),
	r("POST", `/build/prune`, nil),

	r("GET", `/networks`, nil),
	r("POST", `/networks/(create|prune)`, nil),
	r("GET DELETE", `/networks/`+seg, nil),
	r("POST", `/networks/`+seg+`/(connect|disconnect)`, nil),

	r("GET", `/volumes`, nil),
	r("POST", `/volumes/create`, checkVolumeCreate),
	r("POST", `/volumes/prune`, nil),
	r("GET DELETE", `/volumes/`+seg, nil),
}

var versionPrefix = regexp.MustCompile(`^/v[0-9]+(\.[0-9]+)*(/|$)`)

// Decide applies the policy to one request.
func Decide(req Request) Decision {
	u, err := url.ParseRequestURI(req.URI)
	if err != nil || u.Host != "" || u.Scheme != "" || !strings.HasPrefix(u.Path, "/") {
		return deny("the request URI is not a plain path")
	}
	// dockerd's router decides on the decoded path. A path that cleaning would change (a doubled slash, a dot
	// segment) is refused, so this table and the router never read one request two ways.
	p := u.Path
	if path.Clean(p) != p {
		return deny("the request path is not in its canonical form")
	}
	if m := versionPrefix.FindString(p); m != "" {
		p = "/" + strings.TrimPrefix(p, m)
		if p == "/" {
			return deny("%s %s is not on the list of allowed Docker API routes", req.Method, u.Path)
		}
	}
	for _, rt := range routes {
		if !strings.Contains(rt.methods, " "+req.Method+" ") || !rt.path.MatchString(p) {
			continue
		}
		if rt.check == nil {
			return allow
		}
		return rt.check(u, req.Body)
	}
	return deny("%s %s is not on the list of allowed Docker API routes", req.Method, p)
}

// driverConfig is a volume driver and its options. With options, the local driver makes dockerd, as root on the
// System VM, perform a mount the client describes; no test bounds what that can expose, so options are refused.
type driverConfig struct {
	Name    string
	Options map[string]string
}

func (d *driverConfig) problem() string {
	if d == nil {
		return ""
	}
	if d.Name != "" && d.Name != "local" {
		return fmt.Sprintf("volume driver %q is not allowed", d.Name)
	}
	if len(d.Options) != 0 {
		return "volume driver options are not allowed"
	}
	return ""
}

// createBody has dockerd's own field names, so encoding/json matches keys here exactly as it does in dockerd:
// without regard to case, and the last of several equal keys wins.
type createBody struct {
	HostConfig *struct {
		UsernsMode     string
		Runtime        string
		VolumeDriver   string
		Binds          []string
		Privileged     bool
		ReadonlyRootfs bool
		NetworkMode    string
		PidMode        string
		IpcMode        string
		UTSMode        string
		VolumesFrom    []string
		Devices        []json.RawMessage
		DeviceRequests []json.RawMessage
		Mounts         []struct {
			Type          string
			Source        string
			VolumeOptions *struct {
				DriverConfig *driverConfig
			}
		}
	}
	NetworkingConfig *struct {
		EndpointsConfig map[string]json.RawMessage
	}
}

func denyVesper(format string, a ...any) Decision {
	return Decision{Reason: "vesper: " + fmt.Sprintf(format, a...) + " (ADR 0011)"}
}

// checkVesperCreate refuses what a vesper container cannot honour (ADR 0011 §1). Until slice 3 only the default
// bridge and no network are made, and until slice 4 no bind or mount at all.
func checkVesperCreate(c *createBody) Decision {
	h := c.HostConfig
	switch {
	case h.Privileged:
		return denyVesper("--privileged is refused for the vesper runtime: the microVM is the boundary")
	case len(h.Devices) > 0 || len(h.DeviceRequests) > 0:
		return denyVesper("--device and --gpus are refused for the vesper runtime: no host device reaches a microVM")
	case h.ReadonlyRootfs:
		return denyVesper("--read-only is refused for the vesper runtime")
	case len(h.VolumesFrom) > 0:
		return denyVesper("--volumes-from is refused for the vesper runtime")
	case len(h.Binds) > 0 || len(h.Mounts) > 0:
		return denyVesper("-v and --mount are refused for the vesper runtime until shares reach its microVMs (ADR 0011 slice 4)")
	}
	switch h.NetworkMode {
	case "", "default", "bridge", "none":
	default:
		return denyVesper("--network=%s is refused for the vesper runtime: only the default bridge and none, until user-defined networks (ADR 0011 slice 3)", h.NetworkMode)
	}
	for _, m := range []struct{ name, value string }{{"--pid", h.PidMode}, {"--ipc", h.IpcMode}, {"--uts", h.UTSMode}} {
		if m.value != "" && m.value != "private" {
			return denyVesper("%s=%s is refused for the vesper runtime: its namespaces are its own", m.name, m.value)
		}
	}
	if c.NetworkingConfig != nil && len(c.NetworkingConfig.EndpointsConfig) > 1 {
		return denyVesper("a vesper container gets one network at create")
	}
	return allow
}

const unreadable = "its body is not visible to the authorisation plugin (dockerd passes only a JSON body with a declared length under 1 MiB)"

func checkCreate(_ *url.URL, body []byte) Decision {
	if len(body) == 0 {
		return deny("container create refused: %s", unreadable)
	}
	var c createBody
	if err := json.Unmarshal(body, &c); err != nil {
		return deny("container create refused: its body is not valid JSON")
	}
	h := c.HostConfig
	if h == nil {
		return allow
	}
	if h.UsernsMode != "" {
		return deny("--userns=%s is refused: every container runs in the remapped user namespace", h.UsernsMode)
	}
	// vesper-runc is the default (TD-062); runc is the same runtime without the share-ownership refresh; vesper runs
	// the container in a microVM (ADR 0011).
	if h.Runtime != "" && h.Runtime != "vesper-runc" && h.Runtime != "runc" && h.Runtime != VesperRuntime {
		return deny("--runtime=%s is refused: only vesper-runc, runc and vesper are known to stay in the remapped user namespace", h.Runtime)
	}
	// Any runtime: joining a vesper container's namespaces reaches past its microVM (ADR 0011 §1).
	for _, m := range []struct{ name, value string }{{"--network", h.NetworkMode}, {"--pid", h.PidMode}, {"--ipc", h.IpcMode}} {
		if why := joinsVesper(m.name, m.value); why != "" {
			return denyVesper("%s", why)
		}
	}
	if h.Runtime == VesperRuntime {
		if d := checkVesperCreate(&c); !d.Allow {
			return d
		}
	}
	if h.VolumeDriver != "" && h.VolumeDriver != "local" {
		return deny("volume driver %q is not allowed", h.VolumeDriver)
	}
	for _, b := range h.Binds {
		// "source:target[:options]"; a source that is not a path names a volume.
		src, _, _ := strings.Cut(b, ":")
		if p := bindProblem(src); p != "" {
			return deny("%s", p)
		}
	}
	for _, m := range h.Mounts {
		if strings.EqualFold(m.Type, "bind") {
			if p := bindProblem(m.Source); p != "" {
				return deny("%s", p)
			}
		}
		if m.VolumeOptions == nil {
			continue
		}
		if p := m.VolumeOptions.DriverConfig.problem(); p != "" {
			return deny("%s", p)
		}
	}
	return allow
}

// MissingSourceOK reports whether dockerd may create a bind source that does not exist yet. dockerd creates a
// missing source directory itself and gives it to container root. On the System VM's own filesystems that hands a
// container a directory that a root tool may later adopt (a state directory made lazily with mkdir -p), so it is
// allowed only on a shared macOS directory. The plugin's main sets it; the default refuses every missing source.
var MissingSourceOK = func(path string) (exists, creatable bool) { return false, false }

func bindProblem(src string) string {
	if !strings.HasPrefix(src, "/") {
		return "" // a named volume
	}
	if path.Clean(src) != src {
		return fmt.Sprintf("bind source %q is not a canonical path", src)
	}
	exists, creatable := MissingSourceOK(src)
	if exists || creatable {
		return ""
	}
	return fmt.Sprintf("bind source %s does not exist, and Vesper creates missing sources only inside a shared macOS directory", src)
}

func checkVolumeCreate(_ *url.URL, body []byte) Decision {
	if len(body) == 0 {
		return deny("volume create refused: %s", unreadable)
	}
	var v struct {
		Driver     string
		DriverOpts map[string]string
	}
	if err := json.Unmarshal(body, &v); err != nil {
		return deny("volume create refused: its body is not valid JSON")
	}
	if p := (&driverConfig{Name: v.Driver, Options: v.DriverOpts}).problem(); p != "" {
		return deny("%s", p)
	}
	return allow
}

// checkBuild allows the legacy builder, whose steps run remapped (run/GH-194/spike2/out.txt). BuildKit
// (version=2) needs /session and /grpc, which are not on the list; a build on the host network is refused here
// because no test shows what dockerd does with it under remapping.
func checkBuild(u *url.URL, _ []byte) Decision {
	q := u.Query()
	for _, v := range q["version"] {
		if v != "" && v != "1" {
			return deny("BuildKit builds are refused; use the legacy builder (DOCKER_BUILDKIT=0)")
		}
	}
	for _, v := range q["networkmode"] {
		switch strings.ToLower(v) {
		case "", "default", "bridge", "none":
		default:
			return deny("docker build --network=%s is refused", v)
		}
	}
	return allow
}
