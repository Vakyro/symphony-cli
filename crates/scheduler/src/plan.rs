//! Plan por fases y tareas (ADR-0011, ADR-0013 D1): el archivo temporal
//! `.symphony/plan/<task_id>.md` del worktree. Texto puro, sin E/S.
//!
//! Formato (Markdown que cualquier modelo escribe sin esfuerzo):
//!
//! ```text
//! # Plan: arreglar la rotación de tokens
//!
//! ## Fase 1: preparar
//! Verifica: cargo test -p auth
//! - [x] [A1] Leer el módulo de tokens
//! - [ ] [A2] Escribir el test que falla (needs: A1)
//!   > falló: el fixture no existe
//!
//! ## Fase 2: arreglar
//! - [ ] Corregir la rotación
//! - [ ] Correr toda la suite
//! ```
//!
//! - `- [ ]` pendiente, `- [x]` hecha, `- [!]` falló.
//! - `[A1]` (opcional) es el id de la tarea; `(needs: A1, B2)` (opcional) son sus
//!   dependencias dentro del plan. Es el mismo formato para un lote de agentes (F3).
//! - `> ...` bajo una tarea es una nota (lo hecho o lo que falló).
//! - `Verifica: ...` bajo el encabezado de una fase es su comando de verificación.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::dag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMark {
    Pending,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub id: Option<String>,
    pub text: String,
    pub mark: TaskMark,
    pub needs: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub title: String,
    /// `Verifica: ...` de la fase.
    pub verify: Option<String>,
    pub tasks: Vec<PlanTask>,
}

impl Phase {
    pub fn is_done(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|t| t.mark == TaskMark::Done)
    }

    pub fn has_failure(&self) -> bool {
        self.tasks.iter().any(|t| t.mark == TaskMark::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub title: String,
    pub phases: Vec<Phase>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("el plan no tiene ninguna fase (`## Fase N: objetivo`)")]
    NoPhases,
    #[error("la fase «{0}» no tiene tareas")]
    EmptyPhase(String),
    #[error("la tarea «{task}» depende de `{needs}`, que no existe en el plan")]
    UnknownNeed { task: String, needs: String },
    #[error("hay un ciclo de dependencias: {0}")]
    Cycle(String),
    #[error("el id `{0}` está repetido")]
    DuplicateId(String),
}

/// Dónde se avanza: qué fase toca y cómo va.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Toca la fase `index` (0-based); las anteriores están completas.
    Phase { index: usize },
    /// Hay una tarea marcada como fallida en la fase `index`: el agente se detiene.
    Failed { index: usize },
    /// Todo el plan está hecho.
    Done,
}

impl Plan {
    pub fn parse(text: &str) -> Plan {
        let mut plan = Plan::default();
        for raw in text.lines() {
            let line = raw.trim_end();
            let trimmed = line.trim_start();
            if let Some(title) = line.strip_prefix("## ") {
                plan.phases.push(Phase {
                    title: strip_phase_prefix(title.trim()).to_string(),
                    verify: None,
                    tasks: Vec::new(),
                });
            } else if let Some(title) = line.strip_prefix("# ") {
                if plan.title.is_empty() {
                    let title = title.trim();
                    plan.title = strip_prefix_ci(title, "plan:")
                        .unwrap_or(title)
                        .trim()
                        .trim_start_matches('·')
                        .trim()
                        .to_string();
                }
            } else if let Some(phase) = plan.phases.last_mut() {
                if let Some(task) = parse_task(trimmed) {
                    phase.tasks.push(task);
                } else if let Some(note) = trimmed.strip_prefix('>') {
                    if let Some(t) = phase.tasks.last_mut() {
                        t.notes.push(note.trim().to_string());
                    }
                } else if phase.tasks.is_empty()
                    && let Some(v) = strip_prefix_ci(trimmed, "verifica:")
                        .or_else(|| strip_prefix_ci(trimmed, "verify:"))
                    && !v.trim().is_empty()
                {
                    phase.verify = Some(v.trim().to_string());
                }
            }
        }
        plan
    }

