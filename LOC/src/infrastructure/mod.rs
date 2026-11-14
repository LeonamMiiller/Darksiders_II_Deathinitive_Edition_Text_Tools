//! Camada de infraestrutura: implementações concretas das portas dos casos
//! de uso — E/S sobre o sistema de arquivos e (com a feature `gui`) o canal
//! de progresso compartilhado com a interface Dioxus.
//!
//! Nenhum tipo desta camada vaza para `domain`/`use_cases`: os casos de uso
//! só enxergam os traits definidos em `use_cases::ports`.

pub mod file_io;

#[cfg(feature = "gui")]
pub mod gui;
