# Security policy

Diktator handles microphone audio, dictated text and the system clipboard, so security and privacy reports are taken seriously.

## Supported versions

Security fixes go into the latest release. Please update before reporting.

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report privately through GitHub: open this repository's **Security** tab and choose **Report a vulnerability**. Include:

- what the problem is and its impact,
- steps to reproduce, with your OS version and Diktator version,
- any proof-of-concept code or logs (never include real private dictations).

You can expect an acknowledgement within a few days. Once a fix is ready, it is released and the advisory is published, crediting you unless you prefer otherwise.

## In scope

Anything that breaks the promises in [docs/privacy.md](docs/privacy.md), for example:

- audio, transcripts or rewritten text leaving the device, being written to disk, or appearing in logs,
- network access outside the model downloader,
- model downloads that could be tampered with (bypassing the SHA-256 check or the pinned URLs),
- clipboard data being lost or exposed to other apps beyond the paste itself,
- code execution through settings files, downloaded model files, or the settings window.

Bugs in the upstream projects Diktator builds on (Tauri, sherpa-onnx, llama.cpp, ONNX Runtime) should also go to those projects, but a heads-up here is welcome.