    /// Markdown del plan (el mismo formato que `parse` lee).
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Plan: {}\n", self.title);
        for (i, p) in self.phases.iter().enumerate() {
            let _ = writeln!(out, "## Fase {}: {}", i + 1, p.title);
            if let Some(v) = &p.verify {
                let _ = writeln!(out, "Verifica: {v}");
            }
            for t in &p.tasks {
                let mark = match t.mark {
                    TaskMark::Pending => ' ',
                    TaskMark::Done => 'x',
                    TaskMark::Failed => '!',
                };
                let id = t.id.as_ref().map_or(String::new(), |id| format!("[{id}] "));
                let needs = if t.needs.is_empty() {
                    String::new()
                } else {
                    format!(" (needs: {})", t.needs.join(", "))
                };
                let _ = writeln!(out, "- [{mark}] {id}{}{needs}", t.text);
                for n in &t.notes {
                    let _ = writeln!(out, "  > {n}");
                }
            }
            out.push('\n');
        }
        out
    }

    /// Valida el plan: fases con tareas, ids únicos, dependencias que existen y sin ciclos.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.phases.is_empty() {
            return Err(PlanError::NoPhases);
        }
        let mut ids: HashSet<&str> = HashSet::new();
        for p in &self.phases {
            if p.tasks.is_empty() {
                return Err(PlanError::EmptyPhase(p.title.clone()));
            }
            for id in p.tasks.iter().filter_map(|t| t.id.as_deref()) {
                if !ids.insert(id) {
                    return Err(PlanError::DuplicateId(id.to_string()));
                }
            }
        }
        let mut edges: Vec<(String, String)> = Vec::new();
        for p in &self.phases {
            for t in &p.tasks {
                for need in &t.needs {
                    if !ids.contains(need.as_str()) {
                        return Err(PlanError::UnknownNeed {
                            task: t.text.clone(),
                            needs: need.clone(),
                        });
                    }
                    if let Some(id) = &t.id {
                        edges.push((id.clone(), need.clone()));
                    }
                }
            }
        }
        match dag::topo_layers(ids.iter().map(|s| (*s).to_string()), &edges) {
            Ok(_) => Ok(()),
            Err(c) => Err(PlanError::Cycle(c.0.join(" → "))),
        }
    }

    /// Junta cada fase con menos de `min_tasks` tareas con la siguiente (ADR-0011: «una fase
    /// con una sola tarea se junta con la siguiente»). La última queda como está si no hay
    /// siguiente: se junta con la anterior.
    pub fn normalized(mut self, min_tasks: usize) -> Plan {
        let mut out: Vec<Phase> = Vec::new();
        let mut carry: Option<Phase> = None;
        for p in self.phases.drain(..) {
            let cur = match carry.take() {
                Some(mut c) => {
                    c.title = format!("{} + {}", c.title, p.title);
                    c.verify = c.verify.or(p.verify);
                    c.tasks.extend(p.tasks);
                    c
                }
                None => p,
            };
            if cur.tasks.len() < min_tasks {
                carry = Some(cur);
            } else {
                out.push(cur);
            }
        }
        if let Some(rest) = carry {
            match out.last_mut() {
                Some(last) => {
                    last.title = format!("{} + {}", last.title, rest.title);
                    last.tasks.extend(rest.tasks);
                }
                None => out.push(rest),
            }
        }
        self.phases = out;
        self
    }

    pub fn progress(&self) -> Progress {
        for (i, p) in self.phases.iter().enumerate() {
            if p.has_failure() {
                return Progress::Failed { index: i };
            }
            if !p.is_done() {
                return Progress::Phase { index: i };
            }
        }
        Progress::Done
    }

    /// (hechas, total) de todo el plan.
    pub fn counts(&self) -> (usize, usize) {
        let all = self.phases.iter().flat_map(|p| p.tasks.iter());
        let total = all.clone().count();
        (all.filter(|t| t.mark == TaskMark::Done).count(), total)
    }
}

