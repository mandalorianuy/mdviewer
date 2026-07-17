use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::{io::Write, os::unix::fs::OpenOptionsExt, process::Command, thread, time::Duration};

use crate::macos_integration::{IntegrationError, IntegrationStatus};

pub const PRINTER_NAME: &str = "MDViewer_Save_as_Markdown";
pub const PRINTER_URI: &str = "ipp://localhost:8631/ipp/print";
pub const DISPLAY_NAME: &str = "MDViewer — Guardar como Markdown";
const LAUNCH_AGENT_LABEL: &str = "com.mdviewer.desktop.virtual-printer";
const MANAGED_MARKER: &str = "MDVIEWER-VIRTUAL-PRINTER/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualPrinterPaths {
    pub root: PathBuf,
    pub helper: PathBuf,
    pub spool: PathBuf,
    pub launch_agent: PathBuf,
}

impl VirtualPrinterPaths {
    pub fn new(home: &Path) -> Result<Self, IntegrationError> {
        if !home.is_absolute() {
            return Err(IntegrationError::UnsafeTarget);
        }
        let root = home.join("Library/Application Support/com.mdviewer.desktop/Virtual Printer");
        Ok(Self {
            helper: root.join("submit-job"),
            spool: root.join("spool"),
            launch_agent: home
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCH_AGENT_LABEL}.plist")),
            root,
        })
    }
}

pub fn helper_contents(application: &Path) -> Result<String, IntegrationError> {
    if !application.is_absolute()
        || application
            .extension()
            .is_none_or(|extension| extension != "app")
    {
        return Err(IntegrationError::InvalidArtifact);
    }
    let application = application
        .to_str()
        .ok_or(IntegrationError::InvalidArtifact)?;
    let quoted = format!("'{}'", application.replace('\'', "'\\''"));
    Ok(format!(
        "#!/bin/zsh\n# MDVIEWER-VIRTUAL-PRINTER/v1\nset -eu\n[[ $# -eq 1 && -f \"$1\" ]] || exit 64\n/usr/bin/open -a {quoted} -- \"$1\"\n"
    ))
}

#[must_use]
pub fn launch_agent_contents(paths: &VirtualPrinterPaths) -> String {
    let helper = xml_escape(&paths.helper.to_string_lossy());
    let spool = xml_escape(&paths.spool.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- MDVIEWER-VIRTUAL-PRINTER/v1 -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/ippeveprinter</string>
    <string>--no-web-forms</string>
    <string>-f</string><string>application/pdf</string>
    <string>-F</string><string>application/pdf</string>
    <string>-c</string><string>{helper}</string>
    <string>-d</string><string>{spool}</string>
    <string>-k</string>
    <string>-n</string><string>localhost</string>
    <string>-p</string><string>8631</string>
    <string>-r</string><string>off</string>
    <string>-M</string><string>MDViewer</string>
    <string>-m</string><string>Save as Markdown</string>
    <string>{DISPLAY_NAME}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#
    )
}

#[must_use]
pub fn queue_uri_from_lpstat(output: &str) -> Option<&str> {
    let prefix = format!("device for {PRINTER_NAME}: ");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
}

pub fn remove_managed_spool_source(source: &Path, home: &Path) -> Result<bool, IntegrationError> {
    let paths = VirtualPrinterPaths::new(home)?;
    let source_metadata = match fs::symlink_metadata(source) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(IntegrationError::Io),
    };
    if !source_metadata.file_type().is_file() || source_metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let spool = match fs::canonicalize(&paths.spool) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(IntegrationError::Io),
    };
    let canonical_source = fs::canonicalize(source).map_err(|_| IntegrationError::Io)?;
    if canonical_source
        .parent()
        .is_none_or(|parent| !parent.starts_with(&spool))
    {
        return Ok(false);
    }
    let sidecar = source.with_extension("prn");
    fs::remove_file(source).map_err(|_| IntegrationError::Io)?;
    if source
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        match fs::symlink_metadata(&sidecar) {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                fs::remove_file(sidecar).map_err(|_| IntegrationError::Io)?;
            }
            Ok(_) => return Err(IntegrationError::UnsafeTarget),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(IntegrationError::Io),
        }
    }
    Ok(true)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub struct VirtualPrinterManager {
    paths: VirtualPrinterPaths,
    application: PathBuf,
}

