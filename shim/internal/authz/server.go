package authz

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
)

// maxMessage bounds one message from dockerd. dockerd passes at most 1 MiB of body, base64-encoded.
const maxMessage = 4 << 20

// message is the part of dockerd's AuthZReq and AuthZRes that the policy reads.
type message struct {
	RequestMethod string
	RequestUri    string //nolint:revive // dockerd's field name
	RequestBody   []byte
}

type answer struct {
	Allow bool
	Msg   string `json:",omitempty"`
	Err   string `json:",omitempty"`
}

// Handler serves dockerd's plugin protocol. logf receives one line for each refusal.
func Handler(logf func(format string, a ...any)) http.Handler {
	mux := http.NewServeMux()
	reply := func(w http.ResponseWriter, v any) {
		w.Header().Set("Content-Type", "application/vnd.docker.plugins.v1+json")
		_ = json.NewEncoder(w).Encode(v)
	}
	mux.HandleFunc("/Plugin.Activate", func(w http.ResponseWriter, _ *http.Request) {
		reply(w, map[string][]string{"Implements": {"authz"}})
	})
	mux.HandleFunc("/AuthZPlugin.AuthZReq", func(w http.ResponseWriter, r *http.Request) {
		var m message
		raw, err := io.ReadAll(io.LimitReader(r.Body, maxMessage+1))
		if err == nil && len(raw) > maxMessage {
			err = fmt.Errorf("message over %d bytes", maxMessage)
		}
		if err == nil {
			err = json.Unmarshal(raw, &m)
		}
		if err != nil {
			// A message this plugin cannot read is refused, never allowed.
			logf("DENY unreadable message from dockerd: %v", err)
			reply(w, answer{Msg: "vesper: the authorisation plugin could not read the request (ADR 0010)"})
			return
		}
		d := Decide(Request{Method: m.RequestMethod, URI: m.RequestUri, Body: m.RequestBody})
		if !d.Allow {
			logf("DENY %s %s: %s", clean(m.RequestMethod, 16), clean(pathOnly(m.RequestUri), 200), d.Reason)
		}
		reply(w, answer{Allow: d.Allow, Msg: d.Reason})
	})
	// The decision is made on the request. The response carries nothing this policy rules on.
	mux.HandleFunc("/AuthZPlugin.AuthZRes", func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, io.LimitReader(r.Body, maxMessage))
		reply(w, answer{Allow: true})
	})
	return mux
}

// pathOnly drops the query, which can hold names and credentials, from a logged URI.
func pathOnly(uri string) string {
	if u, err := url.ParseRequestURI(uri); err == nil {
		return u.Path
	}
	if i := strings.IndexByte(uri, '?'); i >= 0 {
		return uri[:i]
	}
	return uri
}

// clean makes a client-supplied string safe for one log line.
func clean(s string, max int) string {
	if len(s) > max {
		s = s[:max] + "…"
	}
	return strings.Map(func(r rune) rune {
		if r < 0x20 || r == 0x7f {
			return '?'
		}
		return r
	}, s)
}