/// Sin `Fase N:` al inicio, para que `render` no lo duplique.
fn strip_phase_prefix(title: &str) -> &str {
    let lower = title.to_ascii_lowercase();
    for key in ["fase", "phase"] {
        if let Some(rest) = lower.strip_prefix(key) {
            // «Fase 2: arreglar», «Fase 2 · arreglar», «Fase 2 - arreglar».
            let skipped = title.len() - rest.len();
            let tail = &title[skipped..];
            let tail = tail.trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ');
            return tail.trim_start_matches([':', '·', '-', '—', '.']).trim();
        }
    }
    title
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

fn parse_task(line: &str) -> Option<PlanTask> {
    let (mark, rest) = if let Some(r) = line.strip_prefix("- [ ]") {
        (TaskMark::Pending, r)
    } else if let Some(r) = line
        .strip_prefix("- [x]")
        .or_else(|| line.strip_prefix("- [X]"))
    {
        (TaskMark::Done, r)
    } else {
        let r = line.strip_prefix("- [!]")?;
        (TaskMark::Failed, r)
    };
    let mut rest = rest.trim();
    let mut id = None;
    if let Some(r) = rest.strip_prefix('[')
        && let Some(end) = r.find(']')
        && is_id(&r[..end])
    {
        id = Some(r[..end].to_string());
        rest = r[end + 1..].trim();
    }
    let mut needs = Vec::new();
    let mut text = rest.to_string();
    if let Some(open) = rest.rfind("(needs:")
        && rest.ends_with(')')
    {
        needs = rest[open + "(needs:".len()..rest.len() - 1]
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        text = rest[..open].trim().to_string();
    }
    Some(PlanTask {
        id,
        text,
        mark,
        needs,
        notes: Vec::new(),
    })
}

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Plan: arreglar la rotación de tokens

## Fase 1: preparar
Verifica: cargo test -p auth
- [x] [A1] Leer el módulo de tokens
- [ ] [A2] Escribir el test que falla (needs: A1)
  > falló: el fixture no existe

## Fase 2: arreglar
- [ ] Corregir la rotación
- [ ] Correr toda la suite
";

    #[test]
    fn parses_phases_tasks_ids_needs_and_notes() {
        let p = Plan::parse(SAMPLE);
        assert_eq!(p.title, "arreglar la rotación de tokens");
        assert_eq!(p.phases.len(), 2);
        assert_eq!(p.phases[0].title, "preparar");
        assert_eq!(p.phases[0].verify.as_deref(), Some("cargo test -p auth"));
        let t = &p.phases[0].tasks;
        assert_eq!(
            (t[0].id.as_deref(), t[0].mark),
            (Some("A1"), TaskMark::Done)
        );
        assert_eq!(t[1].needs, ["A1"]);
        assert_eq!(t[1].text, "Escribir el test que falla");
        assert_eq!(t[1].notes, ["falló: el fixture no existe"]);
        assert_eq!(p.phases[1].tasks.len(), 2);
        assert_eq!(p.counts(), (1, 4));
    }

    #[test]
    fn render_then_parse_is_stable() {
        let p = Plan::parse(SAMPLE);
        let again = Plan::parse(&p.render());
        assert_eq!(p, again);
        // Y renderizar lo ya renderizado no cambia nada.
        assert_eq!(again.render(), p.render());
    }

    #[test]
    fn progress_walks_the_phases_and_stops_on_failure() {
        let mut p = Plan::parse(SAMPLE);
        assert_eq!(p.progress(), Progress::Phase { index: 0 });
        p.phases[0].tasks[1].mark = TaskMark::Done;
        assert_eq!(p.progress(), Progress::Phase { index: 1 });
        p.phases[1].tasks[0].mark = TaskMark::Failed;
        assert_eq!(p.progress(), Progress::Failed { index: 1 });
        p.phases[1].tasks[0].mark = TaskMark::Done;
        p.phases[1].tasks[1].mark = TaskMark::Done;
        assert_eq!(p.progress(), Progress::Done);
    }

    #[test]
    fn single_task_phases_merge_with_the_next() {
        let p = Plan::parse(
            "# Plan: x\n## Fase 1: a\n- [ ] uno\n## Fase 2: b\n- [ ] dos\n- [ ] tres\n## Fase 3: c\n- [ ] cuatro\n",
        )
        .normalized(2);
        assert_eq!(p.phases.len(), 1, "la última sola se junta con la anterior");
        assert_eq!(p.phases[0].tasks.len(), 4);

        let p = Plan::parse(
            "# Plan: x\n## Fase 1: a\n- [ ] 1\n- [ ] 2\n## Fase 2: b\n- [ ] 3\n## Fase 3: c\n- [ ] 4\n- [ ] 5\n",
        )
        .normalized(2);
        assert_eq!(p.phases.len(), 2);
        assert_eq!(p.phases[1].tasks.len(), 3);
        assert_eq!(p.phases[1].title, "b + c");
    }

    #[test]
    fn validation_catches_unknown_needs_cycles_and_duplicates() {
        let ok = Plan::parse(SAMPLE);
        assert_eq!(ok.validate(), Ok(()));
        let bad = Plan::parse("# Plan: x\n## Fase 1: a\n- [ ] [A] uno (needs: Z)\n- [ ] dos\n");
        assert!(matches!(bad.validate(), Err(PlanError::UnknownNeed { .. })));
        let cyc = Plan::parse(
            "# Plan: x\n## Fase 1: a\n- [ ] [A] uno (needs: B)\n- [ ] [B] dos (needs: A)\n",
        );
        assert!(matches!(cyc.validate(), Err(PlanError::Cycle(_))));
        let dup = Plan::parse("# Plan: x\n## Fase 1: a\n- [ ] [A] uno\n- [ ] [A] dos\n");
        assert_eq!(dup.validate(), Err(PlanError::DuplicateId("A".into())));
        assert_eq!(Plan::parse("nada").validate(), Err(PlanError::NoPhases));
        let empty = Plan::parse("# Plan: x\n## Fase 1: a\n");
        assert_eq!(empty.validate(), Err(PlanError::EmptyPhase("a".into())));
    }

    #[test]
    fn arbitrary_text_never_panics_and_roundtrips_after_one_render() {
        for text in [
            "",
            "\n\n",
            "## ",
            "- [ ]",
            "- [x] [",
            "## Fase\n- [ ] (needs:",
            "# #\n>>",
        ] {
            let p = Plan::parse(text);
            let r = Plan::parse(&p.render());
            assert_eq!(r.render(), p.render(), "{text:?}");
        }
    }
}
