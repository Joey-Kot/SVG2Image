use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn completion_runs_without_conversion_arguments() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let output = Command::new(env!("CARGO_BIN_EXE_svg2image"))
            .args(["completion", shell])
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {:?}", output.stderr);
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("--_complete")
        );
    }

    let mut process = Command::new(env!("CARGO_BIN_EXE_svg2image"))
        .args(["--_complete", "words"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(b"svg2image\0--format\0JP\0")
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let values = String::from_utf8(output.stdout).unwrap();
    assert!(values.lines().any(|value| value == "JPG"));
    assert!(values.lines().any(|value| value == "JPEG"));
}
