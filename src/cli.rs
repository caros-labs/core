use caros_core::supervisor::{AppSpec, RestartPolicy};
use std::{ffi::OsString, time::Duration};

pub(crate) const USAGE: &str = "Usage: caros-core [--max-restarts N] [--restart-delay-ms N] [--stop-timeout-ms N] -- EXECUTABLE [ARG ...]\n\nRuns one app locally. Restarts are disabled by default. SIGINT/SIGTERM stop the app and its process group.";

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<AppSpec>, String> {
    let mut args = args.into_iter();
    let mut restarts = 0;
    let mut delay = 500;
    let mut timeout = 3000;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(None);
        }
        if arg == "--" {
            let executable = args.next().ok_or("missing executable after --")?;
            let mut spec = AppSpec::new("app", executable);
            spec.args = args.collect();
            spec.stop_timeout = Duration::from_millis(timeout);
            if restarts > 0 {
                spec.restart = RestartPolicy::OnFailure {
                    max_restarts: restarts,
                    delay: Duration::from_millis(delay),
                };
            }
            return Ok(Some(spec));
        }
        let name = arg.to_str().ok_or("option is not valid UTF-8")?;
        if !matches!(
            name,
            "--max-restarts" | "--restart-delay-ms" | "--stop-timeout-ms"
        ) {
            return Err(format!(
                "unknown option {name}; put -- before the executable"
            ));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {name}"))?;
        let value: u32 = value
            .to_str()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} requires an unsigned 32-bit integer"))?;
        match name {
            "--max-restarts" => restarts = value,
            "--restart-delay-ms" => delay = u64::from(value),
            "--stop-timeout-ms" => timeout = u64::from(value),
            _ => unreachable!(),
        }
    }
    Err("missing -- EXECUTABLE".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn arguments_are_passed_literally_after_separator() {
        let spec = parse(arguments(&[
            "--max-restarts",
            "2",
            "--",
            "/bin/echo",
            "two words",
            "$(whoami)",
            "--help",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(spec.args, arguments(&["two words", "$(whoami)", "--help"]));
        assert!(matches!(
            spec.restart,
            RestartPolicy::OnFailure {
                max_restarts: 2,
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_cli_before_launching_anything() {
        for args in [
            vec![],
            vec!["--"],
            vec!["--max-restarts", "-1"],
            vec!["--unknown"],
            vec!["--stop-timeout-ms"],
        ] {
            assert!(parse(arguments(&args)).is_err());
        }
        assert!(parse(arguments(&["--help"])).unwrap().is_none());
    }
}
