# Evanescent

A Kubernetes operator for ephemeral environments.

This starter crate defines the default preview lifetime. It does not yet run
a Kubernetes controller or install Helm charts.

## Development

```sh
cargo test --locked
```

Pull requests run formatting, Clippy, tests, documentation, and the shared
[Rust structure](https://github.com/caelurium/rust-structure) check.

## Releases

After checks pass on `main`, Release Please uses Conventional Commits to
update a release PR. Merging that PR publishes a GitHub release and tag; it
does not publish to crates.io.

To enable publishing, configure the `RELEASE_PLEASE_TOKEN` Actions secret
with access to create release PRs and publish releases in this repository.
