//! Reads a bash command on stdin and writes its split pipelines to stdout as a JSON
//! array of stage arrays. The splitting itself lives in the library.

use std::io::{Read, Write};

use bash_splitter::split;

fn main() {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("bash-splitter: failed to read stdin: {e}");
        std::process::exit(1);
    }

    let pipelines = match split(&input) {
        Ok(pipelines) => pipelines,
        // The caller decides what an unparseable command means; we just signal it.
        Err(e) => {
            eprintln!("bash-splitter: parse error: {e}");
            std::process::exit(2);
        }
    };

    let json = serde_json::to_string(&pipelines).expect("Stage is always serializable");
    // A consumer closing stdout early is normal, not a failure; exit cleanly on the
    // broken pipe rather than panicking.
    if let Err(e) = writeln!(std::io::stdout(), "{json}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("bash-splitter: failed to write stdout: {e}");
        std::process::exit(1);
    }
}
