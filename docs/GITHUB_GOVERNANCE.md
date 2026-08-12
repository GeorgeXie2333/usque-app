# GitHub repository governance

This is the bootstrap checklist for making `GeorgeXie2333/usque-app` public. Repository settings are intentionally applied only after the community files and unprivileged workflows have passed on a private validation Pull Request.

## Private validation

1. Keep the repository private and create a branch containing the community and workflow changes.
2. Open a Pull Request into `main`.
3. Confirm the stable contexts exist:
   - `PR Check / gate`
   - `CI / gate`
   - `Build / gate`
4. Confirm Draft Pull Requests run only PR Check; mark the validation PR Ready and confirm CI and Build then run.
5. Confirm Dependency Review is skipped while the personal repository remains private and GitHub Code Security is unavailable.
6. Confirm no workflow run receives release secrets, uploads a development binary, installs an MSI, starts VPN/TUN, or changes runner network state.

Do not create a fake passing status or weaken a workflow to bootstrap the ruleset.

## Public switch

After the validation change is merged and pushed:

1. Review the complete Git history for credentials, signing material, diagnostics, personal information, and binaries.
2. Change repository visibility to **Public**.
3. Enable Issues and leave Discussions disabled.
4. Disable blank Issues; retain the checked-in Bug and Feature forms.
5. Enable Private Vulnerability Reporting and test the **Report a vulnerability** button.
6. Enable the dependency graph, Dependabot alerts, Dependabot security updates, Secret Scanning, and Push Protection.
7. Enable CodeQL Default Setup for every supported language detected by GitHub.
8. Set the default `GITHUB_TOKEN` permission to read repository contents; do not grant write permission unless an individual protected job requires it.
9. Do not send Actions secrets to Pull Requests from forks. Require approval for first-time external workflow runs.
10. Require external Actions and reusable workflows to be pinned to full commit SHAs.

The conduct and security links intentionally use GitHub Private Vulnerability Reporting and become operational only after this switch.

## Main ruleset

Create an active repository ruleset targeting `~DEFAULT_BRANCH` with:

- a Pull Request required before merge;
- no mandatory approving review while the repository has only one active
  maintainer with write permission;
- `CODEOWNERS` retained for ownership routing, with mandatory Code Owner review
  enabled only after a second independent maintainer is available;
- the branch required to be up to date before merge;
- required status checks `PR Check / gate`, `CI / gate`, and `Build / gate`;
- all review conversations resolved;
- squash merge as the only merge method and linear history required;
- force pushes and branch deletion blocked.

The repository owner retains bypass permission. Bypass does not manufacture successful checks and must never be used to publish a release that failed signing, provenance, artifact-integrity, or release-contract gates.

## Release tags and environments

Keep the protected release contract in [RELEASE.md](RELEASE.md):

- protect the exact stable tag used by the current release workflow;
- allow only the release maintainer to create a release tag;
- require approval for `release-signing` and `release-publish` environments;
- keep signing identities only in `release-signing` environment secrets;
- never substitute a local build for a failed GitHub Actions candidate.

## Verification after publication

- GitHub Community Standards recognizes README, License, Code of Conduct, Contributing, Security, Issue Forms, and the Pull Request template.
- A public test PR runs Dependency Review and rejects a deliberately vulnerable fixture before that fixture is merged.
- A fork test PR has a read-only token, cannot read secrets, and cannot upload installable output.
- CodeQL, Dependabot, Secret Scanning, and Push Protection show as enabled under repository security settings.
- The README names only artifacts published by the protected release workflow as official.
