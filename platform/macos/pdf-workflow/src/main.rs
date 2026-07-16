use std::{env, process::ExitCode};

use mdviewer_pdf_workflow::{default_job_root, dispatch_with_launch_services, run_invocation};

fn main() -> ExitCode {
    let Some(home) = env::var_os("HOME") else {
        eprintln!("the PDF Workflow environment is invalid");
        return ExitCode::FAILURE;
    };
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    match run_invocation(
        &arguments,
        &default_job_root(home.as_ref()),
        dispatch_with_launch_services,
    ) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
