//! Finding the running League client and its per-launch credentials.
//!
//! The client writes `name:pid:port:password:protocol` into a `lockfile` in its
//! install directory, and passes the same port and password to `LeagueClientUx`
//! as `--app-port` / `--remoting-auth-token`. Both change on every launch.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub port: u16,
    pub password: String,
    pub pid: Option<u32>,
}

impl Credentials {
    /// Value for the `Authorization` header (`Basic riot:<password>`).
    pub fn auth_header(&self) -> String {
        let token =
            base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", self.password));
        format!("Basic {token}")
    }
}

// Never print the password, even in debug logs.
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("port", &self.port)
            .field("password", &"<hidden>")
            .field("pid", &self.pid)
            .finish()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("League client not running (no LeagueClientUx process and no lockfile found)")]
    NotFound,
    #[error("malformed lockfile: {0}")]
    MalformedLockfile(String),
}

/// Parses lockfile contents: `name:pid:port:password:protocol`.
pub fn parse_lockfile(contents: &str) -> Result<Credentials, DiscoveryError> {
    let parts: Vec<&str> = contents.trim().split(':').collect();
    let [_name, pid, port, password, _protocol] = parts.as_slice() else {
        return Err(DiscoveryError::MalformedLockfile(format!(
            "expected 5 fields, got {}",
            parts.len()
        )));
    };
    let port = port
        .parse::<u16>()
        .map_err(|_| DiscoveryError::MalformedLockfile(format!("bad port {port:?}")))?;
    if password.is_empty() {
        return Err(DiscoveryError::MalformedLockfile("empty password".into()));
    }
    Ok(Credentials {
        port,
        password: (*password).to_string(),
        pid: pid.parse().ok(),
    })
}

/// Extracts credentials from `LeagueClientUx` command-line arguments.
/// Accepts both `--flag=value` and `"--flag=value"` (quoted, as some process
/// listings report them).
pub fn parse_process_args<I, S>(args: I) -> Option<Credentials>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut port = None;
    let mut password = None;
    let mut pid = None;
    for arg in args {
        let arg = arg.as_ref().trim_matches('"');
        if let Some(v) = arg.strip_prefix("--app-port=") {
            port = v.parse::<u16>().ok();
        } else if let Some(v) = arg.strip_prefix("--remoting-auth-token=") {
            password = Some(v.to_string()).filter(|s| !s.is_empty());
        } else if let Some(v) = arg.strip_prefix("--app-pid=") {
            pid = v.parse().ok();
        }
    }
    Some(Credentials {
        port: port?,
        password: password?,
        pid,
    })
}

/// Install directories to look for a lockfile in, most specific first:
/// `LEAGUE_DIR`, then the default Windows and macOS install paths.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("LEAGUE_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(PathBuf::from(r"C:\Riot Games\League of Legends"));
    dirs.push(PathBuf::from(
        "/Applications/League of Legends.app/Contents/LoL",
    ));
    dirs
}

pub fn read_lockfile(dir: &Path) -> Option<Result<Credentials, DiscoveryError>> {
    let contents = std::fs::read_to_string(dir.join("lockfile")).ok()?;
    Some(parse_lockfile(&contents))
}

/// Finds the running client. Process arguments win over a lockfile because a
/// lockfile can be left behind by a crashed client.
pub fn discover() -> Result<Credentials, DiscoveryError> {
    let mut dirs = Vec::new();
    for proc in client_processes() {
        if let Some(creds) = parse_process_args(&proc.args) {
            return Ok(creds);
        }
        if let Some(dir) = proc.exe_dir {
            dirs.push(dir);
        }
    }
    dirs.extend(candidate_dirs());
    let mut last_err = DiscoveryError::NotFound;
    for dir in dirs {
        match read_lockfile(&dir) {
            Some(Ok(creds)) => return Ok(creds),
            Some(Err(e)) => last_err = e,
            None => {}
        }
    }
    Err(last_err)
}

struct ClientProcess {
    args: Vec<String>,
    exe_dir: Option<PathBuf>,
}

fn client_processes() -> Vec<ClientProcess> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(mut sys) = System::new() else {
        tracing::warn!("process list unavailable; falling back to lockfile lookup");
        return Vec::new();
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    sys.processes()
        .values()
        .filter(|p| {
            let name = p.name().to_string_lossy().to_ascii_lowercase();
            name == "leagueclientux.exe" || name == "leagueclientux"
        })
        .map(|p| ClientProcess {
            args: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            exe_dir: p.exe().and_then(|e| e.parent()).map(Path::to_path_buf),
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/discovery.rs"]
mod tests;
