//! Boot smoke check: run the launcher N times and report pass, fail and boot time (`systemvmctl boot-check`).

use super::ensure_parent;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckConfig {
    pub iterations: u32,
    pub timeout_seconds: u64,
    pub command: String,
    pub args: Vec<String>,
    pub log_path: String,
    pub report_path: String,
    /// Fail an iteration whose boot time (launcher `ready` line, else wall clock) exceeds this.
    #[serde(default)]
    pub max_boot_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheckReport {
    pub iterations: u32,
    pub successful: u32,
    pub failed: u32,
    #[serde(default)]
    pub failures: Vec<String>,
    /// Wall-clock milliseconds per iteration, successful or not, in order.
    #[serde(default)]
    pub durations_ms: Vec<u64>,
    #[serde(default)]
    pub max_duration_ms: u64,
    #[serde(default)]
    pub mean_duration_ms: u64,
    /// Boot time per iteration: `elapsed_ms` of the launcher's `{"status":"ready",...}` line when the
    /// command printed one, otherwise the wall-clock duration.
    #[serde(default)]
    pub boot_ms: Vec<u64>,
    #[serde(default)]
    pub max_boot_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_boot_allowed_ms: Option<u64>,
}

pub fn run_boot_check(config: &BootCheckConfig) -> Result<BootCheckReport, String> {
    if config.iterations == 0 {
        return Err("iterations must be > 0".to_string());
    }
    if config.command.trim().is_empty() {
        return Err("boot command is required".to_string());
    }
    if config.log_path.trim().is_empty() {
        return Err("log path is required".to_string());
    }
    if config.report_path.trim().is_empty() {
        return Err("report path is required".to_string());
    }

    let log_path = Path::new(&config.log_path);
    let report_path = Path::new(&config.report_path);
    ensure_parent(log_path)?;
    ensure_parent(report_path)?;

    let mut report = BootCheckReport {
        iterations: config.iterations,
        successful: 0,
        failed: 0,
        failures: Vec::new(),
        durations_ms: Vec::new(),
        max_duration_ms: 0,
        mean_duration_ms: 0,
        boot_ms: Vec::new(),
        max_boot_ms: 0,
        max_boot_allowed_ms: config.max_boot_ms,
    };

    let mut log = String::new();
    let timeout = Duration::from_secs(config.timeout_seconds.max(1));

    for iteration in 1..=config.iterations {
        log.push_str(&format!(
            "iteration={iteration} command={}\n",
            config.command
        ));
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn boot command: {err}"))?;
        let stdout = child.stdout.take().ok_or("capture boot command stdout")?;
        let reader = thread::spawn(move || {
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::BufReader::new(stdout), &mut out);
            out
        });

        let started_at = Instant::now();
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|err| format!("wait boot command: {err}"))?
            {
                break Ok(status);
            }

            if started_at.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                break Err("timeout".to_string());
            }

            thread::sleep(Duration::from_millis(20));
        };

        let duration_ms = started_at.elapsed().as_millis() as u64;
        report.durations_ms.push(duration_ms);
        let output = reader.join().unwrap_or_default();
        for line in output.lines().filter(|line| !line.trim().is_empty()) {
            log.push_str(&format!("stdout={line}\n"));
        }
        let boot_ms = ready_elapsed_ms(&output).unwrap_or(duration_ms);
        report.boot_ms.push(boot_ms);
        let status = match (status, config.max_boot_ms) {
            (Ok(exit), Some(limit)) if exit.success() && boot_ms > limit => {
                Err(format!("boot took {boot_ms} ms, over the {limit} ms threshold"))
            }
            (other, _) => other,
        };
        match status {
            Ok(exit) if exit.success() => {
                report.successful += 1;
                log.push_str(&format!("result=ok duration_ms={duration_ms} boot_ms={boot_ms}\n"));
            }
            Ok(exit) => {
                report.failed += 1;
                let message = format!(
                    "iteration {iteration}: exit status {}",
                    exit.code().unwrap_or(-1)
                );
                report.failures.push(message.clone());
                log.push_str(&format!("result=failed error={message}\n"));
            }
            Err(error) => {
                report.failed += 1;
                let message = format!("iteration {iteration}: {error}");
                report.failures.push(message.clone());
                log.push_str(&format!("result=failed error={message}\n"));
            }
        }
    }

    report.max_duration_ms = report.durations_ms.iter().copied().max().unwrap_or(0);
    report.max_boot_ms = report.boot_ms.iter().copied().max().unwrap_or(0);
    report.mean_duration_ms = report.durations_ms.iter().sum::<u64>() / report.durations_ms.len().max(1) as u64;

    fs::write(log_path, log)
        .map_err(|err| format!("write boot log {}: {err}", log_path.display()))?;

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("serialize boot report: {err}"))?;
    report_json.push('\n');
    fs::write(report_path, report_json)
        .map_err(|err| format!("write boot report {}: {err}", report_path.display()))?;

    if report.failed > 0 {
        return Err(format!(
            "boot check failed in {}/{} iterations",
            report.failed, report.iterations
        ));
    }

    Ok(report)
}

