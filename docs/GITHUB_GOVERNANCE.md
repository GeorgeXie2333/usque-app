# GitHub repository rules

Documented policy for `GeorgeXie2333/usque-app`. The public repository is
expected to keep Issues on, Discussions off, and blank issues disabled in favor
of the Bug and Feature forms. Private Vulnerability Reporting, the dependency
graph, Dependabot, Secret Scanning, Push Protection, and CodeQL must remain
enabled. Verify live GitHub settings before release; checked-in files establish
workflow behavior, not current server-side settings.

CodeQL uses the default query suite through [`.github/workflows/codeql.yml`](../.github/workflows/codeql.yml) and [`.github/codeql/codeql-config.yml`](../.github/codeql/codeql-config.yml). The workflow analyzes Actions, C/C++, Python, and Rust. It does not analyze Go or JavaScript/TypeScript: first-party Go lives only under the excluded `oracle/**` tree, and there is no first-party JS/TS. CodeQL also does not analyze the Android Kotlin/Java code, the Flutter Dart code, or the retained macOS Swift runner files. Kotlin is covered instead by ktlint, Android lint and unit tests (`:app:ktlintCheck :app:testDebugUnitTest :app:lintDebug`), and Dart by `dart format` and `flutter analyze`, in [CI](../.github/workflows/ci.yml). The macOS Swift files have no static check in CI. The CodeQL configuration also excludes `third_party/**`. A user-owned repository cannot set the `github-codeql-config-file` property that default setup needs to load a config file, so this repository uses the workflow instead of default setup. Default setup is off so GitHub accepts the workflow's uploads.

Do not weaken a required check or invent a passing status to satisfy a ruleset.

## Permissions and Actions

- Default `GITHUB_TOKEN` permission is read repository contents. Grant write only to a job that needs it.
- Do not send Actions secrets to pull requests from forks. First-time external workflow runs need approval.
- Pin external Actions and reusable workflows to full commit SHAs.

Conduct and security reports use GitHub Private Vulnerability Reporting. See [SECURITY.md](../SECURITY.md) and [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md).

## `main` ruleset

The ruleset on `~DEFAULT_BRANCH` must require:

- a pull request before merge;
- no mandatory approving review while there is only one maintainer with write access;
- `CODEOWNERS` for routing; required code-owner review only after a second maintainer is available;
- the branch up to date before merge;
- status checks `PR Check / gate`, `CI / gate`, and `Build / gate`;
- review conversations resolved;
- squash merge only, with linear history;
- no force pushes and no branch deletion.

The owner can bypass the ruleset. Bypass must not be used to publish a release that failed signing, provenance, or artifact checks.

## Release tags and environments

Details are in [RELEASE.md](RELEASE.md):

- only the release maintainer creates the current stable tag;
- `release-signing` and `release-publish` need approval;
- signing identities live only in `release-signing` environment secrets;
- a local build cannot replace a failed GitHub Actions candidate.

Before `v0.3.0`, verify the live `main` ruleset, maintainer-only release-tag
restrictions, required environment reviewers and deployment branch/tag rules.
An `environment:` entry in YAML selects an environment; it does not prove that
GitHub approval protection is enabled. Keep those protections when changing
the accepted release tag. The current executable contract is `v0.3.0`
with Android base versionCode `24`; its coordinated inputs and checks are
described in [Preparing v0.3.0](RELEASE.md#preparing-v030).

The tag workflow separately checks that the tagged SHA is current `main` and
has a successful `ci.yml` push run. Publication requires the signed staged
candidate and `release-publish` approval. The four protected reliability jobs
and their summary remain optional and are skipped in the public repository;
their `not_run` or failure status cannot be presented as a pass and is not a
publication prerequisite.

## Pull requests from outside

- A fork pull request gets a read-only token, cannot read secrets, and cannot upload an installable package.
- Dependency Review runs on public pull requests.
- Only packages published by the approved tag workflow are official; the
  checked-in workflow accepts only `v0.3.0`.
