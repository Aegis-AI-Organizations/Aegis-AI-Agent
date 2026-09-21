use aegis_ai_agent::redaction::Redactor;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Sql,
}

#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    mode: Mode,
}

fn main() -> anyhow::Result<()> {
    let args = parse_args(std::env::args().skip(1))?;
    let redactor = Redactor::new();
    redact_file(&args.input, args.output.as_ref(), args.mode, &redactor)
}

fn parse_args<I>(mut args: I) -> anyhow::Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut input = None;
    let mut output = None;
    let mut mode = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = args.next().map(PathBuf::from),
            "--output" => output = args.next().map(PathBuf::from),
            "--mode" => {
                mode = match args.next().as_deref() {
                    Some("sql") => Some(Mode::Sql),
                    Some(value) => anyhow::bail!("unsupported --mode {value:?}; expected sql"),
                    None => anyhow::bail!("--mode requires a value"),
                };
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            value => anyhow::bail!("unknown argument {value:?}"),
        }
    }

    Ok(Args {
        input: input.ok_or_else(|| anyhow::anyhow!("--input is required"))?,
        output,
        mode: mode.ok_or_else(|| anyhow::anyhow!("--mode is required"))?,
    })
}

fn print_usage() {
    eprintln!("Usage: aegis-redact --input dump.sql --mode sql [--output sanitized.sql]");
}

fn redact_file(
    input: &PathBuf,
    output: Option<&PathBuf>,
    mode: Mode,
    redactor: &Redactor,
) -> anyhow::Result<()> {
    match mode {
        Mode::Sql => {
            let reader = BufReader::new(File::open(input)?);
            eprintln!(
                "[aegis-redact] sanitizing SQL seed file: {}",
                input.display()
            );
            if let Some(output) = output {
                let mut writer = BufWriter::new(File::create(output)?);
                redact_sql_stream(reader, &mut writer, redactor)?;
                writer.flush()?;
                eprintln!(
                    "[aegis-redact] SQL seed file sanitized: {}",
                    output.display()
                );
            } else {
                let stdout = io::stdout();
                let mut writer = BufWriter::new(stdout.lock());
                redact_sql_stream(reader, &mut writer, redactor)?;
                writer.flush()?;
                eprintln!("[aegis-redact] SQL seed file sanitized");
            }
        }
    }
    Ok(())
}

fn redact_sql_stream<R, W>(reader: R, writer: &mut W, redactor: &Redactor) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    for line in reader.lines() {
        let line = line?;
        writeln!(writer, "{}", redactor.redact_sql_line(&line))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sql_args() {
        let args = parse_args(
            [
                "--input", "dump.sql", "--mode", "sql", "--output", "out.sql",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap();

        assert_eq!(args.input, PathBuf::from("dump.sql"));
        assert_eq!(args.output, Some(PathBuf::from("out.sql")));
        assert_eq!(args.mode, Mode::Sql);
    }

    #[test]
    fn streams_sql_without_buffering_full_file() {
        let redactor = Redactor::new();
        let input = "INSERT INTO users VALUES ('john@example.com');\nSELECT 1;\n";
        let mut output = Vec::new();

        redact_sql_stream(BufReader::new(input.as_bytes()), &mut output, &redactor).unwrap();
        let rendered = String::from_utf8(output).unwrap();

        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("SELECT 1;"));
    }
}
