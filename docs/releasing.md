# Releasing Luminatti

Luminatti releases are built by `cargo-dist` when a semantic version tag is
pushed. The workflow creates the GitHub release, uploads native macOS archives,
and updates the Homebrew formula in `jteso/homebrew-tap`.

## One-time Homebrew setup

Create the public tap repository:

```sh
gh repo create jteso/homebrew-tap --public --add-readme
```

Create a fine-grained GitHub personal access token that has **Contents: Read and
write** access to `jteso/homebrew-tap`. Add it to the `jteso/luminatti-cli`
repository as the Actions secret `HOMEBREW_TAP_TOKEN`. The release workflow uses
that secret solely to commit the generated `Formula/luminatti-cli.rb` file to the
tap.

## Publishing a release

1. Set the new version in `Cargo.toml` and update `Cargo.lock` if Cargo changes
   it.
2. Commit the release changes, then create and push a matching semantic version
   tag, for example `v0.1.0`.
3. The `Release` workflow builds Apple Silicon and Intel macOS archives, creates
   the GitHub release, and updates the tap formula.

After the workflow succeeds, users install the published version with:

```sh
brew install jteso/tap/luminatti-cli
```
