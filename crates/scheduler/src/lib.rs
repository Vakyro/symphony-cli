//! Scheduler de Symphony (IDEA §5.4, STACK §8). Sin E/S: reglas puras y deterministas.
//!
//! Este paso (P08.S2) trae el clasificador de operaciones; la cola, los slots y la
//! equidad llegan en P08.S3.

mod classify;
pub use classify::{OpClass, classify};
