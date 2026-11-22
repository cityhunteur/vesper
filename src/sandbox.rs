//! The seatbelt profile a host binary applies to itself at start (ADR 0012, D-080). A binary lists what it may touch
//! as typed rules; `render` writes them as a deny-by-default profile, with every path resolved and escaped; `apply`
//! calls `sandbox_init` in the process (macOS); `self_check` then proves the profile is in force by failing to read
//! a directory the rules do not name. On other systems there is no seatbelt, so `apply` does nothing: the shipped
//! binaries run on macOS only, and Linux builds exist for CI.

use std::path::{Path, PathBuf};

/// One thing a profile allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// Read one file.
    ReadFile(PathBuf),
    /// Read and write one file, which may not exist yet (a socket, a lock, a status file).
    WriteFile(PathBuf),
    /// Read and write a directory and everything under it.
    WriteTree(PathBuf),
    /// Listen on the Unix socket at this path.
    ListenUnix(PathBuf),
    /// Connect to the Unix socket at this path.
    ConnectUnix(PathBuf),
    /// Bind and accept on loopback TCP and UDP ports.
    LoopbackInbound,
    /// Send UDP datagrams to loopback addresses (replies of a published UDP port).
    LoopbackUdpOutbound,
    /// Connect to one loopback TCP port (a `tcp://127.0.0.1:<port>` upstream).
    ConnectLoopbackTcp(u16),
}

/// Resolves `path` as the kernel will see it: the deepest existing ancestor through `realpath`, then the rest
/// unchanged, so `/tmp/x` becomes `/private/tmp/x` before `x` exists.
pub fn canonical(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!(
            "sandbox: {} is not an absolute path",
            path.display()
        ));
    }
    let mut rest = Vec::new();
    let mut base = path.to_path_buf();
    loop {
        match std::fs::canonicalize(&base) {
            Ok(real) => {
                let mut out = real;
                for part in rest.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(_) => {
                let Some(name) = base.file_name().map(|n| n.to_os_string()) else {
                    return Err(format!(
                        "sandbox: no existing ancestor of {}",
                        path.display()
                    ));
                };
                rest.push(name);
                if !base.pop() {
                    return Err(format!(
                        "sandbox: no existing ancestor of {}",
                        path.display()
                    ));
                }
            }
        }
    }
}

/// A single file as the kernel will see it: its directory resolved, its own name not, so a symlink at the file
/// cannot choose what the rule grants (the access then fails, as the kernel matches the resolved path).
pub fn canonical_file(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("sandbox: {} names no file", path.display()))?;
    let dir = path
        .parent()
        .ok_or_else(|| format!("sandbox: {} has no directory", path.display()))?;
    Ok(canonical(dir)?.join(name))
}

