# GitHub repository rules

Current rules for `GeorgeXie2333/usque-app`. The repository is public. Issues are on; Discussions are off. Blank issues are disabled in favor of the Bug and Feature forms. Private Vulnerability Reporting, the dependency graph, Dependabot, Secret Scanning, Push Protection, and CodeQL are enabled.

CodeQL uses the default query suite through [`.github/workflows/codeql.yml`](../.github/workflows/codeql.yml) and [`.github/codeql/codeql-config.yml`](../.github/codeql/codeql-config.yml). The workflow analyzes Actions, C/C++, Python, and Rust. It does not analyze Go or JavaScript/TypeScript: first-party Go lives only under the excluded `oracle/**` tree, and there is no first-party JS/TS. That configuration also excludes `third_party/**`. A user-owned repository cannot set the `github-codeql-config-file` property that default setup needs to load a config file, so this repository uses the workflow instead of default setup. Default setup is off so GitHub accepts the workflow's uploads.

Do not weaken a required check or invent a passing status to satisfy a ruleset.

## Permissions and Actions

- Default `GITHUB_TOKEN` permission is read repository contents. Grant write only to a job that needs it.
- Do not send Actions secrets to pull requests from forks. First-time external workflow runs need approval.
- Pin external Actions and reusable workflows to full commit SHAs.

Conduct and security reports use GitHub Private Vulnerability Reporting. See [SECURITY.md](../SECURITY.md) and [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md).

## `main` ruleset

The active ruleset on `~DEFAULT_BRANCH` requires:

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

## Pull requests from outside

- A fork pull request gets a read-only token, cannot read secrets, and cannot upload an installable package.
- Dependency Review runs on public pull requests.
- The README treats only files from the `v0.2.4` GitHub Release as official.
