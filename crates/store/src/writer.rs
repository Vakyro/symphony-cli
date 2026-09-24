//! Escritor único (STACK §9.2, DB §1, §7): un hilo dueño de la única conexión
//! de escritura, alimentado por un canal acotado. Los eventos se escriben en
//! lotes: todo lo que haya en cola, hasta `BATCH_MAX`, en una transacción.
//! Sin espera artificial: con carga los lotes se llenan solos y sin carga la
//! latencia es mínima.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use rusqlite::{Connection, OpenFlags, Transaction, params};
use tokio::sync::{mpsc, oneshot};

use crate::StoreError;

/// Máximo de eventos por transacción (DB §7: "cada 50 eventos").
pub const BATCH_MAX: usize = 50;
/// Capacidad del canal: con la cola llena, los productores esperan (backpressure).
const CHANNEL_CAPACITY: usize = 4096;

/// Un evento para `events` (DB §3.F).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub project_id: String,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
    pub event_type: String,
    pub source: String,
    pub payload_json: Option<String>,
    pub occurred_at: i64,
}

/// Escritura arbitraria dentro de una transacción (la usan los repositorios).
pub type WriteFn = Box<dyn FnOnce(&Transaction<'_>) -> Result<(), rusqlite::Error> + Send>;

enum Command {
    Event(NewEvent),
    Write(WriteFn, oneshot::Sender<Result<(), StoreError>>),
    Flush(oneshot::Sender<WriterStats>),
    Shutdown,
}

/// Contadores acumulados del escritor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriterStats {
    pub events_written: u64,
    pub events_failed: u64,
    pub batches: u64,
}

/// Lado de los productores. Se puede clonar libremente.
#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<Command>,
}

/// Dueño del hilo escritor. Al soltarlo (o con `shutdown`) se escribe lo ya encolado
/// y el hilo termina; los handles que queden reciben `WriterClosed`.
pub struct Writer {
    handle: WriterHandle,
    thread: Option<JoinHandle<()>>,
    db_path: PathBuf,
}

impl Writer {
    /// Abre la base (migrando si hace falta) y arranca el hilo escritor.
    pub fn start(db_path: &Path) -> Result<Self, StoreError> {
        let conn = crate::open(db_path)?;
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("symphony-store-writer".into())
            .spawn(move || run(conn, rx))?;
        Ok(Self {
            handle: WriterHandle { tx },
            thread: Some(thread),
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn handle(&self) -> WriterHandle {
        self.handle.clone()
    }

    /// Conexión de solo lectura a la misma base. Las lecturas no bloquean al escritor (WAL).
    pub fn reader(&self) -> Result<Connection, StoreError> {
        open_reader(&self.db_path)
    }

    /// Espera a que se escriba todo lo encolado y termina el hilo.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        // `blocking_send` entra en pánico dentro de un runtime de tokio: se envía desde un hilo aparte.
        let tx = self.handle.tx.clone();
        let sender = std::thread::spawn(move || tx.blocking_send(Command::Shutdown).is_ok());
        let _ = sender.join();
        let _ = thread.join();
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, thiserror::Error)]
#[error("el escritor de la base de datos ya no está corriendo")]
pub struct WriterClosed;

impl From<WriterClosed> for StoreError {
    fn from(_: WriterClosed) -> Self {
        StoreError::WriterClosed
    }
}

impl WriterHandle {
    /// Encola un evento. Espera solo si la cola está llena.
    pub async fn event(&self, ev: NewEvent) -> Result<(), WriterClosed> {
        self.tx
            .send(Command::Event(ev))
            .await
            .map_err(|_| WriterClosed)
    }

    /// Ejecuta `f` en una transacción propia y devuelve su resultado.
    pub async fn write(&self, f: WriteFn) -> Result<(), StoreError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Write(f, reply))
            .await
            .map_err(|_| WriterClosed)?;
        rx.await.map_err(|_| WriterClosed)?
    }

    /// Espera a que todo lo encolado antes esté escrito.
    pub async fn flush(&self) -> Result<WriterStats, WriterClosed> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Flush(reply))
            .await
            .map_err(|_| WriterClosed)?;
        rx.await.map_err(|_| WriterClosed)
    }
}

/// Abre una conexión de solo lectura (`query_only`).
pub fn open_reader(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch(
        "PRAGMA query_only = ON; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;",
    )?;
    Ok(conn)
}

fn insert_event(tx: &Transaction<'_>, ev: &NewEvent) -> Result<(), rusqlite::Error> {
    tx.prepare_cached(
        "INSERT INTO events (project_id, agent_id, run_id, type, source, payload_json, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        ev.project_id,
        ev.agent_id,
        ev.run_id,
        ev.event_type,
        ev.source,
        ev.payload_json,
        ev.occurred_at
    ])?;
    Ok(())
}

fn write_batch(conn: &mut Connection, batch: &[NewEvent], stats: &mut WriterStats) {
    if batch.is_empty() {
        return;
    }
    let all = conn.transaction().and_then(|tx| {
        for ev in batch {
            insert_event(&tx, ev)?;
        }
        tx.commit()
    });
    stats.batches += 1;
    if all.is_ok() {
        stats.events_written += batch.len() as u64;
        return;
    }
    // Un evento inválido no puede tirar a los demás: se reintenta uno por uno.
    for ev in batch {
        let one = conn.transaction().and_then(|tx| {
            insert_event(&tx, ev)?;
            tx.commit()
        });
        match one {
            Ok(()) => stats.events_written += 1,
            Err(_) => stats.events_failed += 1,
        }
    }
}

fn run(mut conn: Connection, mut rx: mpsc::Receiver<Command>) {
    let mut stats = WriterStats::default();
    let mut batch: Vec<NewEvent> = Vec::with_capacity(BATCH_MAX);
    while let Some(first) = rx.blocking_recv() {
        let mut next = Some(first);
        // Juntar lo que ya esté en cola, respetando el orden de llegada.
        while let Some(cmd) = next.take() {
            match cmd {
                Command::Event(ev) => {
                    batch.push(ev);
                    if batch.len() >= BATCH_MAX {
                        write_batch(&mut conn, &batch, &mut stats);
                        batch.clear();
                    }
                }
                Command::Write(f, reply) => {
                    write_batch(&mut conn, &batch, &mut stats);
                    batch.clear();
                    let result = conn
                        .transaction()
                        .and_then(|tx| {
                            f(&tx)?;
                            tx.commit()
                        })
                        .map_err(StoreError::from);
                    let _ = reply.send(result);
                }
                Command::Flush(reply) => {
                    write_batch(&mut conn, &batch, &mut stats);
                    batch.clear();
                    let _ = reply.send(stats);
                }
                Command::Shutdown => {
                    write_batch(&mut conn, &batch, &mut stats);
                    let _ = conn.execute_batch("PRAGMA optimize;");
                    return;
                }
            }
            next = rx.try_recv().ok();
        }
        write_batch(&mut conn, &batch, &mut stats);
        batch.clear();
    }
    let _ = conn.execute_batch("PRAGMA optimize;");
}
