//! `cargo xtask er`: diagrama ER en Mermaid generado desde `migrations/*.sql` (P13.S6).
//!
//! Las migraciones son la fuente de verdad del esquema (DB como explicación);
//! este módulo aplica su DDL en orden (`CREATE TABLE`, `DROP TABLE`,
//! `ALTER TABLE ... ADD COLUMN/RENAME TO`) sobre un estado acumulado y renderiza
//! el estado final. Parseo a mano, sin dependencias.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[derive(Debug, Default)]
struct Column {
    name: String,
    ty: String,
    pk: bool,
    not_null: bool,
    fk: Option<String>,
}

#[derive(Debug, Default)]
struct Table {
    name: String,
    columns: Vec<Column>,
}

#[derive(Debug, Default)]
struct Schema {
    tables: Vec<Table>,
}

impl Schema {
    fn create(&mut self, table: Table) {
        match self.tables.iter_mut().find(|t| t.name == table.name) {
            // Recreación en el mismo lugar (patrón `_bak` de las migraciones).
            Some(slot) => *slot = table,
            None => self.tables.push(table),
        }
    }

    fn drop(&mut self, name: &str) {
        self.tables.retain(|t| t.name != name);
    }

    fn rename(&mut self, old: &str, new: &str) {
        if let Some(t) = self.tables.iter_mut().find(|t| t.name == old) {
            t.name = new.to_string();
        }
    }

    fn add_column(&mut self, table: &str, col: Column) {
        let Some(t) = self.tables.iter_mut().find(|t| t.name == table) else {
            return;
        };
        if !t.columns.iter().any(|c| c.name == col.name) {
            t.columns.push(col);
        }
    }
}

/// Punto de entrada del subcomando: lee `migrations_dir` en orden y escribe `out`.
pub fn run(migrations_dir: &Path, out: &Path) -> Result<(), String> {
    let mut files: Vec<_> = fs::read_dir(migrations_dir)
        .map_err(|e| format!("no se pudo leer {}: {e}", migrations_dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();

    let mut schema = Schema::default();
    for path in &files {
        let sql = fs::read_to_string(path)
            .map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
        apply_sql(&mut schema, &sql);
    }

    let doc = render(&schema);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("no se pudo crear {}: {e}", parent.display()))?;
    }
    fs::write(out, doc).map_err(|e| format!("no se pudo escribir {}: {e}", out.display()))?;
    eprintln!("xtask: er → {}", out.display());
    Ok(())
}

/// Aplica el DDL de un archivo de migración al estado acumulado.
fn apply_sql(schema: &mut Schema, sql: &str) {
    for stmt in split_statements(&strip_comments(sql)) {
        let tokens = tokenize(&stmt);
        match tokens.first().map(|t| t.to_ascii_uppercase()).as_deref() {
            Some("CREATE") => apply_create(schema, &tokens),
            Some("DROP") => {
                // DROP TABLE [IF EXISTS] <nombre>;
                if tokens
                    .get(1)
                    .is_some_and(|t| t.eq_ignore_ascii_case("TABLE"))
                {
                    let idx = if tokens.get(2).is_some_and(|t| t.eq_ignore_ascii_case("IF")) {
                        4
                    } else {
                        2
                    };
                    if let Some(name) = tokens.get(idx) {
                        schema.drop(&unquote(name));
                    }
                }
            }
            Some("ALTER") => apply_alter(schema, &tokens),
            _ => {}
        }
    }
}

/// `CREATE TABLE <nombre> (...)` (ignora `VIRTUAL` y `AS SELECT`).
fn apply_create(schema: &mut Schema, tokens: &[String]) {
    if !tokens
        .get(1)
        .is_some_and(|t| t.eq_ignore_ascii_case("TABLE"))
        || tokens
            .get(2)
            .is_some_and(|t| t.eq_ignore_ascii_case("VIRTUAL"))
    {
        return;
    }
    let Some(open) = tokens.iter().position(|t| t == "(") else {
        return; // p. ej. `CREATE TABLE x AS SELECT ...`
    };
    let Some(name) = tokens.get(open - 1) else {
        return;
    };
    // El token previo al nombre debe ser TABLE o EXISTS (`IF NOT EXISTS`).
    let prev_ok = tokens
        .get(open.wrapping_sub(2))
        .is_some_and(|t| t.eq_ignore_ascii_case("TABLE") || t.eq_ignore_ascii_case("EXISTS"));
    if !prev_ok {
        return;
    }
    let Some(close) = matching_close(tokens, open) else {
        return;
    };
    let mut table = Table {
        name: unquote(name),
        ..Table::default()
    };
    for part in split_top_level_commas(&tokens[open + 1..close]) {
        if let Some(col) = parse_column(&part) {
            table.columns.push(col);
        }
    }
    schema.create(table);
}

