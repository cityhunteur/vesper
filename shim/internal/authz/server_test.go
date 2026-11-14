package authz

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func post(t *testing.T, srv *httptest.Server, path, body string) answer {
	t.Helper()
	res, err := http.Post(srv.URL+path, "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	var a answer
	if err := json.NewDecoder(res.Body).Decode(&a); err != nil {
		t.Fatalf("%s: %v", path, err)
	}
	return a
}

func TestProtocol(t *testing.T) {
	var log []string
	srv := httptest.NewServer(Handler(func(f string, a ...any) { log = append(log, fmt.Sprintf(f, a...)) }))
	defer srv.Close()

	res, err := http.Post(srv.URL+"/Plugin.Activate", "application/json", nil)
	if err != nil {
		t.Fatal(err)
	}
	var act map[string][]string
	_ = json.NewDecoder(res.Body).Decode(&act)
	res.Body.Close()
	if len(act["Implements"]) != 1 || act["Implements"][0] != "authz" {
		t.Fatalf("activate: %v", act)
	}

	msg := func(method, uri, body string) string {
		b, _ := json.Marshal(map[string]any{"RequestMethod": method, "RequestUri": uri,
			"RequestBody": base64.StdEncoding.EncodeToString([]byte(body)), "User": "", "RequestHeaders": map[string]string{}})
		return string(b)
	}
	if a := post(t, srv, "/AuthZPlugin.AuthZReq", msg("GET", "/v1.51/version", "")); !a.Allow {
		t.Errorf("version refused: %+v", a)
	}
	if a := post(t, srv, "/AuthZPlugin.AuthZReq", msg("POST", "/v1.51/containers/create?name=secret-name",
		`{"HostConfig":{"UsernsMode":"host"}}`)); a.Allow || !strings.Contains(a.Msg, "--userns=host") {
		t.Errorf("host userns: %+v", a)
	}
	if a := post(t, srv, "/AuthZPlugin.AuthZReq", msg("POST", "/v1.51/containers/create", "")); a.Allow {
		t.Errorf("create without a body allowed")
	}
	for _, bad := range []string{``, `{`, `[]`, `"x"`, `{"RequestBody":"not base64!"}`, strings.Repeat("x", maxMessage+2)} {
		if a := post(t, srv, "/AuthZPlugin.AuthZReq", bad); a.Allow {
			t.Errorf("unreadable message %.20q allowed", bad)
		}
	}
	if a := post(t, srv, "/AuthZPlugin.AuthZReq", msg("GET\nDENY forged", "/v1.51/x\ny", "")); a.Allow {
		t.Errorf("forged method allowed")
	}
	if a := post(t, srv, "/AuthZPlugin.AuthZRes", msg("POST", "/v1.51/swarm/init", "")); !a.Allow {
		t.Errorf("response hook refused: %+v", a)
	}
	for _, l := range log {
		if strings.ContainsAny(l, "\n\r") {
			t.Errorf("log line holds a line break: %q", l)
		}
		if strings.Contains(l, "secret-name") {
			t.Errorf("log line holds the query: %q", l)
		}
	}
	if len(log) < 4 {
		t.Errorf("refusals logged: %d", len(log))
	}
}
