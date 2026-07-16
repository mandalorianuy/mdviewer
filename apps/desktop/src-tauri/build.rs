use std::{env, fs, path::PathBuf};

use sha2::{Digest, Sha256};

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let destination = output.join("mdviewer-pdf-workflow");
    let bytes = match env::var_os("MDVIEWER_PDF_WORKFLOW_ARTIFACT") {
        Some(source) => {
            println!(
                "cargo:rerun-if-changed={}",
                PathBuf::from(&source).display()
            );
            fs::read(source).expect("MDVIEWER_PDF_WORKFLOW_ARTIFACT must be readable")
        }
        None => Vec::new(),
    };
    fs::write(destination, &bytes).expect("embedded workflow staging must succeed");
    fs::write(
        output.join("workflow_metadata.rs"),
        format!(
            "const EMBEDDED_WORKFLOW_SHA256: &str = \"{:x}\";\n",
            Sha256::digest(&bytes)
        ),
    )
    .expect("embedded workflow metadata must be generated");
    println!("cargo:rerun-if-env-changed=MDVIEWER_PDF_WORKFLOW_ARTIFACT");
    tauri_build::build();
}