/// `ALTER TABLE <nombre> ADD [COLUMN] <def> | RENAME TO <nuevo>`.
fn apply_alter(schema: &mut Schema, tokens: &[String]) {
    let Some(table) = tokens.get(2) else { return };
    let table = unquote(table);
    if let Some(pos) = tokens.iter().position(|t| t.eq_ignore_ascii_case("RENAME")) {
        if let Some(new) = tokens.get(pos + 2) {
            schema.rename(&table, &unquote(new));
        }
        return;
    }
    if let Some(pos) = tokens.iter().position(|t| t.eq_ignore_ascii_case("ADD")) {
        let start = pos
            + 1
            + usize::from(
                tokens
                    .get(pos + 1)
                    .is_some_and(|t| t.eq_ignore_ascii_case("COLUMN")),
            );
        if let Some(col) = parse_column(&tokens[start..]) {
            schema.add_column(&table, col);
        }
    }
}

/// Parsea la definición de una columna; `None` si es una restricción de tabla.
fn parse_column(tokens: &[String]) -> Option<Column> {
    let first = tokens.first()?.to_ascii_uppercase();
    if matches!(
        first.as_str(),
        "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN" | "CONSTRAINT"
    ) {
        return None;
    }
    let mut i = 1;
    let mut ty = tokens.get(i)?.clone();
    i += 1;
    if tokens.get(i).is_some_and(|t| t == "(") {
        // Tipo con parámetros, p. ej. VARCHAR(255).
        let mut depth = 0usize;
        let mut ty_toks = vec![ty];
        while i < tokens.len() {
            depth += usize::from(tokens[i] == "(");
            depth = depth.saturating_sub(usize::from(tokens[i] == ")"));
            ty_toks.push(tokens[i].clone());
            i += 1;
            if depth == 0 {
                break;
            }
        }
        ty = ty_toks.join("");
    }
    let mut col = Column {
        name: unquote(&tokens[0]),
        ty,
        ..Column::default()
    };
    while i < tokens.len() {
        let upper = tokens[i].to_ascii_uppercase();
        match upper.as_str() {
            "PRIMARY" => col.pk = true,
            "NOT"
                if tokens
                    .get(i + 1)
                    .is_some_and(|t| t.eq_ignore_ascii_case("NULL")) =>
            {
                col.not_null = true;
            }
            "REFERENCES" => {
                if let Some(target) = tokens.get(i + 1) {
                    col.fk = Some(unquote(target));
                    i += 1;
                    // Columna referenciada opcional: REFERENCES t(col).
                    if tokens.get(i + 1).is_some_and(|t| t == "(") {
                        i += 2;
                        while i < tokens.len() && tokens[i] != ")" {
                            i += 1;
                        }
                    }
                }
            }
            "DEFAULT" => i += 1, // salta el valor por defecto
            "CHECK" => {
                // Salta el parentesisado del CHECK.
                while i < tokens.len() && tokens[i] != "(" {
                    i += 1;
                }
                if i < tokens.len() {
                    let mut depth = 0usize;
                    while i < tokens.len() {
                        depth += usize::from(tokens[i] == "(");
                        depth = depth.saturating_sub(usize::from(tokens[i] == ")"));
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    i = i.saturating_sub(1); // el `i += 1` del loop compensa
                }
            }
            _ => {}
        }
        i += 1;
    }
    Some(col)
}

fn render(schema: &Schema) -> String {
    let mut out = String::new();
    out.push_str("# Diagrama ER\n\n");
    out.push_str(
        "Generado por `cargo xtask er` desde `migrations/*.sql`. No editar a mano:\n\
         la fuente de verdad del esquema son las migraciones.\n\n```mermaid\nerDiagram\n",
    );
    for table in &schema.tables {
        let _ = writeln!(out, "    {} {{", table.name);
        for col in &table.columns {
            let mut keys = String::new();
            if col.pk {
                keys.push_str("PK");
            }
            if col.fk.is_some() {
                if !keys.is_empty() {
                    keys.push_str(", ");
                }
                keys.push_str("FK");
            }
            if keys.is_empty() {
                let _ = writeln!(out, "        {} {}", col.ty, col.name);
            } else {
                let _ = writeln!(out, "        {} {} {}", col.ty, col.name, keys);
            }
        }
        let _ = writeln!(out, "    }}");
    }
    for table in &schema.tables {
        for col in &table.columns {
            if let Some(target) = &col.fk {
                let child = if col.not_null { "||" } else { "}o" };
                let _ = writeln!(
                    out,
                    "    {} {}--|| {} : \"{}\"",
                    table.name, child, target, col.name
                );
            }
        }
    }
    out.push_str("```\n");
    out
}

/// Quita comentarios de línea (`--`), respetando literales de cadena.
fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut in_string = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_string = !in_string;
                out.push(c);
            }
            '-' if !in_string && chars.peek() == Some(&'-') => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Divide en sentencias por `;` fuera de paréntesis y cadenas.
