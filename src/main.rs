mod cli;
mod runner;

use std::process::ExitCode;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let spec = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Some(spec)) => spec,
        Ok(None) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match runner::run(spec).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("caros-core: {error}");
            ExitCode::FAILURE
        }
    }
}
