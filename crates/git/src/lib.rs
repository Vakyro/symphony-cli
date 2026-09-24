//! Wrapper del CLI oficial de Git (STACK §12.1): se ejecuta el `git` del
//! usuario y se parsean solo formatos pensados para máquinas
//! (`--porcelain=v2 -z`, `--numstat -z`, `worktree list --porcelain -z`).
//!
//! Cada llamada corre con `GIT_TERMINAL_PROMPT=0` (nunca pide credenciales)
//! y `LC_ALL=C` (mensajes estables), sin pager ni colores.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("no se pudo ejecutar git: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("`git {args}` falló ({code}): {stderr}")]
    Failed {
        args: String,
        code: String,
        stderr: String,
    },
    #[error("salida inesperada de `git {args}`: {detail}")]
    Parse { args: String, detail: String },
}

/// Un repositorio (o worktree) sobre el que se corren comandos.
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

/// Entrada de `git worktree list --porcelain -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub head: Option<String>,
    /// `refs/heads/...` sin el prefijo; `None` si está en detached HEAD.
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// Estado de un archivo según `git status --porcelain=v2 -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// `1`/`2`: cambio en index (X) y/o worktree (Y), p. ej. `.M`, `A.`, `R.`.
    Changed {
        xy: String,
        path: String,
        renamed_from: Option<String>,
    },
    /// `u`: conflicto de merge.
    Unmerged {
        xy: String,
        path: String,
    },
    Untracked {
        path: String,
    },
    Ignored {
        path: String,
    },
}

impl FileStatus {
    pub fn path(&self) -> &str {
        match self {
            Self::Changed { path, .. }
            | Self::Unmerged { path, .. }
            | Self::Untracked { path }
            | Self::Ignored { path } => path,
        }
    }
}

/// Una línea de `git diff --numstat -z`. `None` en archivos binarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumStat {
    pub path: String,
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    pub renamed_from: Option<String>,
}

/// Resultado de `merge-tree --write-tree`: si la rama entra limpia en la base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePreflight {
    Clean { tree: String },
    Conflicts { paths: Vec<String> },
}

/// Nombre de rama de un agente (IDEA §5.9): `symphony/<sesión>/agent-003`.
pub fn agent_branch(session: &str, agent_number: u32) -> String {
    format!("symphony/{session}/agent-{agent_number:03}")
}