fn split_statements(sql: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_string = false;
    for c in sql.chars() {
        match c {
            '\'' => {
                in_string = !in_string;
                cur.push(c);
            }
            '(' if !in_string => {
                depth += 1;
                cur.push(c);
            }
            ')' if !in_string => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ';' if !in_string && depth == 0 => {
                let trimmed = cur.trim();
                if !trimmed.is_empty() {
                    stmts.push(trimmed.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        stmts.push(trimmed.to_string());
    }
    stmts
}

/// Tokeniza: separa por espacios y saca `(`, `)`, `,` como tokens propios.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                cur.push(c);
                for c2 in chars.by_ref() {
                    cur.push(c2);
                    if c2 == '\'' {
                        break;
                    }
                }
            }
            '(' | ')' | ',' => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
                tokens.push(c.to_string());
            }
            c if c.is_whitespace() => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        tokens.push(cur.trim().to_string());
    }
    tokens
}

/// Índice del `)` que cierra el `(` en `pos`.
fn matching_close(tokens: &[String], pos: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate().skip(pos) {
        depth += usize::from(t == "(");
        depth = depth.saturating_sub(usize::from(t == ")"));
        if depth == 0 {
            return Some(i);
        }
    }
    None
}

/// Divide tokens por comas de nivel superior (dentro de una definición de tabla).
fn split_top_level_commas(tokens: &[String]) -> Vec<Vec<String>> {
    let mut parts = Vec::new();
    let mut cur = Vec::new();
    let mut depth = 0usize;
    for t in tokens {
        depth += usize::from(t == "(");
        depth = depth.saturating_sub(usize::from(t == ")"));
        if t == "," && depth == 0 {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(t.clone());
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Quita comillas delimitadoras de un identificador.
fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '[' || c == ']')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQL: &str = "
        CREATE TABLE authors (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL
        ) STRICT;
        CREATE TABLE books (
            id        TEXT PRIMARY KEY,
            author_id TEXT NOT NULL REFERENCES authors(id),
            title     TEXT,
            genre     TEXT CHECK (genre IN ('FIC','NONFIC'))
        );
        CREATE VIRTUAL TABLE books_fts USING fts5(title);
    ";

    #[test]
    fn render_dos_tablas_con_fk() {
        let mut schema = Schema::default();
        apply_sql(&mut schema, SQL);
        let out = render(&schema);

        assert!(out.contains("erDiagram"));
        assert!(out.contains("authors {"));
        assert!(out.contains("books {"));
        // VIRTUAL TABLE queda fuera.
        assert!(!out.contains("books_fts"));
        // Columnas con tipo, marca PK y FK.
        assert!(out.contains("TEXT id PK"));
        assert!(out.contains("TEXT author_id FK"));
        assert!(out.contains("TEXT title"));
        // Relación por la FK, NOT NULL => ||--||.
        assert!(out.contains("books ||--|| authors : \"author_id\""));
    }

    #[test]
    fn drop_y_recreate_dejan_estado_final() {
        let mut schema = Schema::default();
        apply_sql(
            &mut schema,
            "CREATE TABLE things (id TEXT PRIMARY KEY, old_col TEXT);
             CREATE TABLE things_bak AS SELECT * FROM things;
             DROP TABLE things;
             CREATE TABLE things (id TEXT PRIMARY KEY, new_col INTEGER);",
        );
        let out = render(&schema);
        assert!(out.contains("INTEGER new_col"));
        assert!(!out.contains("old_col"));
        assert!(!out.contains("things_bak"));
    }

    #[test]
    fn alter_add_column_y_rename() {
        let mut schema = Schema::default();
        apply_sql(
            &mut schema,
            "CREATE TABLE profile_models (profile_id TEXT NOT NULL REFERENCES profiles(id));
             ALTER TABLE profile_models ADD COLUMN outcome_score_sum REAL NOT NULL DEFAULT 0;
             CREATE TABLE old_name (id TEXT);
             ALTER TABLE old_name RENAME TO new_name;",
        );
        let out = render(&schema);
        assert!(out.contains("REAL outcome_score_sum"));
        assert!(out.contains("profile_models ||--|| profiles"));
        assert!(out.contains("new_name {"));
        assert!(!out.contains("old_name {"));
    }
}
