//! TD-026: the guest agent's line protocol stays as documented. The verbs in `vesper-agent`'s header are the verbs its
//! loop answers; its handler bound is socat's `max-children`; the loop has an idle timeout; `LAUNCHAPI` and `FCAPI`
//! check for a free lease slot before they take the connection; and the protocol version is the client's.

use std::collections::BTreeSet;

const AGENT: &str = include_str!("../systemvm/overlay/usr/local/bin/vesper-agent");
const INIT: &str = include_str!("../systemvm/overlay/etc/init.d/vesper-agent");

fn documented() -> BTreeSet<String> {
    AGENT
        .lines()
        .take_while(|l| !l.starts_with("#   anything else"))
        .filter_map(|l| l.strip_prefix("#   "))
        .filter(|l| !l.starts_with(' '))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| w.len() > 1 && w.chars().all(|c| c.is_ascii_uppercase()))
        .map(str::to_string)
        .collect()
}

fn answered() -> BTreeSet<String> {
    let start = AGENT.find("\nwhile IFS= read").expect("the command loop");
    AGENT[start..]
        .lines()
        .filter_map(|l| l.strip_prefix("    "))
        .filter(|l| !l.starts_with(' '))
        .filter_map(|l| l.split_once(')').map(|(labels, _)| labels))
        .flat_map(|labels| labels.split('|'))
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_uppercase()))
        .map(str::to_string)
        .collect()
}

fn number(text: &str, key: &str) -> u32 {
    let at = text.find(key).unwrap_or_else(|| panic!("{key} missing"));
    text[at + key.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

#[test]
fn the_documented_verbs_are_the_answered_verbs() {
    let (doc, ans) = (documented(), answered());
    assert!(
        doc.contains("LAUNCHAPI") && doc.contains("HEALTH") && doc.len() >= 20,
        "{doc:?}"
    );
    assert_eq!(
        doc,
        ans,
        "documented but not answered: {:?}; answered but not documented: {:?}",
        doc.difference(&ans).collect::<Vec<_>>(),
        ans.difference(&doc).collect::<Vec<_>>()
    );
}

#[test]
fn the_handler_bound_is_socats_and_leases_check_it_first() {
    assert_eq!(
        number(AGENT, "HANDLERS_MAX="),
        number(INIT, "max-children=")
    );
    assert!(number(AGENT, "CONTROL_RESERVE=") > 0);
    for verb in ["LAUNCHAPI", "FCAPI"] {
        let arm = AGENT
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{verb})")))
            .unwrap();
        let (check, exec) = (
            arm.find("lease_room").expect("lease_room"),
            arm.find("exec ").expect("exec"),
        );
        assert!(
            check < exec,
            "{verb} takes the connection before it checks for a slot: {arm}"
        );
    }
}

#[test]
fn an_idle_connection_is_closed() {
    assert!(number(AGENT, "AGENT_IDLE_S=") >= 60);
    assert!(AGENT.contains("while IFS= read -r -t \"$AGENT_IDLE_S\" line; do"));
}

#[test]
fn the_protocol_version_is_the_clients() {
    let v = vesper::control::AGENT_PROTOCOL_VERSION;
    assert!(AGENT.contains(&format!("printf 'VESPER-AGENT {v} %s\\n'")));
    assert!(AGENT.contains(&format!("VERSION) echo \"VERSION {v}\" ;;")));
}
