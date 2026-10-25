# Release runbook

CreditBar ships these artifacts from GitHub Releases:

| Platform | Architecture | Artifacts | Runner |
| --- | --- | --- | --- |
| macOS | universal (Apple Silicon + Intel) | `.dmg` | `macos-14` |
| Linux | x64 | AppImage, `.deb` | `ubuntu-22.04` (older glibc, so the AppImage runs widely) |

Windows is not built. The version lives in `crates/app/tauri.conf.json`, the workspace
`Cargo.toml` and `Cargo.lock`; `scripts/release-version.mjs set` keeps the three in step.

## Signing

macOS builds are ad-hoc signed and not notarized, because there is no Apple Developer account.
Gatekeeper reports a downloaded build as damaged; clear the quarantine flag once:

```bash
xattr -dr com.apple.quarantine /Applications/CreditBar.app
```

Linux packages are unsigned; check them against `SHA256SUMS.txt`.

## Publish a release

1. Make sure `main` holds everything for the release and CI passes.
2. Run **Release** from `main` under GitHub Actions → Release → Run workflow. Set **version**
   to `patch`, `minor`, `major`, or an exact version such as `1.2.0`. For a beta, see
   [Publish a beta](#publish-a-beta).

   While a beta is out, a release must sort above it. With `0.3.0-beta.2` out, `patch`
   (`0.2.1`) fails and names `0.3.0` instead, so release `minor`.
3. The workflow:
   1. resolves the version on top of `main`, and fails if its tag already exists or a higher
      version is already out;
   2. runs `cargo fmt --check`, clippy with warnings as errors and the tests against the bumped
      tree, while it packages macOS and Linux at the same time;
   3. commits `chore(release): prepare vX.Y.Z` to `main` and pushes it with the `vX.Y.Z` tag in
      one atomic push;
   4. drafts the GitHub release with notes generated since the previous release, every
      artifact, and `SHA256SUMS.txt`;
   5. publishes the draft, marked as the latest release.
4. Download the artifacts and check them against `SHA256SUMS.txt`.

Nothing is committed or tagged until all checks and both packaging jobs pass, so a failed run
leaves the repository unchanged. Fix the cause on `main` and run the workflow again. If someone
pushes to `main` during the run, the atomic push fails; run the workflow again to release the
newer commit.

The first release can name the version already in `tauri.conf.json` (`0.1.0`): with nothing to
bump, the workflow tags the current commit without a release commit.

### Pushing to a protected main

Today `main` has no ruleset, and the release job pushes with the workflow token. Once `main`
only takes pull requests, add a write deploy key, store its private key as the
`RELEASE_DEPLOY_KEY` secret, and make **Deploy keys** the ruleset's only bypass actor. The
release job then pushes with that key. Without it, the push is rejected with
`GH013: Repository rule violations` and nothing is committed or tagged.

## Publish a beta

1. Make sure `main` holds the changes and CI passes.
2. Run **Release** from `main` with **version** set to `beta`. The workflow picks the next
   minor version's next beta: with `main` at `0.2.0`, that is `0.3.0-beta.1`, then
   `0.3.0-beta.2`. An exact version such as `0.3.0-beta.5` works too; only `-beta.N` versions
   are accepted.
3. The workflow runs the same gates and packaging as a release. It then commits the version on
   top of `main`, tags it, and pushes only the tag, so `main` does not take the beta's version.
   The release is marked as a prerelease, and its notes list what changed since the version
   before it.
4. When the betas hold up, [publish a release](#publish-a-release) with `minor`.

To try one branch on its own, run the workflow from that branch with `beta` and
**preview_branch** checked. Without it, a beta from a branch other than `main` fails.

## Local builds

`cargo tauri build` (or `npx @tauri-apps/cli@2 build`) in `crates/app` writes installers for
the current OS to `target/release/bundle/` and never publishes anything.

## Rollback

To pull a broken release, delete it or mark it as a prerelease, then fix forward with a higher
version. Keep earlier installers until the replacement is confirmed.