/// `path` as an SBPL string literal. NUL, newlines and other control characters are refused; a backslash and a
/// double quote are escaped, so no path can end the string and add a rule of its own.
pub fn quote(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("sandbox: {} is not UTF-8", path.display()))?;
    if text.chars().any(char::is_control) {
        return Err(format!("sandbox: {text:?} has a control character"));
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

/// The profile for `rules`: deny by default, the system's own base (`system.sb`), then each rule.
pub fn render(rules: &[Rule]) -> Result<String, String> {
    let mut out = String::from(
        "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow signal (target self))\n",
    );
    for rule in rules {
        let line = match rule {
            Rule::ReadFile(p) => format!(
                "(allow file-read* (literal {}))",
                quote(&canonical_file(p)?)?
            ),
            Rule::WriteFile(p) => {
                format!(
                    "(allow file-read* file-write* (literal {}))",
                    quote(&canonical_file(p)?)?
                )
            }
            Rule::WriteTree(p) => {
                format!(
                    "(allow file-read* file-write* (subpath {}))",
                    quote(&canonical(p)?)?
                )
            }
            Rule::ListenUnix(p) => {
                let q = quote(&canonical_file(p)?)?;
                format!(
                    "(allow file-read* file-write* (literal {q}))\n(allow network-bind network-inbound (local unix-socket (path-literal {q})))"
                )
            }
            Rule::ConnectUnix(p) => {
                format!(
                    "(allow network-outbound (remote unix-socket (path-literal {})))",
                    quote(&canonical_file(p)?)?
                )
            }
            Rule::LoopbackInbound => {
                "(allow network-bind network-inbound (local ip \"localhost:*\"))".to_string()
            }
            Rule::LoopbackUdpOutbound => {
                "(allow network-outbound (remote udp \"localhost:*\"))".to_string()
            }
            Rule::ConnectLoopbackTcp(port) => {
                format!("(allow network-outbound (remote tcp \"localhost:{port}\"))")
            }
        };
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Applies `profile` to this process. It cannot be undone or widened. Call it before any thread starts.
#[cfg(target_os = "macos")]
pub fn apply(profile: &str) -> Result<(), String> {
    use std::ffi::{CStr, CString, c_char, c_int};
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }
    let text =
        CString::new(profile).map_err(|_| "sandbox: the profile has a NUL byte".to_string())?;
    let mut err: *mut c_char = std::ptr::null_mut();
    // SAFETY: `text` is a NUL-terminated string that outlives the call; `err` is either left null or set by
    // libsandbox to a string we read once and free with its own function. Flags 0 means the profile is SBPL text.
    let rc = unsafe { sandbox_init(text.as_ptr(), 0, &mut err) };
    if rc == 0 {
        return Ok(());
    }
    let reason = if err.is_null() {
        std::io::Error::last_os_error().to_string()
    } else {
        // SAFETY: libsandbox set `err` to a NUL-terminated string; it is freed right after it is copied.
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { sandbox_free_error(err) };
        message
    };
    Err(format!(
        "cannot apply the sandbox profile: {reason}; is the parent process sandboxed? (--no-sandbox runs without it, for diagnosis only)"
    ))
}

/// No seatbelt here: the shipped binaries run on macOS only.
#[cfg(not(target_os = "macos"))]
pub fn apply(_profile: &str) -> Result<(), String> {
    Ok(())
}

/// Proves the profile is in force: listing `canary`, a directory the rules do not name, must be refused. On other
/// systems there is no profile to prove.
pub fn self_check(canary: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    match std::fs::read_dir(canary) {
        // EPERM is the sandbox's refusal; EACCES (a home owned by someone else) proves nothing.
        Err(e) if e.raw_os_error() == Some(1) => Ok(()),
        Err(e) => Err(format!(
            "sandbox self-check: {} gave {e}, not a refusal",
            canary.display()
        )),
        Ok(_) => Err(format!(
            "sandbox self-check: {} could be read, so the profile is not in force",
            canary.display()
        )),
    }
}

/// Refuses rules that would cover `canary` (a tree at or above it), so `self_check` cannot fail for that reason
/// with a misleading message: say which rule, and where to put the file instead.
pub fn check_canary_free(rules: &[Rule], canary: &Path) -> Result<(), String> {
    let canary = canonical(canary)?;
    for rule in rules {
        if let Rule::WriteTree(p) = rule
            && canary.starts_with(canonical(p)?)
        {
            return Err(format!(
                "sandbox: the directory {} would cover {} (the self-check's canary); put the file in a subdirectory",
                p.display(),
                canary.display()
            ));
        }
    }
    Ok(())
}

/// The canary for `self_check`: the user's home directory, which no profile names.
pub fn canary() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_paths_cannot_end_their_string() {
        assert_eq!(quote(Path::new("/a b/c")).unwrap(), "\"/a b/c\"");
        assert_eq!(
            quote(Path::new("/x\")(allow default)(\"")).unwrap(),
            "\"/x\\\")(allow default)(\\\"\""
        );
        assert_eq!(
            quote(Path::new("/back\\slash")).unwrap(),
            "\"/back\\\\slash\""
        );
        for bad in ["/a\nb", "/a\0b", "/a\tb", "/a\u{1b}b"] {
            assert!(quote(Path::new(bad)).is_err(), "{bad:?}");
        }
        // Every quote in the output is escaped, so the literal holds the whole path.
        let q = quote(Path::new("/p\")\"(q")).unwrap();
        let inner = &q[1..q.len() - 1];
        let unescaped = inner
            .char_indices()
            .filter(|(i, c)| *c == '"' && !inner[..*i].ends_with('\\'))
            .count();
        assert_eq!(unescaped, 0, "{q}");
    }

    #[test]
    fn paths_resolve_through_their_deepest_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(
            canonical(&dir.path().join("not/yet/there.sock")).unwrap(),
            real.join("not/yet/there.sock")
        );
        assert!(canonical(Path::new("relative")).is_err());
        #[cfg(target_os = "macos")]
        assert!(
            canonical(Path::new("/tmp/vesper-x"))
                .unwrap()
                .starts_with("/private/tmp")
        );
    }

    #[test]
    fn the_profile_denies_by_default_and_names_each_rule() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let p = render(&[Rule::ListenUnix(sock.clone()), Rule::LoopbackInbound]).unwrap();
        assert!(p.starts_with("(version 1)\n(deny default)\n"));
        assert!(p.contains("(local unix-socket (path-literal "));
        assert!(p.contains(&canonical(&sock).unwrap().display().to_string()));
        assert!(p.contains("(local ip \"localhost:*\")"));
        assert!(!p.contains("(allow default)"));
    }
}
