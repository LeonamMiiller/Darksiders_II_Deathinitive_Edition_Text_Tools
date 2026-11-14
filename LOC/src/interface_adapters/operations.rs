//! Orquestração compartilhada pelas interfaces **CLI** e **GUI**.
//!
//! Ponto de composição de ambas as interfaces: abre os arquivos, monta as
//! portas de infraestrutura (E/S) e executa os casos de uso, devolvendo um
//! resumo em texto. Nenhuma regra de negócio mora aqui — todo o domínio
//! continua em `domain`/`use_cases`.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::infrastructure::file_io::{create_sink, open_source};
use crate::use_cases::export::{self, ExportPorts, ExportRequest};
use crate::use_cases::import::{self, ImportPorts, ImportRequest};
use crate::use_cases::ports::ProgressReporter;

/// Resultado de uma operação: resumo em texto ou erro pronto para exibir.
pub type OperationResult = Result<String, Box<dyn Error + Send + Sync>>;

/// Operação a executar — inferida pela extensão do arquivo de entrada ou
/// forçada pelo usuário (`-e`/`-i` na CLI, botão na GUI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Exporta os textos do `.loc` para um TXT.
    Export,
    /// Importa as traduções de um TXT para `<loc>.NEW`.
    Import,
}

/// Infere a operação a partir do arquivo de entrada.
///
/// Regra simplificada em relação ao legado (documentada no README):
/// extensão `.loc` (qualquer caixa) -> `Export`; extensão `.txt` (qualquer
/// caixa) -> `Import` (o `.loc` correspondente é derivado da mesma pasta).
/// O Delphi classificava qualquer extensão diferente de `.loc` como
/// *import*, mas na prática esse caminho era quebrado — ele tentava abrir o
/// próprio TXT como se fosse um `.loc`.
pub fn infer_operation(input: &Path) -> Option<Operation> {
    let extension = input.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("loc") {
        return Some(Operation::Export);
    }
    if extension.eq_ignore_ascii_case("txt") {
        return Some(Operation::Import);
    }
    None
}

/// `<entrada>.txt` — padrão usado pelo legado (`ChangeFileExt`): o TXT é
/// gravado no **mesmo diretório** do `.loc`.
pub fn default_txt_path(loc: &Path) -> PathBuf {
    loc.with_extension("txt")
}

/// `<entrada>.loc` — o `.loc` correspondente a um TXT, no **mesmo
/// diretório** do TXT (stem idêntico, extensão trocada).
pub fn default_loc_path(txt: &Path) -> PathBuf {
    txt.with_extension("loc")
}

/// Caminho absoluto para exibição em mensagens (melhor esforço: se o
/// diretório atual não puder ser obtido, devolve o caminho como está).
fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(current) => current.join(path),
        Err(_) => path.to_path_buf(),
    }
}

/// Mensagem exibida quando o `.loc` esperado não existe.
fn missing_loc_message(loc: &Path) -> String {
    format!(
        "arquivo `{}` não encontrado, verifique!",
        absolute_path(loc).display()
    )
}

/// `<entrada>.NEW` — nome de saída do Import, idêntico ao
/// `FFileName + '.NEW'` do Delphi.
pub fn import_output_path(loc: &Path) -> PathBuf {
    let mut path = loc.as_os_str().to_os_string();
    path.push(".NEW");
    PathBuf::from(path)
}

/// Resolve os caminhos `(loc, txt)` de um **Import** a partir dos
/// posicionais da CLI.
///
/// * `INPUT` com extensão `.txt` (qualquer caixa) -> é o TXT; o `.loc` é
///   `OUTPUT` (se dado) ou o `<stem>.loc` do **mesmo diretório** do TXT.
/// * demais entradas (sintaxe legada) -> `INPUT` é o `.loc`; o TXT é
///   `OUTPUT` (se dado) ou `<INPUT>.txt`.
pub fn resolve_import_paths(input: &Path, output: Option<&Path>) -> (PathBuf, PathBuf) {
    let input_is_txt = input
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));

    if input_is_txt {
        let txt = input.to_path_buf();
        let loc = match output {
            Some(path) => path.to_path_buf(),
            None => default_loc_path(&txt),
        };
        (loc, txt)
    } else {
        let loc = input.to_path_buf();
        let txt = match output {
            Some(path) => path.to_path_buf(),
            None => default_txt_path(&loc),
        };
        (loc, txt)
    }
}

/// **Export**: lê o `.loc` e grava o TXT em `txt_path`.
///
/// Em caso de falha a saída parcial é removida, para nunca deixar um
/// arquivo incompleto que possa ser confundido com um resultado válido.
pub fn export(loc_path: &Path, txt_path: &Path, progress: &mut dyn ProgressReporter) -> OperationResult {
    let result = {
        let mut loc = open_source(loc_path)?;
        let mut txt = create_sink(txt_path)?;
        export::execute(
            &ExportRequest {
                loc_path: loc_path.to_path_buf(),
                txt_path: txt_path.to_path_buf(),
            },
            &mut ExportPorts {
                loc: &mut *loc,
                txt: &mut *txt,
                progress,
            },
        )
    };

    match result {
        Ok(report) => Ok(format!(
            "{} text(s) ({} unique) exported to {} ({} bytes)",
            report.texts,
            report.unique,
            txt_path.display(),
            report.bytes
        )),
        Err(error) => {
            let _ = std::fs::remove_file(txt_path);
            Err(error.into())
        }
    }
}