/// Versión del git instalado, p. ej. `2.47.1.windows.1`.
pub fn version() -> Result<String, GitError> {
    let out = git_cmd(None, ["--version"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .strip_prefix("git version ")
        .map(str::to_string)
        .ok_or_else(|| GitError::Parse {
            args: "--version".into(),
            detail: text.into_owned(),
        })
}

fn git_cmd<I, S>(cwd: Option<&Path>, args: I) -> Result<Output, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .collect();
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(["-c", "color.ui=false", "-c", "core.quotepath=false"])
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    let out = cmd.output().map_err(GitError::Spawn)?;
    if out.status.success() {
        return Ok(out);
    }
    Err(GitError::Failed {
        args: args
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        code: out.status.code().map_or("señal".into(), |c| c.to_string()),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// Separa una salida `-z` en registros (el último NUL es terminador, no separador).
fn nul_fields(bytes: &[u8]) -> Vec<String> {
    let mut v: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    if v.last().is_some_and(String::is_empty) {
        v.pop();
    }
    v
}

impl Repo {
    /// Abre el repositorio que contiene `path` (su raíz de trabajo).
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        let out = git_cmd(Some(path), ["rev-parse", "--show-toplevel"])?;
        let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Self {
            root: PathBuf::from(top),
        })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn run<I, S>(&self, args: I) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        git_cmd(Some(&self.root), args)
    }

    fn text<I, S>(&self, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Ok(String::from_utf8_lossy(&self.run(args)?.stdout)
            .trim()
            .to_string())
    }

    pub fn head_commit(&self) -> Result<String, GitError> {
        self.text(["rev-parse", "HEAD"])
    }

    /// Rama actual, o `None` en detached HEAD.
    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        let b = self.text(["branch", "--show-current"])?;
        Ok((!b.is_empty()).then_some(b))
    }

    /// `git worktree add -b <branch> <path> <base>`: worktree nuevo con rama nueva.
    pub fn worktree_add(
        &self,
        path: &Path,
        branch: &str,
        base_ref: &str,
    ) -> Result<Repo, GitError> {
        self.run([
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(branch),
            path.as_os_str(),
            OsStr::new(base_ref),
        ])?;
        Ok(Repo::at(path))
    }

    pub fn worktree_list(&self) -> Result<Vec<WorktreeInfo>, GitError> {
        let out = self.run(["worktree", "list", "--porcelain", "-z"])?;
        let mut list = Vec::new();
        let mut cur: Option<WorktreeInfo> = None;
        for field in nul_fields(&out.stdout) {
            if field.is_empty() {
                list.extend(cur.take());
                continue;
            }
            let (key, value) = field.split_once(' ').unwrap_or((field.as_str(), ""));
            if key == "worktree" {
                list.extend(cur.take());
                cur = Some(WorktreeInfo {
                    path: PathBuf::from(value),
                    head: None,
                    branch: None,
                    bare: false,
                    locked: false,
                    prunable: false,
                });
                continue;
            }
            let Some(wt) = cur.as_mut() else {
                return Err(GitError::Parse {
                    args: "worktree list".into(),
                    detail: format!("campo sin worktree: {field}"),
                });
            };
            match key {
                "HEAD" => wt.head = Some(value.to_string()),
                "branch" => {
                    wt.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_string(),
                    )
                }
                "bare" => wt.bare = true,
                "locked" => wt.locked = true,
                "prunable" => wt.prunable = true,
                _ => {}
            }
        }
        list.extend(cur);
        Ok(list)
    }

    /// Quita el worktree. Con `force`, aunque tenga cambios sin commitear.
    pub fn worktree_remove(&self, path: &Path, force: bool) -> Result<(), GitError> {
        let mut args = vec![OsStr::new("worktree"), OsStr::new("remove")];
        if force {
            args.push(OsStr::new("--force"));
        }
        args.push(path.as_os_str());
        self.run(args)?;
        Ok(())
    }

    /// Limpia registros de worktrees cuyo directorio ya no existe.
    pub fn worktree_prune(&self) -> Result<(), GitError> {
        self.run(["worktree", "prune"])?;
        Ok(())
    }

    pub fn delete_branch(&self, branch: &str, force: bool) -> Result<(), GitError> {
        self.run(["branch", if force { "-D" } else { "-d" }, branch])?;
        Ok(())
    }

    pub fn status(&self) -> Result<Vec<FileStatus>, GitError> {
        let out = self.run(["status", "--porcelain=v2", "-z", "--untracked-files=all"])?;
        let fields = nul_fields(&out.stdout);
        let mut entries = Vec::new();
        let mut i = 0;
        let bad = |f: &str| GitError::Parse {
            args: "status --porcelain=v2".into(),
            detail: f.to_string(),
        };
        while i < fields.len() {
            let f = &fields[i];
            let kind = f.chars().next().unwrap_or(' ');
            match kind {
                // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
                '1' => {
                    let parts: Vec<&str> = f.splitn(9, ' ').collect();
                    let (Some(xy), Some(path)) = (parts.get(1), parts.get(8)) else {
                        return Err(bad(f));
                    };
                    entries.push(FileStatus::Changed {
                        xy: xy.to_string(),
                        path: path.to_string(),
                        renamed_from: None,
                    });
                }
                // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path> NUL <origPath>
                '2' => {
                    let parts: Vec<&str> = f.splitn(10, ' ').collect();
                    let (Some(xy), Some(path)) = (parts.get(1), parts.get(9)) else {
                        return Err(bad(f));
                    };
                    i += 1;
                    let from = fields.get(i).cloned().ok_or_else(|| bad(f))?;
                    entries.push(FileStatus::Changed {
                        xy: xy.to_string(),
                        path: path.to_string(),
                        renamed_from: Some(from),
                    });
                }
                // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
                'u' => {
                    let parts: Vec<&str> = f.splitn(11, ' ').collect();
                    let (Some(xy), Some(path)) = (parts.get(1), parts.get(10)) else {
                        return Err(bad(f));
                    };
                    entries.push(FileStatus::Unmerged {
                        xy: xy.to_string(),
                        path: path.to_string(),
                    });
                }
                '?' => entries.push(FileStatus::Untracked {
                    path: f[2..].to_string(),
                }),
                '!' => entries.push(FileStatus::Ignored {
                    path: f[2..].to_string(),
                }),
                '#' => {}
                _ => return Err(bad(f)),
            }
            i += 1;
        }
        Ok(entries)
    }

    /// Diff del worktree contra `base` (incluye cambios sin commitear), sin diff externo.
    pub fn diff(&self, base: &str) -> Result<String, GitError> {
        let out = self.run(["diff", "--no-ext-diff", "--no-color", base])?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn diff_numstat(&self, base: &str) -> Result<Vec<NumStat>, GitError> {
        let out = self.run(["diff", "--no-ext-diff", "--numstat", "-z", "-M", base])?;
        let fields = nul_fields(&out.stdout);
        let mut stats = Vec::new();
        let mut i = 0;
        while i < fields.len() {
            // "<add>\t<del>\t<path>" o, en renames, "<add>\t<del>\t" NUL <from> NUL <to>
            let mut parts = fields[i].splitn(3, '\t');
            let (Some(a), Some(d), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
                return Err(GitError::Parse {
                    args: "diff --numstat".into(),
                    detail: fields[i].clone(),
                });
            };
            let (path, renamed_from) = if rest.is_empty() {
                let from = fields.get(i + 1).cloned();
                let to = fields.get(i + 2).cloned();
                i += 2;
                match (from, to) {
                    (Some(from), Some(to)) => (to, Some(from)),
                    _ => {
                        return Err(GitError::Parse {
                            args: "diff --numstat".into(),
                            detail: "rename incompleto".into(),
                        });
                    }
                }
            } else {
                (rest.to_string(), None)
            };
            stats.push(NumStat {
                path,
                added: a.parse().ok(),
                deleted: d.parse().ok(),
                renamed_from,
            });
            i += 1;
        }
        Ok(stats)
    }

    /// ¿La rama `theirs` entra limpia en `ours`? Sin tocar ningún worktree.
    pub fn merge_preflight(&self, ours: &str, theirs: &str) -> Result<MergePreflight, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args([
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                "-z",
                ours,
                theirs,
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .map_err(GitError::Spawn)?;
        let fields = nul_fields(&out.stdout);
        let tree = fields.first().cloned().unwrap_or_default();
        // Exit 1 significa "conflictos" y también "no se pudo mergear" (p. ej. rama inexistente).
        // Solo el primero trae el OID del tree resultante.
        let has_tree = matches!(tree.len(), 40 | 64) && tree.bytes().all(|b| b.is_ascii_hexdigit());
        match out.status.code() {
            Some(0) => Ok(MergePreflight::Clean { tree }),
            // 1 = hay conflictos: después del tree vienen los archivos en conflicto.
            Some(1) if has_tree => {
                let mut paths: Vec<String> = fields
                    .into_iter()
                    .skip(1)
                    .filter(|p| !p.is_empty())
                    .collect();
                paths.dedup();
                Ok(MergePreflight::Conflicts { paths })
            }
            code => Err(GitError::Failed {
                args: format!("merge-tree --write-tree {ours} {theirs}"),
                code: code.map_or("señal".into(), |c| c.to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }),
        }
    }
}