impl VirtualPrinterManager {
    pub fn new(home: &Path, application: &Path) -> Result<Self, IntegrationError> {
        let _ = helper_contents(application)?;
        Ok(Self {
            paths: VirtualPrinterPaths::new(home)?,
            application: application.to_path_buf(),
        })
    }

    #[must_use]
    pub fn paths(&self) -> &VirtualPrinterPaths {
        &self.paths
    }

    pub fn status(&self) -> Result<IntegrationStatus, IntegrationError> {
        let helper = read_regular(&self.paths.helper)?;
        let launch_agent = read_regular(&self.paths.launch_agent)?;
        let queue = current_queue_uri()?;
        if helper.is_none() && launch_agent.is_none() && queue.is_none() {
            return Ok(IntegrationStatus::NotInstalled);
        }
        if queue.as_deref().is_some_and(|uri| uri != PRINTER_URI) {
            return Ok(IntegrationStatus::Invalid);
        }
        let expected_plist = launch_agent_contents(&self.paths);
        let expected_helper = helper_contents(&self.application)?;
        if helper.as_deref() == Some(expected_helper.as_bytes())
            && launch_agent.as_deref() == Some(expected_plist.as_bytes())
            && queue.as_deref() == Some(PRINTER_URI)
        {
            return Ok(IntegrationStatus::Installed);
        }
        let managed_helper = helper.as_deref().is_none_or(contains_marker);
        let managed_agent = launch_agent.as_deref().is_none_or(contains_marker);
        if managed_helper && managed_agent {
            Ok(IntegrationStatus::Outdated)
        } else {
            Ok(IntegrationStatus::Invalid)
        }
    }

    #[cfg(target_os = "macos")]
    pub fn install(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::NotInstalled {
            return Err(IntegrationError::TargetExists);
        }
        ensure_private_directory(&self.paths.root)?;
        ensure_private_directory(&self.paths.spool)?;
        let launch_agents = self
            .paths
            .launch_agent
            .parent()
            .ok_or(IntegrationError::UnsafeTarget)?;
        fs::create_dir_all(launch_agents).map_err(|_| IntegrationError::Io)?;
        let helper = helper_contents(&self.application)?;
        write_new_private(&self.paths.helper, helper.as_bytes(), 0o700)?;
        let plist = launch_agent_contents(&self.paths);
        if let Err(error) = write_new_private(&self.paths.launch_agent, plist.as_bytes(), 0o600)
            .and_then(|_| bootstrap_agent(&self.paths.launch_agent))
            .and_then(|_| register_queue())
        {
            let _ = unregister_queue();
            let _ = bootout_agent();
            let _ = remove_managed_file(&self.paths.launch_agent);
            let _ = remove_managed_file(&self.paths.helper);
            return Err(error);
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn repair(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::Outdated {
            return Err(IntegrationError::UnsafeTarget);
        }
        self.remove_managed_components()?;
        self.install()
    }

    #[cfg(target_os = "macos")]
    pub fn uninstall(&self) -> Result<(), IntegrationError> {
        if self.status()? != IntegrationStatus::Installed {
            return Err(IntegrationError::UnsafeTarget);
        }
        self.remove_managed_components()
    }

    #[cfg(target_os = "macos")]
    fn remove_managed_components(&self) -> Result<(), IntegrationError> {
        let queue = current_queue_uri()?;
        if queue.as_deref().is_some_and(|uri| uri != PRINTER_URI) {
            return Err(IntegrationError::UnsafeTarget);
        }
        for path in [&self.paths.helper, &self.paths.launch_agent] {
            if let Some(bytes) = read_regular(path)?
                && !contains_marker(&bytes)
            {
                return Err(IntegrationError::UnsafeTarget);
            }
        }
        if queue.is_some() {
            unregister_queue()?;
        }
        let _ = bootout_agent();
        remove_managed_file(&self.paths.launch_agent)?;
        remove_managed_file(&self.paths.helper)?;
        Ok(())
    }
}

fn contains_marker(bytes: &[u8]) -> bool {
    bytes
        .windows(MANAGED_MARKER.len())
        .any(|window| window == MANAGED_MARKER.as_bytes())
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, IntegrationError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(IntegrationError::Io),
    };
    if !metadata.file_type().is_file() {
        return Err(IntegrationError::UnsafeTarget);
    }
    fs::read(path).map(Some).map_err(|_| IntegrationError::Io)
}

#[cfg(target_os = "macos")]
fn ensure_private_directory(path: &Path) -> Result<(), IntegrationError> {
    fs::create_dir_all(path).map_err(|_| IntegrationError::Io)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| IntegrationError::Io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(IntegrationError::UnsafeTarget);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_new_private(path: &Path, bytes: &[u8], mode: u32) -> Result<(), IntegrationError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options.open(path).map_err(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => IntegrationError::TargetExists,
        _ => IntegrationError::Io,
    })?;
    file.write_all(bytes).map_err(|_| IntegrationError::Io)?;
    file.sync_all().map_err(|_| IntegrationError::Io)
}

