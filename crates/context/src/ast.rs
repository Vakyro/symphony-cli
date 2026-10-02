//! AstReducer (IDEA §5.6, P09.S4): un archivo de código a distintos niveles de detalle, con
//! tree-sitter y sin LLM.
//!
//! | Nivel | Qué entra |
//! |---|---|
//! | L0 | Nada |
//! | L1 | El nombre del archivo |
//! | L2 | Símbolos (tipo, nombre, línea) |
//! | L3 | Firmas y tipos, sin cuerpos |
//! | L4 | Firmas, y completas las funciones relevantes para una búsqueda |
//! | L5 | El archivo completo |
//!
//! L2–L4 necesitan gramática (Rust, Python, JavaScript, TypeScript/TSX); sin ella devuelven
//! `None` y el llamador usa chunks. Es reversible: el archivo sigue en el worktree.

use std::fmt::Write as _;

use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
}

impl Lang {
    pub fn for_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    fn grammar(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    fn comment(self) -> &'static str {
        if self == Self::Python { "#" } else { "//" }
    }
}

/// Nivel de detalle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    /// L0
    Nothing,
    /// L1
    Name,
    /// L2
    Symbols,
    /// L3
    Signatures,
    /// L4: completas las funciones cuyo nombre o cuerpo contiene alguno de estos términos.
    Relevant(Vec<String>),
    /// L5
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub kind: &'static str,
    pub name: String,
    /// 1-based.
    pub line: usize,
}

/// Nodos que envuelven una declaración sin ser una (`export …`, decoradores de Python).
const WRAPPERS: &[&str] = &["export_statement", "decorated_definition"];

/// (tipo de nodo, etiqueta, ¿contiene declaraciones?, ¿es función?)
const DECLS: &[(&str, &str, bool, bool)] = &[
    // Rust
    ("function_item", "fn", false, true),
    ("function_signature_item", "fn", false, true),
    ("struct_item", "struct", false, false),
    ("enum_item", "enum", false, false),
    ("union_item", "union", false, false),
    ("trait_item", "trait", true, false),
    ("impl_item", "impl", true, false),
    ("mod_item", "mod", true, false),
    ("type_item", "type", false, false),
    ("const_item", "const", false, false),
    ("static_item", "static", false, false),
    ("macro_definition", "macro", false, false),
    // Python
    ("function_definition", "def", false, true),
    ("class_definition", "class", true, false),
    // JavaScript / TypeScript
    ("function_declaration", "function", false, true),
    ("generator_function_declaration", "function", false, true),
    ("class_declaration", "class", true, false),
    ("abstract_class_declaration", "class", true, false),
    ("method_definition", "method", false, true),
    ("method_signature", "method", false, true),
    ("abstract_method_signature", "method", false, true),
    ("interface_declaration", "interface", false, false),
    ("type_alias_declaration", "type", false, false),
    ("enum_declaration", "enum", false, false),
    ("internal_module", "namespace", true, false),
    ("lexical_declaration", "const", false, true),
];

/// Tipos con cuerpo corto se muestran enteros (son «firmas y tipos»).
const SHORT_TYPE_LINES: usize = 12;
const MAX_HEADER: usize = 300;

fn decl(kind: &str) -> Option<(&'static str, bool, bool)> {
    DECLS.iter().find(|d| d.0 == kind).map(|d| (d.1, d.2, d.3))
}

fn text(src: &str, from: usize, to: usize) -> &str {
    src.get(from..to).unwrap_or("")
}

fn name_of(node: Node<'_>, src: &str) -> String {
    let field = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"));
    if let Some(n) = field {
        return text(src, n.start_byte(), n.end_byte()).to_string();
    }
    // `const f = () => …`: el nombre del primer declarador.
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|c| c.kind() == "variable_declarator")
        .and_then(|d| d.child_by_field_name("name"))
        .map(|n| text(src, n.start_byte(), n.end_byte()).to_string())
        .unwrap_or_default()
}

/// Dónde empieza el cuerpo de una declaración (para cortar la firma ahí).
fn body_of(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(b) = node.child_by_field_name("body") {
        return Some(b);
    }
    if node.kind() == "lexical_declaration" {
        // `const f = (a) => { … }`: el cuerpo de la función flecha.
        let mut cursor = node.walk();
        let declarator = node
            .named_children(&mut cursor)
            .find(|c| c.kind() == "variable_declarator")?;
        let value = declarator.child_by_field_name("value")?;
        return value.child_by_field_name("body").or(Some(value));
    }
    None
}

fn clip_header(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_HEADER {
        one_line
    } else {
        let head: String = one_line.chars().take(MAX_HEADER).collect();
        format!("{head}…")
    }
}

