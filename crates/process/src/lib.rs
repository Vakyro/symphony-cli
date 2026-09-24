//! Supervisión de procesos (STACK §7, ADR-0002, ADR-0005): cada executor corre
//! en su propio grupo contenido por el SO (Job Object en Windows, cgroup v2 o
//! process group en Unix). Matar el grupo mata todo el árbol, sin huérfanos.
//!
//! Regla (PLAN P04.S3): ningún tipo de ProcessKit sale de este crate.
//! Sin PTY en v0.1 (ADR-0005): stdin por pipe, stdout/stderr por líneas.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use processkit::prelude::StreamExt;
use processkit::{Command, Outcome, ProcessEvent, ProcessGroup, ProcessGroupOptions, ProcessStdin};
use tokio::sync::{mpsc, oneshot};

/// Líneas que se pueden acumular sin que nadie las lea antes de frenar al hijo.
const OUTPUT_BUFFER: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("no se pudo arrancar `{program}`: {detail}")]
    Spawn { program: String, detail: String },
    #[error("control del grupo de procesos: {0}")]
    Control(String),
    #[error("el proceso no tiene stdin abierto")]
    NoStdin,
    #[error("escritura a stdin: {0}")]
    Stdin(#[from] std::io::Error),
}

fn control(e: impl std::fmt::Display) -> ProcessError {
    ProcessError::Control(e.to_string())
}

/// Límites duros del grupo. Donde el SO no los soporta, no se aplican
/// (ver `Guarantees`) y el scheduler limita por concurrencia.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Limits {
    pub max_memory_bytes: Option<u64>,
    pub cpu_cores: Option<f64>,
    pub max_processes: Option<u32>,
}

impl Limits {
    fn is_empty(&self) -> bool {
        self.max_memory_bytes.is_none() && self.cpu_cores.is_none() && self.max_processes.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    /// Deja stdin abierto para escribirle después (Claude `--input-format stream-json`).
    pub stdin: bool,
    pub limits: Limits,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    pub fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, k: impl Into<OsString>, v: impl Into<OsString>) -> Self {
        self.env.push((k.into(), v.into()));
        self
    }

    pub fn with_stdin(mut self) -> Self {
        self.stdin = true;
        self
    }

    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
}

/// Una línea de salida del proceso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputLine {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Exited(i32),
    /// Terminado por señal (Unix) o por `terminate_tree` / kill del grupo.
    Killed(Option<i32>),
    /// El supervisor perdió el proceso (error interno).
    Unknown,
}

impl ExitStatus {
    pub fn success(self) -> bool {
        self == Self::Exited(0)
    }
}

/// Qué garantías da el SO para este grupo (ADR-0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guarantees {
    /// `JOB_OBJECT`, `CGROUP_V2`, `PROCESS_GROUP` o `PROCESS_REAPER`.
    pub mechanism: &'static str,
    /// `false` si se pidieron límites y el SO no los puede aplicar.
    pub limits_enforced: bool,
    /// Por qué no se aplicaron, si es el caso.
    pub limits_note: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessStats {
    pub active_processes: usize,
    pub memory_bytes: Option<u64>,
    pub peak_memory_bytes: Option<u64>,
    pub cpu_time: Option<Duration>,
}

/// Un proceso raíz con todo su árbol, contenido en su propio grupo.
/// Al soltarlo, el SO mata el árbol completo.
pub struct Supervised {
    group: ProcessGroup,
    pid: Option<u32>,
    stdin: Option<ProcessStdin>,
    output: mpsc::Receiver<OutputLine>,
    exit: Option<oneshot::Receiver<ExitStatus>>,
    exit_status: Option<ExitStatus>,
    guarantees: Guarantees,
}

fn mechanism_name(group: &ProcessGroup) -> &'static str {
    match format!("{:?}", group.mechanism()).as_str() {
        "JobObject" => "JOB_OBJECT",
        "CgroupV2" => "CGROUP_V2",
        "ProcessReaper" => "PROCESS_REAPER",
        _ => "PROCESS_GROUP",
    }
}

/// Crea el grupo con límites; si el SO no los soporta, lo crea sin ellos y lo informa.
fn make_group(limits: Limits) -> Result<(ProcessGroup, bool, Option<String>), ProcessError> {
    if limits.is_empty() {
        return Ok((ProcessGroup::new().map_err(control)?, true, None));
    }
    let mut opts = ProcessGroupOptions::default();
    if let Some(m) = limits.max_memory_bytes {
        opts = opts.max_memory(m);
    }
    if let Some(c) = limits.cpu_cores {
        opts = opts.cpu_quota(c);
    }
    if let Some(p) = limits.max_processes {
        opts = opts.max_processes(p);
    }
    match ProcessGroup::with_options(opts) {
        Ok(g) => Ok((g, true, None)),
        Err(e) => Ok((
            ProcessGroup::new().map_err(control)?,
            false,
            Some(e.to_string()),
        )),
    }
}

