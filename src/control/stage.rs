//! Staging and the snapshot store for `vesperd` microVMs (ADR 0007 section 1, slice 5).
//!
//! `vesperd --stage SRC:DEST` turns into a `stage=SRC:DEST` token on `LAUNCHAPI`, staged into the fresh jail before
//! Firecracker starts; `vesperd stage` and `vesperd store` send `VESPERD stage|store-list|store-rm` while microVMs
//! run. This module checks the same forms the guest helper checks (`vesper-vesperd`, `valid_path` and
//! `valid_name`), so a bad argument is refused before anything reaches the System VM; the guest checks every
//! argument again and alone decides what a path names.

use super::Session;

/// `stage=` tokens per `LAUNCHAPI`.
pub const MAX_STAGES: usize = 16;
/// Bytes in a jail or share path.
pub const MAX_STAGE_PATH: usize = 1024;
/// Bytes in a store name.
pub const MAX_STORE_NAME: usize = 128;

/// Where a staged file comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageSource {
    /// An absolute macOS path inside a share named in the launch configuration (mounted at the same path).
    Share(String),
    /// A file in the store.
    Store(String),
    /// A path in the microVM's own jail.
    Jail(String),
}

/// Where a staged file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageDest {
    /// A path in the microVM's jail, as Firecracker sees it.
    Jail(String),
    /// A file in the store.
    Store(String),
}

/// One `--stage SRC:DEST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageSpec {
    pub src: StageSource,
    pub dest: StageDest,
}

/// One `STORE` line of `vesperd store list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    pub name: String,
    pub bytes: u64,
    pub links: u64,
}

const PATH_CHARS: &str = "._+,@=-";

/// An absolute path of letters, digits and `._+,@=-` between single slashes, with no `.` or `..` component and no
/// trailing slash, of at most `MAX_STAGE_PATH` bytes. The guest refuses every other form.
pub fn valid_stage_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    path.len() <= MAX_STAGE_PATH
        && rest
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && c.chars().all(|ch| ch.is_ascii_alphanumeric() || PATH_CHARS.contains(ch)))
}

/// A store name: 1 to `MAX_STORE_NAME` letters, digits and `._+,@=-`, not starting with a dot.
pub fn valid_store_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_STORE_NAME
        && !name.starts_with('.')
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || PATH_CHARS.contains(ch))
}

impl StageSource {
    /// `share:/PATH`, `store:NAME` or `jail:/PATH`.
    pub fn parse(text: &str) -> Result<StageSource, String> {
        let (scheme, value) = text.split_once(':').ok_or_else(|| format!("bad SRC {text:?}: use share:/PATH, store:NAME or jail:/PATH"))?;
        match scheme {
            "share" if valid_stage_path(value) => Ok(StageSource::Share(value.to_string())),
            "jail" if valid_stage_path(value) => Ok(StageSource::Jail(value.to_string())),
            "store" if valid_store_name(value) => Ok(StageSource::Store(value.to_string())),
            "share" | "jail" => Err(format!("bad SRC {text:?}: {}", path_rule())),
            "store" => Err(format!("bad SRC {text:?}: {}", name_rule())),
            _ => Err(format!("bad SRC {text:?}: use share:/PATH, store:NAME or jail:/PATH")),
        }
    }

    pub fn wire(&self) -> String {
        match self {
            StageSource::Share(p) => format!("share:{p}"),
            StageSource::Store(n) => format!("store:{n}"),
            StageSource::Jail(p) => format!("jail:{p}"),
        }
    }
}

impl StageDest {
    /// `/PATH` in the jail or `store:NAME`.
    pub fn parse(text: &str) -> Result<StageDest, String> {
        if let Some(name) = text.strip_prefix("store:") {
            return if valid_store_name(name) {
                Ok(StageDest::Store(name.to_string()))
            } else {
                Err(format!("bad DEST {text:?}: {}", name_rule()))
            };
        }
        if valid_stage_path(text) {
            Ok(StageDest::Jail(text.to_string()))
        } else if text.starts_with('/') {
            Err(format!("bad DEST {text:?}: {}", path_rule()))
        } else {
            Err(format!("bad DEST {text:?}: use /PATH in the jail or store:NAME"))
        }
    }

    pub fn wire(&self) -> String {
        match self {
            StageDest::Jail(p) => p.clone(),
            StageDest::Store(n) => format!("store:{n}"),
        }
    }
}

fn path_rule() -> String {
    format!("a path is absolute, of letters, digits and {PATH_CHARS} between single slashes, with no . or .. component, at most {MAX_STAGE_PATH} bytes")
}

fn name_rule() -> String {
    format!("a store name is 1 to {MAX_STORE_NAME} letters, digits and {PATH_CHARS}, not starting with a dot")
}