struct Walk<'a> {
    src: &'a str,
    lang: Lang,
    terms: Vec<String>,
    symbols: Vec<Symbol>,
    out: String,
}

impl Walk<'_> {
    fn relevant(&self, name: &str, body: &str) -> bool {
        let (name, body) = (name.to_lowercase(), body.to_lowercase());
        self.terms
            .iter()
            .any(|t| name.contains(t.as_str()) || body.contains(t.as_str()))
    }

    fn visit(&mut self, parent: Node<'_>, depth: usize) {
        let mut cursor = parent.walk();
        let children: Vec<Node<'_>> = parent.named_children(&mut cursor).collect();
        for child in children {
            if WRAPPERS.contains(&child.kind()) {
                self.visit(child, depth);
                continue;
            }
            let Some((label, container, function)) = decl(child.kind()) else {
                continue;
            };
            // `const x = 1` dentro de una función no es una declaración del archivo.
            if child.kind() == "lexical_declaration" && depth > 0 {
                continue;
            }
            self.emit(child, depth, label, container, function);
        }
    }

    fn emit(
        &mut self,
        node: Node<'_>,
        depth: usize,
        label: &'static str,
        container: bool,
        function: bool,
    ) {
        let src = self.src;
        let start = match node.parent() {
            Some(p) if WRAPPERS.contains(&p.kind()) => p.start_byte(),
            _ => node.start_byte(),
        };
        let name = name_of(node, src);
        self.symbols.push(Symbol {
            kind: label,
            name: name.clone(),
            line: node.start_position().row + 1,
        });
        let pad = "    ".repeat(depth);
        let full = text(src, start, node.end_byte());
        let body = body_of(node);
        let python = self.lang == Lang::Python;

        if function && self.relevant(&name, full) {
            let _ = writeln!(self.out, "{pad}{}", full.trim_end());
            return;
        }
        match body {
            Some(b) if container => {
                let header = clip_header(text(src, start, b.start_byte()));
                if python {
                    let _ = writeln!(self.out, "{pad}{header}");
                } else {
                    let _ = writeln!(self.out, "{pad}{header} {{");
                }
                self.visit(b, depth + 1);
                if !python {
                    let _ = writeln!(self.out, "{pad}}}");
                }
            }
            Some(b) if function => {
                let header = clip_header(text(src, start, b.start_byte()));
                let tail = if python { " …" } else { " { … }" };
                let _ = writeln!(self.out, "{pad}{header}{tail}");
            }
            _ => {
                // Tipos y constantes: enteros si son cortos.
                if full.lines().count() <= SHORT_TYPE_LINES {
                    for line in full.trim_end().lines() {
                        let _ = writeln!(self.out, "{pad}{line}");
                    }
                } else {
                    let first = full.lines().next().unwrap_or("");
                    let _ = writeln!(self.out, "{pad}{} …", first.trim_end());
                }
            }
        }
    }
}

fn walk<'a>(path: &str, src: &'a str, terms: Vec<String>) -> Option<(Lang, Walk<'a>)> {
    let lang = Lang::for_path(path)?;
    let mut parser = Parser::new();
    parser.set_language(&lang.grammar()).ok()?;
    let tree = parser.parse(src, None)?;
    let mut w = Walk {
        src,
        lang,
        terms: terms
            .into_iter()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect(),
        symbols: Vec::new(),
        out: String::new(),
    };
    w.visit(tree.root_node(), 0);
    Some((lang, w))
}

/// Los símbolos de primer nivel y los de dentro de clases, impls, traits y módulos.
pub fn symbols(path: &str, src: &str) -> Option<Vec<Symbol>> {
    walk(path, src, Vec::new()).map(|(_, w)| w.symbols)
}

