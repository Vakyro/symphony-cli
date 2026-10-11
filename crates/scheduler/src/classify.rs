//! Clase de costo de una operación (IDEA §5.4) a partir del nombre de la herramienta y,
//! para el shell, del comando. Es una heurística sobre texto: se equivoca hacia lo
//! conservador (un comando desconocido es «medio», nunca «gratis»).

use std::fmt;

/// Clase 0–4 de `tool_calls.op_class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpClass {
    /// Espera de LLM o de red: no consume máquina.
    Network = 0,
    /// Leer archivos, grep, `git diff`, editar.
    Cheap = 1,
    /// Lint, format, typecheck pequeño.
    Medium = 2,
    /// Tests unitarios, typecheck grande.
    Heavy = 3,
    /// Build, tests de integración, navegador, instalaciones.
    VeryHeavy = 4,
}

impl OpClass {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// ¿Ocupa un slot pesado del scheduler? (clases 3 y 4)
    pub fn is_heavy(self) -> bool {
        self >= OpClass::Heavy
    }
}

impl fmt::Display for OpClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_i64())
    }
}

/// Clasifica una operación. `command` solo importa cuando la herramienta es un shell.
pub fn classify(tool_name: &str, command: Option<&str>) -> OpClass {
    let tool = tool_name.to_ascii_lowercase();
    if tool.starts_with("mcp") || tool.contains("web") || tool == "task" {
        return OpClass::Network;
    }
    if is_shell(&tool) {
        return command.map_or(OpClass::Medium, classify_command);
    }
    // Read, Glob, Grep, Edit, Write, apply_patch, TodoWrite…
    OpClass::Cheap
}

fn is_shell(tool: &str) -> bool {
    matches!(
        tool,
        "bash"
            | "shell"
            | "exec_command"
            | "local_shell"
            | "powershell"
            | "run_command"
            | "command"
    ) || tool.contains("bash")
        || tool.contains("shell")
}

/// El mayor costo de los segmentos de una línea (`a && b | c; d`).
fn classify_command(line: &str) -> OpClass {
    split_segments(line)
        .into_iter()
        .map(classify_segment)
        .max()
        .unwrap_or(OpClass::Medium)
}

