//! Caso de uso **Import** — regrava os textos de um TXT em um `.loc`.
//!
//! Fidelidade ao `TLoc.Import` do Delphi: o bloco da língua é remontado
//! (cabeçalho + tabelas preservados, textos reescritos, bloco Z copiado no
//! fim) e as duas tabelas + `TextBlockSize` + `LangBlockSize` são
//! atualizados com *seek* explícito.
//!
//! Desvio documentado do legado: `ZBlockOffset` **é** gravado com o valor
//! novo (o Delphi calculava o campo mas nunca o escrevia, deixando o
//! arquivo apontando para o bloco Z antigo).
//!
//! O resultado é gravado na porta `output` (a CLI/GUI decide o caminho —
//! por padrão `<entrada>.NEW`, como no legado).

use std::path::PathBuf;

use super::error::UseCaseError;
use super::ports::{BinarySink, BinarySource, ProgressEvent, ProgressReporter};
use super::read_source;
use crate::domain::loc::LocFile;

/// Entrada do caso de uso **Import**.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    /// Caminho do `.loc` a ser regravado (apenas para as mensagens).
    pub loc_path: PathBuf,
    /// Caminho do TXT com as traduções.
    pub txt_path: PathBuf,
}

/// Portas utilizadas pelo **Import** (injetadas pelo adaptador).
pub struct ImportPorts<'a> {
    /// O `.loc` original (fonte das tabelas e do bloco Z).
    pub loc: &'a mut dyn BinarySource,
    /// O TXT com as traduções.
    pub txt: &'a mut dyn BinarySource,
    /// Destino do `.loc` regravado.
    pub output: &'a mut dyn BinarySink,
    /// Relatório de progresso (console ou canal da GUI).
    pub progress: &'a mut dyn ProgressReporter,
}

/// Resumo do **Import** (também usado pela GUI para exibir o resultado).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Linhas lidas do TXT.
    pub lines: usize,
    /// Textos únicos regravados.
    pub unique: usize,
    /// Linhas do TXT ignoradas (sobrando).
    pub extra: usize,
    /// Tamanho do `.loc` gerado.
    pub bytes: usize,
    /// `TextBlockSize` novo.
    pub text_block_size: u32,
    /// `LangBlockSize` novo.
    pub lang_block_size: u32,
}

/// Executa o Import: lê o `.loc` e o TXT, remonta o bloco da língua e
/// grava o arquivo regravado pela porta `output`.
///
/// Linhas que sobrarem no TXT são ignoradas (como no legado); TXT com
/// menos linhas que textos únicos é erro.
pub fn execute(
    request: &ImportRequest,
    ports: &mut ImportPorts<'_>,
) -> Result<ImportReport, UseCaseError> {
    ports.progress.report(&ProgressEvent::Info(format!(
        "Importing texts from {} into {} ...",
        request.txt_path.display(),
        request.loc_path.display()
    )));

    let loc = LocFile::parse(&mut *ports.loc)?;
    let txt = read_source(&mut *ports.txt, "reading the TXT file")?;
    let outcome = loc.import_txt(&txt)?;
    let stats = outcome.stats;

    ports.progress.report(&ProgressEvent::Texts {
        total: loc.text_references(),
        unique: stats.unique,
    });

    let bytes = outcome.data.len();
    ports
        .output
        .write_all(&outcome.data)
        .map_err(|source| UseCaseError::adapter("writing the new LOC file", source.into()))?;

    ports.progress.report(&ProgressEvent::Done {
        summary: format!(
            "{} unique text(s) imported from {} line(s) ({} extra ignored); \
             text block {} -> {} byte(s), language block {} byte(s), file {bytes} byte(s)",
            stats.unique, stats.lines, stats.extra, stats.old_text_block_size,
            stats.new_text_block_size, stats.new_lang_block_size
        ),
    });
    Ok(ImportReport {
        lines: stats.lines,
        unique: stats.unique,
        extra: stats.extra,
        bytes,
        text_block_size: stats.new_text_block_size,
        lang_block_size: stats.new_lang_block_size,
    })
}