/// **Import**: lê o `.loc` e o TXT e grava `<loc>.NEW`.
///
/// Se o `.loc` esperado não existir a operação aborta com uma mensagem
/// indicando o caminho completo procurado (o `.loc` pode ter sido derivado
/// do TXT, quando o usuário informou apenas o arquivo de texto).
///
/// Em caso de falha a saída parcial é removida (idem ao Export).
pub fn import(loc_path: &Path, txt_path: &Path, progress: &mut dyn ProgressReporter) -> OperationResult {
    if !loc_path.exists() {
        return Err(missing_loc_message(loc_path).into());
    }
    let output_path = import_output_path(loc_path);
    let result = {
        let mut loc = open_source(loc_path)?;
        let mut txt = open_source(txt_path)?;
        let mut output = create_sink(&output_path)?;
        import::execute(
            &ImportRequest {
                loc_path: loc_path.to_path_buf(),
                txt_path: txt_path.to_path_buf(),
            },
            &mut ImportPorts {
                loc: &mut *loc,
                txt: &mut *txt,
                output: &mut *output,
                progress,
            },
        )
    };

    match result {
        Ok(report) => Ok(format!(
            "{} unique text(s) imported from {} line(s) -> {} ({} bytes)",
            report.unique,
            report.lines,
            output_path.display(),
            report.bytes
        )),
        Err(error) => {
            let _ = std::fs::remove_file(&output_path);
            Err(error.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::ports::ProgressEvent;

    /// Reporter silencioso para os testes de orquestração.
    struct SilentProgress;

    impl ProgressReporter for SilentProgress {
        fn report(&mut self, _event: &ProgressEvent) {}
    }

    #[test]
    fn infers_export_for_loc_and_import_for_txt() {
        assert_eq!(
            infer_operation(Path::new("media/ui_core.loc")),
            Some(Operation::Export)
        );
        assert_eq!(
            infer_operation(Path::new("media/UI_CORE.LOC")),
            Some(Operation::Export)
        );
        assert_eq!(
            infer_operation(Path::new("media/ui_core.txt")),
            Some(Operation::Import)
        );
        assert_eq!(
            infer_operation(Path::new("media/ui_core.TXT")),
            Some(Operation::Import)
        );
        assert_eq!(infer_operation(Path::new("media/ui_core.bin")), None);
        assert_eq!(infer_operation(Path::new("sem_extensao")), None);
    }

    #[test]
    fn default_paths_stay_in_the_same_directory() {
        assert_eq!(
            default_txt_path(Path::new("/jogo/media/ui_core.loc")),
            PathBuf::from("/jogo/media/ui_core.txt")
        );
        assert_eq!(
            default_loc_path(Path::new("/jogo/media/ui_core.txt")),
            PathBuf::from("/jogo/media/ui_core.loc")
        );
    }

    #[test]
    fn resolves_import_paths_from_txt_input() {
        // INPUT é o TXT: o `.loc` é derivado da mesma pasta.
        let (loc, txt) = resolve_import_paths(Path::new("/jogo/media/ui_core.txt"), None);
        assert_eq!(loc, PathBuf::from("/jogo/media/ui_core.loc"));
        assert_eq!(txt, PathBuf::from("/jogo/media/ui_core.txt"));

        // OUTPUT explícito vence a derivação.
        let (loc, txt) = resolve_import_paths(
            Path::new("/jogo/media/ui_core.txt"),
            Some(Path::new("/out/outro.loc")),
        );
        assert_eq!(loc, PathBuf::from("/out/outro.loc"));
        assert_eq!(txt, PathBuf::from("/jogo/media/ui_core.txt"));
    }

    #[test]
    fn resolves_import_paths_from_loc_input() {
        // Sintaxe legada: INPUT é o `.loc`, OUTPUT (se dado) é o TXT.
        let (loc, txt) = resolve_import_paths(Path::new("/jogo/media/ui_core.loc"), None);
        assert_eq!(loc, PathBuf::from("/jogo/media/ui_core.loc"));
        assert_eq!(txt, PathBuf::from("/jogo/media/ui_core.txt"));

        let (loc, txt) = resolve_import_paths(
            Path::new("/jogo/media/ui_core.loc"),
            Some(Path::new("/out/traducoes.txt")),
        );
        assert_eq!(loc, PathBuf::from("/jogo/media/ui_core.loc"));
        assert_eq!(txt, PathBuf::from("/out/traducoes.txt"));
    }

    #[test]
    fn import_aborts_with_full_path_when_loc_is_missing() -> Result<(), Box<dyn std::error::Error>> {
        let missing = std::env::temp_dir().join("loc_tool_test_ausente.loc");
        let txt = std::env::temp_dir().join("loc_tool_test_ausente.txt");
        // Garante que nenhum resíduo de teste anterior interfira.
        let _ = std::fs::remove_file(&missing);

        let error = match import(&missing, &txt, &mut SilentProgress) {
            Ok(_) => return Err("esperava que o import abortasse sem o .loc".into()),
            Err(error) => error,
        };

        let message = error.to_string();
        assert!(
            message.contains("não encontrado, verifique!"),
            "mensagem inesperada: {message}"
        );
        assert!(
            message.contains(&missing.display().to_string()),
            "o caminho completo do .loc devia constar na mensagem: {message}"
        );
        // Nada foi criado.
        assert!(!missing.exists());
        assert!(!import_output_path(&missing).exists());
        Ok(())
    }
}