/// Parte por `&&`, `||`, `;`, `|` y saltos de línea. Es textual: no entiende comillas
/// (un `;` dentro de un string parte de más, y eso solo puede subir la clase).
fn split_segments(line: &str) -> Vec<&str> {
    line.split(['\n', ';', '|', '&'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn classify_segment(seg: &str) -> OpClass {
    let words: Vec<&str> = seg
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']))
        .skip_while(|w| is_prefix_word(w))
        .collect();
    let Some(&first) = words.first() else {
        return OpClass::Cheap;
    };
    let prog = program_name(first);
    let args = &words[1..];
    match prog.as_str() {
        // Solo lectura y navegación.
        "rg" | "grep" | "egrep" | "fgrep" | "ag" | "ack" | "fd" | "find" | "ls" | "dir" | "cat"
        | "type" | "head" | "tail" | "wc" | "sed" | "awk" | "cut" | "sort" | "uniq" | "diff"
        | "tree" | "pwd" | "echo" | "printf" | "cd" | "which" | "where" | "stat" | "file"
        | "mkdir" | "touch" | "cp" | "mv" | "rm" | "test" | "true" | "false" | "jq" | "less"
        | "more" | "get-content" | "get-childitem" | "select-string" | "set-location" => {
            OpClass::Cheap
        }
        "curl" | "wget" | "ssh" | "scp" | "ping" | "invoke-webrequest" => OpClass::Network,
        "git" => git(args),
        "npm" | "pnpm" | "yarn" | "bun" | "npx" | "pnpx" | "bunx" => node(prog.as_str(), args),
        "cargo" => cargo(args),
        "pytest" | "jest" | "vitest" | "mocha" | "rspec" | "phpunit" | "ctest" | "tox" => {
            OpClass::Heavy
        }
        "playwright" | "cypress" | "docker" | "podman" | "make" | "cmake" | "ninja" | "gradle"
        | "gradlew" | "mvn" | "bazel" | "webpack" | "vite" | "next" | "rustc" | "gcc" | "g++"
        | "clang" | "msbuild" | "xcodebuild" => very_heavy_or_heavy(prog.as_str(), args),
        "go" => match args.first().copied() {
            Some("test") => OpClass::Heavy,
            Some("build" | "install" | "get" | "mod") => OpClass::VeryHeavy,
            _ => OpClass::Medium,
        },
        "dotnet" => match args.first().copied() {
            Some("test") => OpClass::Heavy,
            Some("build" | "publish" | "restore") => OpClass::VeryHeavy,
            _ => OpClass::Medium,
        },
        "pip" | "pip3" | "uv" | "poetry" | "conda" | "apt" | "apt-get" | "brew" | "choco"
        | "winget" | "gem" | "bundle" | "composer" => {
            if args
                .iter()
                .any(|a| matches!(*a, "install" | "add" | "sync" | "update" | "upgrade" | "i"))
            {
                OpClass::VeryHeavy
            } else {
                OpClass::Medium
            }
        }
        "tsc" => {
            if args.contains(&"--noEmit") {
                OpClass::Heavy
            } else {
                OpClass::VeryHeavy
            }
        }
        "eslint" | "prettier" | "ruff" | "black" | "isort" | "flake8" | "rustfmt" | "biome"
        | "gofmt" | "clippy-driver" | "oxlint" | "stylelint" | "shellcheck" | "mypy"
        | "pyright" | "pylint" | "tidy" => OpClass::Medium,
        "python" | "python3" | "py" | "node" | "deno" | "ruby" | "perl" => {
            // `python -m pytest` / `node --test`.
            if args.windows(2).any(|w| w == ["-m", "pytest"]) || args.contains(&"--test") {
                OpClass::Heavy
            } else {
                OpClass::Medium
            }
        }
        _ => OpClass::Medium,
    }
}

/// `sudo`, `time`, `env`, `FOO=bar`, `cmd /c`… no son el programa.
fn is_prefix_word(w: &str) -> bool {
    matches!(
        w,
        "sudo" | "time" | "env" | "nice" | "command" | "exec" | "nohup" | "call"
    ) || w.eq_ignore_ascii_case("cmd")
        || w.eq_ignore_ascii_case("/c")
        || w.eq_ignore_ascii_case("-c")
        || w.eq_ignore_ascii_case("bash")
        || w.eq_ignore_ascii_case("sh")
        || w.eq_ignore_ascii_case("pwsh")
        || w.eq_ignore_ascii_case("powershell")
        || w.eq_ignore_ascii_case("-command")
        || w.eq_ignore_ascii_case("-noprofile")
        || (w.contains('=') && !w.starts_with('-') && !w.contains('/'))
}

/// Nombre del programa sin ruta ni extensión, en minúsculas (`C:\x\Cargo.EXE` → `cargo`).
fn program_name(word: &str) -> String {
    let word = word.trim_matches(['"', '\'']);
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let base = base.to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

fn git(args: &[&str]) -> OpClass {
    match args.iter().find(|a| !a.starts_with('-')).copied() {
        Some("fetch" | "pull" | "push" | "clone" | "ls-remote") => OpClass::Network,
        Some("gc" | "repack" | "filter-branch") => OpClass::Heavy,
        _ => OpClass::Cheap,
    }
}

fn cargo(args: &[&str]) -> OpClass {
    // `cargo +nightly test`: el toolchain no es el subcomando.
    match args
        .iter()
        .find(|a| !a.starts_with('+') && !a.starts_with('-'))
        .copied()
    {
        Some(
            "build" | "b" | "install" | "bench" | "doc" | "publish" | "package" | "dist" | "xtask"
            | "fetch" | "update" | "vendor" | "llvm-cov" | "deny" | "audit",
        ) => OpClass::VeryHeavy,
        Some("test" | "t" | "nextest") => OpClass::Heavy,
        Some("check" | "c" | "clippy" | "fmt" | "metadata" | "tree") => OpClass::Medium,
        Some("run" | "r") => OpClass::VeryHeavy,
        _ => OpClass::Medium,
    }
}

/// `npm|pnpm|yarn|bun` y los ejecutores (`npx`): por subcomando y por nombre del script.
fn node(prog: &str, args: &[&str]) -> OpClass {
    let positional: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let Some(&sub) = positional.first() else {
        // `yarn` a secas instala.
        return if prog == "yarn" {
            OpClass::VeryHeavy
        } else {
            OpClass::Medium
        };
    };
    match sub {
        "install" | "i" | "ci" | "add" | "remove" | "rm" | "update" | "up" | "upgrade" | "dlx"
        | "create" | "init" | "rebuild" | "link" | "audit" => OpClass::VeryHeavy,
        "build" => OpClass::VeryHeavy,
        "test" | "t" | "tst" => OpClass::Heavy,
        "run" | "run-script" | "exec" | "x" => script(positional.get(1).copied().unwrap_or("")),
        // `npx vitest`, `pnpm playwright test`, `yarn lint`…
        other => script(other),
    }
}

/// Costo por el nombre de un script de `package.json` o de un ejecutor.
fn script(name: &str) -> OpClass {
    let n = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["e2e", "playwright", "cypress", "build"]) {
        OpClass::VeryHeavy
    } else if has(&["test", "vitest", "jest", "spec", "tsc", "typecheck"]) {
        OpClass::Heavy
    } else {
        // Lint, format, check… y cualquier script desconocido.
        OpClass::Medium
    }
}

/// Herramientas de build general: sin subcomando conocido son muy pesadas; con `test`, pesadas.
fn very_heavy_or_heavy(prog: &str, args: &[&str]) -> OpClass {
    if args
        .iter()
        .any(|a| matches!(*a, "test" | "--version" | "--help" | "-v"))
    {
        return if args.contains(&"test") {
            OpClass::Heavy
        } else {
            OpClass::Cheap
        };
    }
    if matches!(prog, "docker" | "podman")
        && args
            .first()
            .is_some_and(|a| matches!(*a, "ps" | "images" | "logs" | "inspect" | "version"))
    {
        return OpClass::Cheap;
    }
    OpClass::VeryHeavy
}

#[cfg(test)]
mod tests {
    use super::*;
    use OpClass::*;

    fn sh(cmd: &str) -> OpClass {
        classify("Bash", Some(cmd))
    }

    /// La tabla de casos del PLAN (P08.S2) y los comandos que Signalia mostró.
    #[test]
    fn command_table() {
        let cases: &[(&str, OpClass)] = &[
            ("npm test", Heavy),
            ("npm run test:unit", Heavy),
            ("pnpm test --filter api", Heavy),
            ("pnpm build", VeryHeavy),
            ("npm run build", VeryHeavy),
            ("pnpm install", VeryHeavy),
            ("npm ci", VeryHeavy),
            ("yarn", VeryHeavy),
            ("pnpm run e2e", VeryHeavy),
            ("npx playwright test", VeryHeavy),
            ("npx vitest run", Heavy),
            ("pnpm lint", Medium),
            ("npm run typecheck", Heavy),
            ("npx eslint src", Medium),
            ("rg foo src", Cheap),
            ("grep -rn TODO .", Cheap),
            ("git diff --stat", Cheap),
            ("git status", Cheap),
            ("git commit -m x", Cheap),
            ("git push origin main", Network),
            ("curl https://example.com", Network),
            ("cat Cargo.toml", Cheap),
            ("cargo build --release", VeryHeavy),
            ("cargo test --workspace", Heavy),
            ("cargo nextest run -p symphony-store", Heavy),
            ("cargo +nightly fmt --check", Medium),
            ("cargo clippy -- -D warnings", Medium),
            ("pytest -q", Heavy),
            ("python -m pytest tests", Heavy),
            ("pip install -r requirements.txt", VeryHeavy),
            ("go test ./...", Heavy),
            ("docker build .", VeryHeavy),
            ("docker ps", Cheap),
            ("tsc --noEmit", Heavy),
            ("tsc -p .", VeryHeavy),
            ("some-unknown-tool --flag", Medium),
        ];
        for (cmd, want) in cases {
            assert_eq!(sh(cmd), *want, "{cmd}");
        }
    }

    #[test]
    fn compound_commands_cost_as_their_heaviest_part() {
        assert_eq!(sh("cd web && pnpm install && pnpm build"), VeryHeavy);
        assert_eq!(sh("git diff | rg TODO"), Cheap);
        assert_eq!(sh("npm run lint; npm test"), Heavy);
        assert_eq!(sh("FOO=1 NODE_ENV=test npm test"), Heavy);
        assert_eq!(sh("sudo apt-get install -y gcc"), VeryHeavy);
    }

    #[test]
    fn paths_extensions_and_wrappers_are_ignored() {
        assert_eq!(sh(r"C:\Users\x\.cargo\bin\cargo.exe build"), VeryHeavy);
        assert_eq!(sh("/usr/bin/rg foo"), Cheap);
        assert_eq!(sh("pwsh -NoProfile -Command npm test"), Heavy);
        assert_eq!(sh("cmd /c pnpm build"), VeryHeavy);
        assert_eq!(sh("bash -c \"npm test\""), Heavy);
    }

    #[test]
    fn tools_that_are_not_a_shell() {
        assert_eq!(classify("Read", None), Cheap);
        assert_eq!(classify("Edit", Some("npm install")), Cheap);
        assert_eq!(classify("apply_patch", None), Cheap);
        assert_eq!(classify("WebFetch", None), Network);
        assert_eq!(classify("mcp__github__search", None), Network);
        // Shell sin comando conocido: el costo conservador.
        assert_eq!(classify("Bash", None), Medium);
        assert_eq!(classify("exec_command", Some("pnpm build")), VeryHeavy);
    }

    #[test]
    fn heavy_means_classes_three_and_four() {
        assert!(!Medium.is_heavy());
        assert!(Heavy.is_heavy() && VeryHeavy.is_heavy());
        assert_eq!(VeryHeavy.as_i64(), 4);
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Nunca entra en pánico y siempre cae en 0–4, con cualquier texto.
            #[test]
            fn any_text_gets_a_class_in_range(tool in ".{0,12}", cmd in ".{0,80}") {
                let c = classify(&tool, Some(&cmd)).as_i64();
                prop_assert!((0..=4).contains(&c));
            }

            /// Añadir un segmento nunca baja el costo de la línea.
            #[test]
            fn appending_a_segment_never_lowers_the_class(a in "[a-z ]{1,20}", b in "[a-z ]{1,20}") {
                let alone = classify_command(&a);
                let both = classify_command(&format!("{a} && {b}"));
                prop_assert!(both >= alone);
            }
        }
    }
}
