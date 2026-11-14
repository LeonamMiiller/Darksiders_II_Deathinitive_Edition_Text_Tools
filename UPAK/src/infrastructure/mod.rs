//! Camada de infraestrutura: implementações concretas das portas dos casos
//! de uso — E/S sobre o sistema de arquivos, códec zlib e (com a feature
//! `gui`) o buffer de progresso compartilhado com a interface Dioxus.
//!
//! Nenhum tipo desta camada vaza para `domain`/`use_cases`: os casos de uso
//! só enxergam os traits definidos em `use_cases::ports`.

pub mod compression;
pub mod file_io;

#[cfg(feature = "gui")]
pub mod gui;