/// El archivo al nivel pedido. `None` si el nivel necesita gramática y no hay.
pub fn reduce(path: &str, src: &str, level: &Level) -> Option<String> {
    let terms = match level {
        Level::Nothing => return Some(String::new()),
        Level::Name => return Some(path.to_string()),
        Level::Full => return Some(src.to_string()),
        Level::Relevant(t) => t.clone(),
        Level::Symbols | Level::Signatures => Vec::new(),
    };
    let (lang, w) = walk(path, src, terms)?;
    let lines = src.lines().count();
    let c = lang.comment();
    if *level == Level::Symbols {
        let mut out = format!("{c} {path} · símbolos (L2) · {lines} líneas\n");
        for s in &w.symbols {
            let _ = writeln!(out, "{} {} :{}", s.kind, s.name, s.line);
        }
        return Some(out);
    }
    let what = if matches!(level, Level::Relevant(_)) {
        "firmas y funciones relevantes (L4)"
    } else {
        "firmas y tipos (L3)"
    };
    Some(format!(
        "{c} {path} · {what} · {lines} líneas en el original\n{}",
        w.out
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = r#"use std::fmt;

/// Un token.
pub struct Token {
    pub value: String,
    expires: u64,
}

pub enum Kind { Access, Refresh }

pub trait Store {
    fn get(&self, id: &str) -> Option<Token>;
}

impl Token {
    pub fn new(value: &str) -> Self {
        let expires = 3600;
        Self { value: value.into(), expires }
    }

    pub fn rotate(&mut self) -> Result<(), fmt::Error> {
        self.value = format!("{}-r", self.value);
        Ok(())
    }
}

pub fn refresh(t: &mut Token) -> bool {
    t.rotate().is_ok()
}
"#;

    const PY: &str = r#"import os

class Session:
    """Una sesión."""

    def __init__(self, token: str) -> None:
        self.token = token

    @property
    def valid(self) -> bool:
        return bool(self.token)

def refresh(session: Session) -> Session:
    return Session(os.urandom(8).hex())
"#;

    const TS: &str = r#"import { db } from './db';

export interface User { id: string; email: string }

export type Role = 'admin' | 'user';

export class UserStore {
  private users: User[] = [];
  add(u: User): void {
    this.users.push(u);
  }
  find(id: string): User | undefined {
    return this.users.find((u) => u.id === id);
  }
}

export function validateEmail(e: string): boolean {
  return e.includes('@');
}

export const hashPassword = (p: string): string => {
  return 'hash:' + p;
};
"#;

    #[test]
    fn levels_zero_one_and_five_need_no_grammar() {
        assert_eq!(reduce("a.txt", "x", &Level::Nothing).unwrap(), "");
        assert_eq!(reduce("a.txt", "x", &Level::Name).unwrap(), "a.txt");
        assert_eq!(reduce("a.txt", "x", &Level::Full).unwrap(), "x");
        assert!(reduce("a.txt", "x", &Level::Signatures).is_none());
        assert!(symbols("Makefile", "all:").is_none());
    }

    #[test]
    fn rust_skeleton() {
        insta::assert_snapshot!(reduce("src/token.rs", RUST, &Level::Signatures).unwrap());
        let names: Vec<String> = symbols("src/token.rs", RUST)
            .unwrap()
            .into_iter()
            .map(|s| format!("{} {}", s.kind, s.name))
            .collect();
        assert_eq!(
            names,
            [
                "struct Token",
                "enum Kind",
                "trait Store",
                "fn get",
                "impl Token",
                "fn new",
                "fn rotate",
                "fn refresh"
            ]
        );
    }

    #[test]
    fn python_skeleton() {
        insta::assert_snapshot!(reduce("auth/session.py", PY, &Level::Signatures).unwrap());
    }

    #[test]
    fn typescript_skeleton_and_symbols() {
        insta::assert_snapshot!(reduce("src/users.ts", TS, &Level::Signatures).unwrap());
        insta::assert_snapshot!(reduce("src/users.ts", TS, &Level::Symbols).unwrap());
    }

    #[test]
    fn relevant_level_keeps_matching_functions_whole() {
        let l4 = reduce(
            "src/token.rs",
            RUST,
            &Level::Relevant(vec!["Rotate".into()]),
        )
        .unwrap();
        assert!(
            l4.contains(r#"self.value = format!("{}-r", self.value);"#),
            "{l4}"
        );
        assert!(
            l4.contains("t.rotate().is_ok()"),
            "el cuerpo de refresh menciona rotate"
        );
        assert!(
            !l4.contains("let expires = 3600;"),
            "new no es relevante: {l4}"
        );
        let l3 = reduce("src/token.rs", RUST, &Level::Signatures).unwrap();
        assert!(!l3.contains("self.value = format!"));
    }

    #[test]
    fn javascript_and_broken_code_do_not_fail() {
        let js = "export function a(x) { return x }\nfunction b( {\n";
        let out = reduce("x.js", js, &Level::Signatures).unwrap();
        assert!(out.contains("export function a(x) { … }"), "{out}");
        let tsx = "export const App = () => <div>hola</div>;\n";
        assert!(
            reduce("App.tsx", tsx, &Level::Symbols)
                .unwrap()
                .contains("const App")
        );
    }

    #[test]
    fn skeletons_are_much_shorter_than_big_files() {
        let mut big = String::new();
        for i in 0..200 {
            let _ = write!(
                big,
                "pub fn f{i}(a: u32) -> u32 {{\n    let x = a * {i};\n    let y = x + 1;\n    y * 2\n}}\n\n"
            );
        }
        let l3 = reduce("big.rs", &big, &Level::Signatures).unwrap();
        assert!(l3.len() * 2 < big.len(), "{} vs {}", l3.len(), big.len());
        assert_eq!(symbols("big.rs", &big).unwrap().len(), 200);
    }
}