/// Lanza el proceso en un grupo nuevo y empieza a leer su salida.
pub async fn spawn(spec: ProcessSpec) -> Result<Supervised, ProcessError> {
    let program = spec.program.display().to_string();
    let (group, limits_enforced, limits_note) = make_group(spec.limits)?;
    let mut cmd = Command::new(&spec.program).args(&spec.args).no_timeout();
    if let Some(dir) = &spec.cwd {
        cmd = cmd.current_dir(dir);
    }
    for (k, v) in &spec.env {
        cmd = cmd.env(k, v);
    }
    if spec.stdin {
        cmd = cmd.keep_stdin_open();
    }
    let mut run = group.start(&cmd).await.map_err(|e| ProcessError::Spawn {
        program: program.clone(),
        detail: e.to_string(),
    })?;
    let pid = run.pid();
    let stdin = if spec.stdin { run.take_stdin() } else { None };
    let mut events = run.events().map_err(control)?;

    let (out_tx, out_rx) = mpsc::channel(OUTPUT_BUFFER);
    let (exit_tx, exit_rx) = oneshot::channel();
    tokio::spawn(async move {
        let forward = async {
            while let Some(ev) = events.next().await {
                let line = match ev {
                    ProcessEvent::Stdout(l) => OutputLine::Stdout(l.text().to_string()),
                    ProcessEvent::Stderr(l) => OutputLine::Stderr(l.text().to_string()),
                    _ => continue,
                };
                // Si nadie escucha, igual hay que drenar la salida para que el hijo no se trabe.
                let _ = out_tx.send(line).await;
            }
        };
        let (_, finished) = tokio::join!(forward, run.finish());
        let status = match finished.map(|f| f.outcome) {
            Ok(Outcome::Exited(code)) => ExitStatus::Exited(code),
            Ok(Outcome::Signalled(sig)) => ExitStatus::Killed(sig),
            Ok(_) | Err(_) => ExitStatus::Unknown,
        };
        let _ = exit_tx.send(status);
    });

    let guarantees = Guarantees {
        mechanism: mechanism_name(&group),
        limits_enforced,
        limits_note,
    };
    Ok(Supervised {
        group,
        pid,
        stdin,
        output: out_rx,
        exit: Some(exit_rx),
        exit_status: None,
        guarantees,
    })
}

impl Supervised {
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn guarantees(&self) -> &Guarantees {
        &self.guarantees
    }

    /// Siguiente línea de salida, o `None` cuando el proceso cerró stdout y stderr.
    pub async fn next_output(&mut self) -> Option<OutputLine> {
        self.output.recv().await
    }

    pub async fn write_stdin(&mut self, bytes: &[u8]) -> Result<(), ProcessError> {
        let stdin = self.stdin.as_mut().ok_or(ProcessError::NoStdin)?;
        stdin.write(bytes).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Cierra stdin (EOF para el hijo).
    pub async fn close_stdin(&mut self) -> Result<(), ProcessError> {
        match self.stdin.take() {
            Some(s) => Ok(s.finish().await?),
            None => Ok(()),
        }
    }

    /// Mata el árbol completo (hijos y nietos incluidos).
    pub fn terminate_tree(&self) -> Result<(), ProcessError> {
        self.group.kill_all().map_err(control)
    }

    /// Congela el árbol completo (Job Object / cgroup freezer / SIGSTOP).
    pub fn suspend(&self) -> Result<(), ProcessError> {
        self.group.suspend().map_err(control)
    }

    pub fn resume(&self) -> Result<(), ProcessError> {
        self.group.resume().map_err(control)
    }

    /// Estadísticas del grupo. Donde el SO no las da, la memoria se suma con sysinfo.
    pub fn stats(&self) -> ProcessStats {
        let members = self.group.members().unwrap_or_default();
        let from_group = self.group.stats().ok();
        let memory_bytes = {
            let mut sys = sysinfo::System::new();
            let pids: Vec<sysinfo::Pid> =
                members.iter().map(|p| sysinfo::Pid::from_u32(*p)).collect();
            sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&pids),
                true,
                sysinfo::ProcessRefreshKind::nothing().with_memory(),
            );
            let total: u64 = pids
                .iter()
                .filter_map(|p| sys.process(*p))
                .map(sysinfo::Process::memory)
                .sum();
            (!pids.is_empty()).then_some(total)
        };
        ProcessStats {
            active_processes: from_group
                .as_ref()
                .map_or(members.len(), |s| s.active_process_count),
            memory_bytes,
            peak_memory_bytes: from_group.as_ref().and_then(|s| s.peak_memory_bytes),
            cpu_time: from_group.and_then(|s| s.total_cpu_time),
        }
    }

    /// Espera a que termine el proceso raíz y devuelve cómo terminó.
    pub async fn wait(&mut self) -> ExitStatus {
        if let Some(s) = self.exit_status {
            return s;
        }
        let status = match self.exit.take() {
            Some(rx) => rx.await.unwrap_or(ExitStatus::Unknown),
            None => ExitStatus::Unknown,
        };
        self.exit_status = Some(status);
        status
    }

    /// `wait` con límite de tiempo; `None` si todavía corre.
    pub async fn wait_timeout(&mut self, timeout: Duration) -> Option<ExitStatus> {
        if let Some(s) = self.exit_status {
            return Some(s);
        }
        let rx = self.exit.as_mut()?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(r) => {
                let status = r.unwrap_or(ExitStatus::Unknown);
                self.exit = None;
                self.exit_status = Some(status);
                Some(status)
            }
            Err(_) => None,
        }
    }
}
