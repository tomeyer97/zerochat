# Maintainers

| Name | GitHub | Role |
|---|---|---|
| Torstein Meyer | [@tomeyer97](https://github.com/tomeyer97) | Sole maintainer |

The maintainer:

- decides what goes in and where the project goes;
- reviews and merges pull requests ([CONTRIBUTING.md](CONTRIBUTING.md#how-changes-land));
- cuts releases and publishes to npm ([RELEASING.md](RELEASING.md));
- handles security reports ([SECURITY.md](SECURITY.md)) and enforces the
  [Code of Conduct](CODE_OF_CONDUCT.md).

## How decisions are made

- The maintainer decides.
- Small changes: open a pull request.
- Larger changes (new features, new dependencies, changes to the API or the UI's promises in
  [DESIGN.md](DESIGN.md)): start with a [Discussion](https://github.com/tomeyer97/zerochat/discussions) or
  an issue, and agree on the approach before writing the code.

## Bus factor: one

There is one maintainer. If they are unavailable, nobody else can merge, release or publish to npm. Pull
requests and issues wait until they are back. How access is held and what happens if the maintainer
disappears: [docs/maintainers/access.md](docs/maintainers/access.md).

## Becoming a maintainer

The project wants a second maintainer. The path: a record of good reviews and merged pull requests over
some months, then the maintainer invites you. A new maintainer gets the access described in
[docs/maintainers/access.md](docs/maintainers/access.md) and is added to this file and
[.github/CODEOWNERS](.github/CODEOWNERS).
