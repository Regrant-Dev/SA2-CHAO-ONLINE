//! Rebuilds the link machine's model data (`assets/link_machine/`) from what
//! tools/link_machine.py exports:
//!
//!     blender -b --python tools/link_machine.py -- <dir>
//!     cargo run --release --target x86_64-unknown-linux-gnu --bin machineconv -- <dir> assets/link_machine
//!
//! Reads `<dir>/<variant>.obj` and `<dir>/<variant>.tga` for each garden's variant and writes
//! `<out>/<variant>.lmc` (src/creature_link/machine_model.rs).

#[path = "../creature_link/machine_model.rs"]
mod machine_model;

use std::path::Path;
use std::process::ExitCode;

const VARIANTS: [&str; 3] = ["neutral", "hero", "dark"];

fn run(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for v in VARIANTS {
        let obj = from.join(format!("{v}.obj"));
        let tga = from.join(format!("{v}.tga"));
        let obj = std::fs::read_to_string(&obj).map_err(|e| format!("{}: {e}", obj.display()))?;
        let tga = std::fs::read(&tga).map_err(|e| format!("{}: {e}", tga.display()))?;
        let bytes = machine_model::convert(&obj, &tga).map_err(|e| format!("{v}: {e}"))?;
        let m = machine_model::Machine::parse(&bytes).map_err(|e| format!("{v}: {e}"))?;
        let out = to.join(format!("{v}.lmc"));
        std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
        let verts: Vec<u32> = m.pieces.iter().map(|p| p.vlist[1] >> 16).collect();
        println!(
            "{}: {} bytes, vertices per piece {verts:?}",
            out.display(),
            bytes.len()
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [from, to] = args.as_slice() else {
        eprintln!("usage: machineconv <exported dir> <out dir>");
        return ExitCode::FAILURE;
    };
    match run(Path::new(from), Path::new(to)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("machineconv: {e}");
            ExitCode::FAILURE
        }
    }
}
