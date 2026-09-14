# Governance

This project is maintained under the [intersystems-community](https://github.com/intersystems-community)
organization.

## Maintainers

The maintainers listed in [.github/CODEOWNERS](.github/CODEOWNERS) are responsible
for reviewing contributions, triaging issues, and cutting releases. They are
automatically requested for review on every pull request.

## Decision making

Changes are proposed and discussed through issues and pull requests. Maintainers
make the final decision on what is merged, favoring rough consensus among
contributors.

## Contributions

All changes reach `main` through a pull request — direct pushes are disabled.

- Every PR requires review approval from a code owner before merging.
- Pull requests from external contributors are approved by a development
  manager; contributions from employees may be approved by any maintainer.
- Automated checks (see [.github/workflows/ci.yml](.github/workflows/ci.yml))
  must pass before a PR is merged.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow and
[SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## Releases

Releases are cut from `main` and follow [Semantic Versioning 2.0](https://semver.org).
