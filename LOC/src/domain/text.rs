//! Regras de texto do TXT de tradução — portes fieis do legado Delphi:
//!
//! * [`break_line`] espelha `Delutto.Utils.BreakLine`: converte quebras de
//!   linha reais em escapes `<\n>`/`<\r>` (e vice-versa, quando o texto já
//!   contém escapes);
//! * [`split_lines`] espelha `TStrings.SetTextStr` (o que
//!   `TStringList.LoadFromFile` usa): quebra em CRLF/LF/CR e **não** gera
//!   uma linha vazia extra por quebra final;
//! * [`join_lines`] espelha `TStringList.SaveToFile(TEncoding.UTF8)`:
//!   BOM UTF-8 + linhas unidas por CRLF, **sem** CRLF final.
//!
//! As operações são feitas sobre bytes crus: UTF-8 nunca contém `0x0A`/
//! `0x0D` em bytes de sequência, então o resultado é idêntico ao do Delphi
//! (que trabalharia em UTF-16) para qualquer texto válido.

/// Escape de quebra de linha usado pelo jogo.
const ESC_LF: &[u8] = b"<\\n>";
const ESC_CR: &[u8] = b"<\\r>";

/// BOM UTF-8 (`EF BB BF`).
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// CRLF usado na gravação do TXT.
const CRLF: &[u8] = b"\r\n";

/// Porte de `Delutto.Utils.BreakLine`.
///
/// Se o texto já contém algum escape (`<\n>`/`<\r>`), os escapes são
/// **decodificados** em quebras reais; caso contrário, as quebras reais são
/// **escapadas**. A ordem das substituições é a mesma do Delphi.
pub fn break_line(raw: &[u8]) -> Vec<u8> {
    if contains(raw, ESC_LF) || contains(raw, ESC_CR) {
        let decoded = replace_all(raw, ESC_LF, b"\n");
        replace_all(&decoded, ESC_CR, b"\r")
    } else {
        let escaped = replace_all(raw, b"\n", ESC_LF);
        replace_all(&escaped, b"\r", ESC_CR)
    }
}

/// Porte de `TStrings.SetTextStr`: divide o conteúdo do TXT em linhas.
///
/// * um BOM UTF-8 inicial é ignorado (o `LoadFromFile` do Delphi faz idem);
/// * CRLF conta como uma única quebra;
/// * uma quebra final **não** cria uma linha vazia extra.
pub fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let body = strip_bom(data);
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < body.len() {
        match body[index] {
            b'\r' => {
                lines.push(&body[start..index]);
                index += 1;
                if index < body.len() && body[index] == b'\n' {
                    index += 1;
                }
                start = index;
            }
            b'\n' => {
                lines.push(&body[start..index]);
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    // Cauda não terminada por quebra de linha (ou arquivo vazio: start == len).
    if start < body.len() {
        lines.push(&body[start..]);
    }
    lines
}

/// Porte de `TStringList.SaveToFile(TEncoding.UTF8)`: BOM + linhas unidas
/// por CRLF, sem CRLF após a última linha.
pub fn join_lines(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(UTF8_BOM.len());
    out.extend_from_slice(&UTF8_BOM);
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.extend_from_slice(CRLF);
        }
        out.extend_from_slice(line);
    }
    out
}

/// Remove um BOM UTF-8 inicial (se presente).
fn strip_bom(data: &[u8]) -> &[u8] {
    if data.starts_with(&UTF8_BOM) {
        &data[UTF8_BOM.len()..]
    } else {
        data
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Substituição de todas as ocorrências (como `StringReplace(..., [rfReplaceAll])`).
fn replace_all(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0usize;
    while index < haystack.len() {
        let remaining = &haystack[index..];
        if remaining.starts_with(from) {
            out.extend_from_slice(to);
            index += from.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texto sem escapes -> quebras reais viram escapes (caminho de saída).
    #[test]
    fn break_line_escapes_real_breaks() {
        assert_eq!(break_line(b"line1\nline2\r\nline3"), b"line1<\\n>line2<\\r><\\n>line3");
    }

    /// Texto já escapado -> escapes voltam a ser quebras reais (entrada).
    #[test]
    fn break_line_decodes_existing_escapes() {
        assert_eq!(break_line(b"line1<\\n>line2<\\r>line3"), b"line1\nline2\rline3");
    }

    /// Texto sem nenhuma quebra nem escape permanece intacto.
    #[test]
    fn break_line_keeps_plain_text() {
        assert_eq!(break_line(b"plain text"), b"plain text");
        assert_eq!(break_line(b""), b"");
    }

    /// Multibyte UTF-8 não é alterado (nenhum byte de sequência é 0x0A/0x0D).
    #[test]
    fn break_line_is_utf8_safe() {
        assert_eq!(break_line("olá\nmundo".as_bytes()), "olá<\\n>mundo".as_bytes());
    }

    /// CRLF/LF/CR isolados e cauda sem quebra.
    #[test]
    fn split_lines_handles_all_break_styles() {
        assert_eq!(split_lines(b"a\r\nb\nc\rd"), vec![&b"a"[..], b"b", b"c", b"d"]);
        assert_eq!(split_lines(b"sem quebra"), vec![&b"sem quebra"[..]]);
        assert_eq!(split_lines(b""), Vec::<&[u8]>::new());
    }

    /// Quebra final não cria linha vazia extra; linhas vazias internas contam.
    #[test]
    fn split_lines_ignores_single_trailing_break() {
        assert_eq!(split_lines(b"a\r\n"), vec![&b"a"[..]]);
        assert_eq!(split_lines(b"a\n"), vec![&b"a"[..]]);
        assert_eq!(split_lines(b"a\n\n"), vec![&b"a"[..], b""]);
        assert_eq!(split_lines(b"\r\n"), vec![&b""[..]]);
    }

    /// BOM inicial é ignorado (como no `LoadFromFile` do Delphi).
    #[test]
    fn split_lines_strips_bom() {
        let mut data = Vec::from(UTF8_BOM);
        data.extend_from_slice(b"linha");
        assert_eq!(split_lines(&data), vec![&b"linha"[..]]);
    }

    /// Gravação: BOM + CRLF entre linhas, sem CRLF final.
    #[test]
    fn join_lines_writes_bom_without_trailing_break() {
        let lines = vec![b"alpha".to_vec(), b"beta".to_vec(), Vec::new()];
        let mut expected = Vec::from(UTF8_BOM);
        expected.extend_from_slice(b"alpha\r\nbeta\r\n");
        assert_eq!(join_lines(&lines), expected);
    }

    /// Round-trip: exportar (`break_line` + `join_lines`) e reimportar
    /// (`split_lines` + `break_line`) devolve o texto original.
    #[test]
    fn export_import_round_trip() {
        let original = vec![
            b"um".to_vec(),
            b"dois\ncom escape".to_vec(),
            "três".as_bytes().to_vec(),
        ];
        let encoded: Vec<Vec<u8>> = original.iter().map(|line| break_line(line)).collect();
        let file = join_lines(&encoded);
        let decoded: Vec<Vec<u8>> = split_lines(&file).into_iter().map(break_line).collect();
        assert_eq!(decoded, original);
    }
}