/// `elapsed_ms` from the first `{"status":"ready",...}` JSON line the launcher printed.
fn ready_elapsed_ms(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .find(|value| value.get("status").and_then(|s| s.as_str()) == Some("ready"))
        .and_then(|value| value.get("elapsed_ms").and_then(|ms| ms.as_u64()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn boot_check_detects_failure() {
        let dir = tempdir().unwrap();
        let config = BootCheckConfig {
            iterations: 1,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 1".to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: dir.path().join("boot-report.json").display().to_string(),
            max_boot_ms: None,
        };

        let err = run_boot_check(&config).unwrap_err();
        assert!(err.contains("boot check failed"));
    }

    #[test]
    fn boot_check_reads_launcher_ready_time_and_enforces_threshold() {
        let dir = tempdir().unwrap();
        let script = r#"echo '{"status":"ready","elapsed_ms":1200,"ready_pattern":"login:"}'; echo '{"status":"stopped","elapsed_ms":1900}'"#;
        let mut config = BootCheckConfig {
            iterations: 1,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: dir.path().join("boot-report.json").display().to_string(),
            max_boot_ms: Some(5000),
        };
        let report = run_boot_check(&config).unwrap();
        assert_eq!(report.boot_ms, vec![1200]);
        assert_eq!(report.max_boot_ms, 1200);
        assert_eq!(report.max_boot_allowed_ms, Some(5000));
        let log = fs::read_to_string(&config.log_path).unwrap();
        assert!(log.contains("boot_ms=1200"));
        assert!(log.contains(r#"stdout={"status":"ready""#));

        config.max_boot_ms = Some(1000);
        let err = run_boot_check(&config).unwrap_err();
        assert!(err.contains("boot check failed"));
        let written: BootCheckReport =
            serde_json::from_str(&fs::read_to_string(&config.report_path).unwrap()).unwrap();
        assert_eq!(written.failed, 1);
        assert!(written.failures[0].contains("over the 1000 ms threshold"));
    }

    #[test]
    fn boot_check_records_durations() {
        let dir = tempdir().unwrap();
        let report_path = dir.path().join("boot-report.json");
        let config = BootCheckConfig {
            iterations: 2,
            timeout_seconds: 5,
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 0".to_string()],
            log_path: dir.path().join("boot.log").display().to_string(),
            report_path: report_path.display().to_string(),
            max_boot_ms: None,
        };

        let report = run_boot_check(&config).unwrap();
        assert_eq!(report.durations_ms.len(), 2);
        assert_eq!(report.boot_ms, report.durations_ms, "no ready line: boot_ms falls back to wall clock");
        assert!(report.max_duration_ms >= report.mean_duration_ms);
        let written: BootCheckReport =
            serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
        assert_eq!(written.durations_ms, report.durations_ms);
        let log = fs::read_to_string(dir.path().join("boot.log")).unwrap();
        assert_eq!(log.matches("duration_ms=").count(), 2);
    }
}