/// The pairs `vesperd stage` refuses whatever the paths: store to store and jail to jail.
pub fn check_pair(src: &StageSource, dest: &StageDest) -> Result<(), String> {
    match (src, dest) {
        (StageSource::Store(_), StageDest::Store(_)) => Err("store to store is not a stage".to_string()),
        (StageSource::Jail(_), StageDest::Jail(_)) => Err("jail to jail is not a stage".to_string()),
        _ => Ok(()),
    }
}

/// Whether a stage touches the microVM's jail, and so needs `--id`.
pub fn needs_id(src: &StageSource, dest: &StageDest) -> bool {
    matches!(src, StageSource::Jail(_)) || matches!(dest, StageDest::Jail(_))
}

impl StageSpec {
    /// Parses `--stage SRC:DEST`: `share:/PATH:/DEST` or `store:NAME:/DEST`. Paths hold no colon, so the second
    /// colon ends SRC. `--stage` fills the fresh jail, so DEST is a jail path and SRC is not in the jail.
    pub fn parse(text: &str) -> Result<StageSpec, String> {
        let bad = || format!("--stage {text:?}: use SRC:DEST, for example store:snap.mem:/snap.mem");
        let (scheme, rest) = text.split_once(':').ok_or_else(bad)?;
        let (value, dest) = rest.split_once(':').ok_or_else(bad)?;
        let src = StageSource::parse(&format!("{scheme}:{value}")).map_err(|err| format!("--stage {text:?}: {err}"))?;
        let dest = StageDest::parse(dest).map_err(|err| format!("--stage {text:?}: {err}"))?;
        match (&src, &dest) {
            (StageSource::Jail(_), _) => Err(format!("--stage {text:?}: jail: sources need a running microVM; use vesperd stage")),
            (_, StageDest::Store(_)) => Err(format!("--stage {text:?}: --stage writes into the jail; use vesperd stage for store:")),
            _ => Ok(StageSpec { src, dest }),
        }
    }

    /// The `LAUNCHAPI` token.
    pub fn launch_token(&self) -> String {
        format!("stage={}:{}", self.src.wire(), self.dest.wire())
    }

    /// Whether `--stage` links this file instead of copying it: snapshot memory and vmstate files from the store.
    pub fn links(&self) -> bool {
        matches!(&self.src, StageSource::Store(n) if n.ends_with(".mem") || n.ends_with(".vmstate"))
    }
}

/// Parses `vesperd store list`'s reply: `STORE NAME bytes=N links=L` lines, then `FREE avail=N reserve=N store=N quota=N`.
pub fn parse_store_list(lines: &[String]) -> Result<(Vec<StoreEntry>, u64, u64), String> {
    let field = |word: Option<&str>, key: &str, line: &str| -> Result<u64, String> {
        word.and_then(|w| w.strip_prefix(key))
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("bad store line {line:?}"))
    };
    let mut entries = Vec::new();
    for line in lines {
        let mut words = line.split(' ');
        match words.next() {
            Some("STORE") => {
                let name = words.next().filter(|n| valid_store_name(n)).ok_or_else(|| format!("bad store line {line:?}"))?;
                let bytes = field(words.next(), "bytes=", line)?;
                let links = field(words.next(), "links=", line)?;
                entries.push(StoreEntry { name: name.to_string(), bytes, links });
            }
            Some("FREE") => {
                let avail = field(words.next(), "avail=", line)?;
                let reserve = field(words.next(), "reserve=", line)?;
                return Ok((entries, avail, reserve));
            }
            _ => return Err(format!("bad store line {line:?}")),
        }
    }
    Err("store list ended without a FREE line".to_string())
}

/// `DATA <state> …` from `vesper-data status`: true when the data disk is `formatted` or `mounted`.
pub fn data_disk_in_use(line: &str) -> bool {
    matches!(line.split(' ').nth(1), Some("formatted" | "mounted")) && line.starts_with("DATA ")
}

/// A guest helper's reply: the first line, with `ERR ` taken off for a failure.
fn outcome(rc: i32, lines: Vec<String>) -> Result<Vec<String>, (i32, String)> {
    if rc == 0 {
        return Ok(lines);
    }
    let reason = lines.first().map(|l| l.strip_prefix("ERR ").unwrap_or(l).to_string()).unwrap_or_else(|| format!("exit {rc}"));
    Err((rc, reason))
}

impl Session {
    /// `VESPERD stage ID|- SRC DEST`; returns the `STAGED` line, or the guest's exit code and reason.
    pub fn stage(&mut self, id: Option<&str>, src: &StageSource, dest: &StageDest) -> Result<String, (i32, String)> {
        let (src, dest) = (src.wire(), dest.wire());
        let (rc, lines) = self.vesperd(&["stage", id.unwrap_or("-"), &src, &dest]).map_err(|err| (1, err))?;
        let lines = outcome(rc, lines)?;
        lines.into_iter().find(|l| l.starts_with("STAGED ")).ok_or((1, "no STAGED line".to_string()))
    }