#[cfg(target_os = "macos")]
fn current_queue_uri() -> Result<Option<String>, IntegrationError> {
    let output = Command::new("/usr/bin/lpstat")
        .args(["-v", PRINTER_NAME])
        .output()
        .map_err(|_| IntegrationError::Io)?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(queue_uri_from_lpstat(&stdout).map(ToOwned::to_owned))
}

#[cfg(not(target_os = "macos"))]
fn current_queue_uri() -> Result<Option<String>, IntegrationError> {
    Ok(None)
}

#[cfg(target_os = "macos")]
fn bootstrap_agent(plist: &Path) -> Result<(), IntegrationError> {
    let domain = format!("gui/{}", unsafe { libc::geteuid() });
    let status = Command::new("/bin/launchctl")
        .args(["bootstrap", &domain])
        .arg(plist)
        .status()
        .map_err(|_| IntegrationError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(IntegrationError::Io)
    }
}

#[cfg(target_os = "macos")]
fn bootout_agent() -> Result<(), IntegrationError> {
    let service = format!("gui/{}/{LAUNCH_AGENT_LABEL}", unsafe { libc::geteuid() });
    let _ = Command::new("/bin/launchctl")
        .args(["bootout", &service])
        .status();
    Ok(())
}

#[cfg(target_os = "macos")]
fn register_queue() -> Result<(), IntegrationError> {
    let mut last_success = false;
    for _ in 0..10 {
        let status = Command::new("/usr/sbin/lpadmin")
            .args([
                "-p",
                PRINTER_NAME,
                "-E",
                "-v",
                PRINTER_URI,
                "-m",
                "everywhere",
                "-D",
                DISPLAY_NAME,
            ])
            .status()
            .map_err(|_| IntegrationError::Io)?;
        if status.success() {
            last_success = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    if last_success {
        Ok(())
    } else {
        Err(IntegrationError::Io)
    }
}

#[cfg(target_os = "macos")]
fn unregister_queue() -> Result<(), IntegrationError> {
    let status = Command::new("/usr/sbin/lpadmin")
        .args(["-x", PRINTER_NAME])
        .status()
        .map_err(|_| IntegrationError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(IntegrationError::Io)
    }
}

#[cfg(target_os = "macos")]
fn remove_managed_file(path: &Path) -> Result<(), IntegrationError> {
    let Some(bytes) = read_regular(path)? else {
        return Ok(());
    };
    if !contains_marker(&bytes) {
        return Err(IntegrationError::UnsafeTarget);
    }
    fs::remove_file(path).map_err(|_| IntegrationError::Io)
}

pub fn embedded_manager() -> Result<VirtualPrinterManager, IntegrationError> {
    let home = std::env::var_os("HOME").ok_or(IntegrationError::UnsafeTarget)?;
    let executable = std::env::current_exe().map_err(|_| IntegrationError::InvalidArtifact)?;
    let application = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or(IntegrationError::InvalidArtifact)?;
    VirtualPrinterManager::new(Path::new(&home), application)
}
