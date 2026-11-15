//! `quince-soil export`: Skykeep's library to a soil records batch.
//!
//! `--data` is the folder holding `library.db` — Skykeep's app data folder: on Linux
//! `~/.local/share/<bundle id>`, on macOS `~/Library/Application Support/<bundle id>`, on
//! Windows the local app data folder. `--cache` is the `photos/` folder inside it.
//!
//! ```sh
//! quince-soil export --data "$SKYKEEP_DATA" --cache "$SKYKEEP_DATA/photos" \
//!   --book icloud -o /tmp/icloud.json
//! curl -sS -X POST http://127.0.0.1:8791/api/records/batch \
//!   -H "Authorization: Bearer $SOIL_RECORDS_TOKEN" \
//!   --data-binary @/tmp/icloud.json
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use quince_soil::Options;

const USAGE: &str = "\
usage:
  quince-soil export --data <DIR> [--cache <DIR>] [--book <BOOK>] [-o FILE]

  --data   the folder holding library.db (Skykeep's data folder)
  --cache  Skykeep's photos folder; without it records carry no content ids
  --book   the records book to write into (default: icloud)
  -o       write the batch here instead of stdout

Output is one JSON document `{\"records\": [...], \"content\": [...]}`: the `records`
array is exactly `soil-records-ingest`'s `POST /api/records/batch` body.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quince-soil: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("export") => export(&args[1..]),
        Some(other) => Err(format!("unknown command {other}\n\n{USAGE}")),
    }
}

fn export(args: &[String]) -> Result<(), String> {
    let mut data = None;
    let mut cache = None;
    let mut out = None;
    let mut options = Options::default();

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let value = |index: usize| -> Result<&str, String> {
            args.get(index + 1)
                .map(String::as_str)
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg {
            "--data" => {
                data = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--cache" => {
                cache = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--book" => {
                options.book = value(index)?.to_string();
                index += 2;
            }
            "-o" | "--out" => {
                out = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }

    options.cache = cache;
    let data = data.ok_or_else(|| format!("--data is required\n\n{USAGE}"))?;
    let batch = quince_soil::export(&data, &options).map_err(|error| format!("{error:#}"))?;
    let text = serde_json::to_string(&batch).map_err(|error| format!("encode batch: {error}"))?;
    match out {
        Some(path) => {
            std::fs::write(&path, &text)
                .map_err(|error| format!("write {}: {error}", path.display()))?;
        }
        None => println!("{text}"),
    }
    eprintln!(
        "{} records, {} blobs",
        batch.records.len(),
        batch.content.len()
    );
    Ok(())
}