    /// `VESPERD store-list`; returns the raw lines (`STORE …` then `FREE …`).
    pub fn store_list(&mut self) -> Result<Vec<String>, (i32, String)> {
        let (rc, lines) = self.vesperd(&["store-list"]).map_err(|err| (1, err))?;
        let lines = outcome(rc, lines)?;
        parse_store_list(&lines).map_err(|err| (1, err))?;
        Ok(lines)
    }

    /// `VESPERD store-rm NAME`.
    pub fn store_rm(&mut self, name: &str) -> Result<String, (i32, String)> {
        let (rc, lines) = self.vesperd(&["store-rm", name]).map_err(|err| (1, err))?;
        let lines = outcome(rc, lines)?;
        Ok(lines.into_iter().next().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_names_follow_the_guest_rules() {
        for ok in ["/a", "/snap.mem", "/a/b-c/d_e.f+g,h@i=j", &format!("/{}", "x".repeat(MAX_STAGE_PATH - 1))] {
            assert!(valid_stage_path(ok), "{ok:?}");
        }
        for bad in ["", "a", "/", "/a/", "//a", "/a//b", "/./a", "/a/..", "/a/../b", "/a b", "/a*", "/a:b", "/a\\b", "/é",
            &format!("/{}", "x".repeat(MAX_STAGE_PATH))] {
            assert!(!valid_stage_path(bad), "accepted {bad:?}");
        }
        assert!(valid_store_name("snap-1.mem") && valid_store_name(&"n".repeat(MAX_STORE_NAME)));
        for bad in ["", ".partial.1", "..", "a/b", "a b", "a:b", &"n".repeat(MAX_STORE_NAME + 1)] {
            assert!(!valid_store_name(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn stage_specs_parse_into_launch_tokens() {
        let spec = StageSpec::parse("store:a.mem:/snap/a.mem").unwrap();
        assert_eq!(spec, StageSpec { src: StageSource::Store("a.mem".into()), dest: StageDest::Jail("/snap/a.mem".into()) });
        assert_eq!(spec.launch_token(), "stage=store:a.mem:/snap/a.mem");
        assert!(spec.links());
        let share = StageSpec::parse("share:/Users/me/vm/disk.ext4:/disk.ext4").unwrap();
        assert_eq!(share.launch_token(), "stage=share:/Users/me/vm/disk.ext4:/disk.ext4");
        assert!(!share.links());
        assert!(!StageSpec::parse("store:disk.ext4:/disk.ext4").unwrap().links());
        assert!(StageSpec::parse("store:a.vmstate:/a.vmstate").unwrap().links());
        for (bad, why) in [
            ("store:a.mem", "use SRC:DEST"),
            ("/x:/y:/z", "use share:/PATH"),
            ("jail:/a:/b", "need a running microVM"),
            ("share:/a:store:b", "--stage writes into the jail"),
            ("share:/a/../etc/shadow:/x", "no . or .. component"),
            ("store:.partial.1:/x", "not starting with a dot"),
            ("store:a:relative", "use /PATH in the jail"),
            ("share:/a b:/x", "letters, digits"),
        ] {
            let err = StageSpec::parse(bad).unwrap_err();
            assert!(err.contains(why), "{bad:?}: {err}");
        }
    }

    #[test]
    fn pairs_and_ids() {
        let jail = || StageSource::Jail("/a.mem".into());
        let store_src = || StageSource::Store("a".into());
        assert!(check_pair(&store_src(), &StageDest::Store("b".into())).is_err());
        assert!(check_pair(&jail(), &StageDest::Jail("/b".into())).is_err());
        assert!(check_pair(&jail(), &StageDest::Store("a.mem".into())).is_ok());
        assert!(needs_id(&jail(), &StageDest::Store("a".into())));
        assert!(needs_id(&store_src(), &StageDest::Jail("/a".into())));
        assert!(!needs_id(&StageSource::Share("/s/a".into()), &StageDest::Store("a".into())));
        assert_eq!(StageDest::parse("store:x").unwrap().wire(), "store:x");
        assert_eq!(StageSource::parse("jail:/a.mem").unwrap().wire(), "jail:/a.mem");
    }

    #[test]
    fn store_list_and_data_status_parse() {
        let lines: Vec<String> = ["STORE a.mem bytes=268435456 links=2", "FREE avail=100 reserve=50"].iter().map(|s| s.to_string()).collect();
        let (entries, avail, reserve) = parse_store_list(&lines).unwrap();
        assert_eq!(entries, vec![StoreEntry { name: "a.mem".into(), bytes: 268435456, links: 2 }]);
        assert_eq!((avail, reserve), (100, 50));
        assert!(parse_store_list(&lines[..1]).is_err());
        assert!(parse_store_list(&["STORE ../x bytes=1 links=1".to_string()]).is_err());
        assert!(data_disk_in_use("DATA mounted /dev/vdb used=1 avail=2"));
        assert!(data_disk_in_use("DATA formatted /dev/vdb"));
        for no in ["DATA absent", "DATA refused /dev/vdb foreign:ext4:x", "DATA unknown", "mounted", ""] {
            assert!(!data_disk_in_use(no), "{no:?}");
        }
    }
}
