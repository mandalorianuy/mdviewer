use std::{env, io};

fn main() {
    let code = mdconvert_cli::run(env::args_os().skip(1), &mut io::stdout(), &mut io::stderr());
    std::process::exit(i32::from(code));
}
