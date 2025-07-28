# GitHub Copilot Instructions for smtp-acs-bridge

Welcome to the `smtp-acs-bridge` project! This document provides guidance for using GitHub Copilot and contributing to this repository. Please follow these standards to ensure high-quality, maintainable, and reliable code.

## 1. Coding Standards
- **Follow Rust best practices**: Use idiomatic Rust, prefer clarity over cleverness, and leverage the type system for safety.
- **Linting**: All code must pass `cargo clippy` with `-D warnings`.
- **Formatting**: All code must be formatted with `cargo fmt`.
- **Error Handling**: Use `anyhow` for error propagation and `thiserror` or custom error types for domain errors.
- **Logging**: Use the `tracing` crate for all logging. Ensure logs are structured and meaningful.
- **Security**: Never log secrets or sensitive data.

## 2. Testing
- **Unit Tests**: All new logic must be covered by unit tests. Place them in the same module or in the `tests/` directory as appropriate.
- **Integration Tests**: Use the `tests/` directory for end-to-end and integration tests. Mock external services using `wiremock`.
- **Pre-push Hook**: All tests must pass before pushing. The `.git/hooks/pre-push` enforces this.
- **CI/CD**: The GitHub Actions workflow (`.github/workflows/docker.yml`) runs all tests, lints, and checks formatting on every PR and push.

## 3. Documentation
- **README**: The [README.md](./README.md) contains an overview, setup, usage, and configuration instructions. Update it with any user-facing changes.
- **Code Comments**: Document all public functions, structs, and modules using Rust doc comments (`///`).
- **Changelog**: User-facing changes should be described in the release draft and, if needed, in a `CHANGELOG.md`.

## 4. Release Process
- **Release Drafter**: Releases are managed using [Release Drafter](.github/release-drafter.yml). PR titles and labels determine release notes.
- **Versioning**: Follow semantic versioning. Bump the version in `Cargo.toml` for breaking changes, features, or fixes.
- **Docker**: The Docker image is built and published automatically on release.

## 5. Contribution Workflow
- Fork and branch from `main` or `develop`.
- Open a pull request with a clear description of your changes.
- Ensure all checks pass before requesting review.
- Reference issues and link to relevant documentation or RFCs when appropriate.

## 6. References
- [Project README](./README.md)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [tracing crate documentation](https://docs.rs/tracing)
- [wiremock crate documentation](https://docs.rs/wiremock)
- [Release Drafter](https://github.com/release-drafter/release-drafter)

---

**Thank you for contributing to smtp-acs-bridge!**
